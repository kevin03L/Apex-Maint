use crate::{terrain::*, Actor, Permission, Role};
use chrono::{DateTime, Duration, Utc};
use serde_json::json;

async fn fixture() -> (sqlx::SqlitePool, Actor) {
    let pool = crate::db::open(None).await.unwrap();
    for id in ["s", "autre"] {
        sqlx::query("INSERT INTO sites(id,name,created_at,updated_at) VALUES(?,?,?,?)")
            .bind(id)
            .bind(id)
            .bind(crate::store::now())
            .bind(crate::store::now())
            .execute(&pool)
            .await
            .unwrap();
    }
    crate::services::Auth::new(pool.clone(), 24)
        .initialize(false)
        .await
        .unwrap();
    let user_id = sqlx::query_scalar("SELECT id FROM users LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    (
        pool,
        Actor {
            user_id,
            site_id: Some("s".into()),
            role: Role::Technicien,
            permissions: Permission::ALL.iter().copied().collect(),
        },
    )
}

#[tokio::test]
async fn dates_precondition_compte_rendu_et_reponse_atomique() {
    let (pool, actor) = fixture().await;
    let input = || {
        serde_json::from_value(
            json!({"title":"Terrain","type":"hors_equipement","technician_ids":[actor.user_id]}),
        )
        .unwrap()
    };
    let date =
        (Utc::now() - Duration::hours(1)).to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let context = Operation {
        key: Some(crate::store::id()),
        route: "POST /interventions".into(),
        occurred_at: Some(date.clone()),
        ..Default::default()
    };
    let item = OPERATION
        .scope(
            context.clone(),
            crate::interventions::create_intervention(&pool, &actor, input(), None),
        )
        .await
        .unwrap();
    assert_eq!(item.reported_at, date);
    let saved: String =
        sqlx::query_scalar("SELECT response_body FROM idempotency_keys WHERE key=?")
            .bind(context.key)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&saved).unwrap(),
        json!(item)
    );
    let error = OPERATION
        .scope(
            Operation {
                unmodified_since: Some("ancienne".into()),
                ..Default::default()
            },
            crate::interventions::update_intervention(
                &pool,
                &actor,
                &item.id,
                Default::default(),
                None,
            ),
        )
        .await
        .unwrap_err();
    assert_eq!(error.http_status(), 412);
    let updated = crate::interventions::update_intervention(
        &pool,
        &actor,
        &item.id,
        serde_json::from_value(json!({"solution_report":"Diagnostic terrain"})).unwrap(),
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        updated.solution_report.as_deref(),
        Some("Diagnostic terrain")
    );
    let mut tx = pool.begin().await.unwrap();
    let fallback = OPERATION
        .scope(
            Operation {
                occurred_at: Some("2000-01-01T00:00:00Z".into()),
                ..Default::default()
            },
            occurred(&mut tx, &actor, Some(&date)),
        )
        .await
        .unwrap()
        .unwrap();
    assert!(DateTime::parse_from_rfc3339(&fallback).unwrap() > Utc::now() - Duration::minutes(1));
    tx.commit().await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM audit_log WHERE action='offline_date_replaced'"
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        1
    );
    // Un échec de stockage de la réponse annule aussi l'intervention et son audit.
    sqlx::query("CREATE TRIGGER refuse_idempotence BEFORE INSERT ON idempotency_keys BEGIN SELECT RAISE(ABORT,'refus'); END").execute(&pool).await.unwrap();
    assert!(OPERATION
        .scope(
            Operation {
                key: Some(crate::store::id()),
                route: "POST /interventions".into(),
                ..Default::default()
            },
            crate::interventions::create_intervention(&pool, &actor, input(), None)
        )
        .await
        .is_err());
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM interventions")
            .fetch_one(&pool)
            .await
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn instantane_site_delta_taille_et_sans_secrets() {
    let (pool, actor) = fixture().await;
    let before = crate::store::now();
    let mut tx = pool.begin().await.unwrap();
    for index in 0..5000 {
        sqlx::query("INSERT INTO equipments(id,site_id,code,name,created_at,updated_at) VALUES(?,'s',?,'Presse',?,?)").bind(format!("eq-{index}" )).bind(format!("P-{index}")).bind(&before).bind(&before).execute(&mut *tx).await.unwrap();
    }
    sqlx::query("INSERT INTO equipments(id,site_id,code,name,created_at,updated_at) VALUES('hors-site','autre','INVISIBLE','Ne pas révéler',?,?)").bind(&before).bind(&before).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let full = snapshot(&pool, &actor, None).await.unwrap();
    assert_eq!(full["equipments"].as_array().unwrap().len(), 5000);
    let text = full.to_string();
    assert!(text.len() < 2 * 1024 * 1024);
    assert!(!text.contains("INVISIBLE"));
    assert!(!text.contains("password_hash"));
    assert!(!text.contains("username"));
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    let since = crate::store::now();
    assert_eq!(
        snapshot(&pool, &actor, Some(&since)).await.unwrap()["equipments"],
        json!([])
    );
    sqlx::query("UPDATE equipments SET deleted_at=?,updated_at=? WHERE id='eq-0'")
        .bind(&since)
        .bind(&since)
        .execute(&pool)
        .await
        .unwrap();
    let delta = snapshot(&pool, &actor, Some(&since)).await.unwrap();
    assert_eq!(delta["full"], false);
    assert_eq!(delta["equipments"].as_array().unwrap().len(), 1);
    assert!(delta["equipments"][0]["deleted_at"].is_string());
    assert_eq!(
        snapshot(&pool, &actor, Some("2000-01-01T00:00:00Z"))
            .await
            .unwrap()["full"],
        true
    );
}
