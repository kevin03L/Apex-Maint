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
async fn sous_intervention_exige_modification() {
    let (pool, mut actor) = fixture().await;
    let input = |parent: Option<&str>| {
        serde_json::from_value(json!({
            "title":"Incident", "type":"hors_equipement", "parent_id":parent
        }))
        .unwrap()
    };
    let parent = crate::interventions::create_intervention(&pool, &actor, input(None), None)
        .await
        .unwrap();
    actor.permissions.remove(&Permission::InterventionsEdit);
    let error =
        crate::interventions::create_intervention(&pool, &actor, input(Some(&parent.id)), None)
            .await
            .unwrap_err();
    assert_eq!(error.http_status(), 403);
    assert!(
        crate::interventions::create_intervention(&pool, &actor, input(None), None)
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn consommation_ordinaire_refuse_les_pieces_preparees() {
    let (pool, actor) = fixture().await;
    let item = crate::interventions::create_intervention(
        &pool,
        &actor,
        serde_json::from_value(json!({"title":"Panne", "type":"hors_equipement"})).unwrap(),
        None,
    )
    .await
    .unwrap();
    for sql in [
        "INSERT INTO parts(id,site_id,reference,name,created_at,updated_at) VALUES('p','s','P','Pièce','2026','2026')",
        "INSERT INTO locations(id,site_id,code,name,kind,created_at,updated_at) VALUES('l','s','L','Magasin','magasin','2026','2026')",
        "INSERT INTO stock(id,site_id,part_id,location_id,quantity,updated_at) VALUES('stock','s','p','l',5,'2026')",
    ] { sqlx::query(sql).execute(&pool).await.unwrap(); }
    sqlx::query("INSERT INTO stock_reservations(id,site_id,intervention_id,part_id,location_id,quantity,status,prepared_by_id,prepared_at,created_at,updated_at) VALUES('r','s',?,'p','l',5,'prepared',?,'2026','2026','2026')")
        .bind(&item.id).bind(&actor.user_id).execute(&pool).await.unwrap();
    let error = crate::stock::consume_part_on_intervention(
        &pool,
        &actor,
        &item.id,
        serde_json::from_value(json!({"part_id":"p","location_id":"l","quantity":5})).unwrap(),
        None,
    )
    .await
    .unwrap_err();
    assert_eq!(error.http_status(), 409);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT quantity FROM stock WHERE id='stock'")
            .fetch_one(&pool)
            .await
            .unwrap(),
        5
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT status FROM stock_reservations WHERE id='r'")
            .fetch_one(&pool)
            .await
            .unwrap(),
        "prepared"
    );
}

#[tokio::test]
async fn occurrences_compteur_distinctes_apres_remise_a_zero() {
    let (pool, actor) = fixture().await;
    for sql in [
        "INSERT INTO equipments(id,site_id,code,name,created_at,updated_at) VALUES('e','s','E','Machine','2026','2026')",
        "INSERT INTO equipment_counters(id,site_id,equipment_id,name,unit,current_value,created_at,updated_at) VALUES('c','s','e','Heures','h',100,'2026','2026')",
        "INSERT INTO maintenance_plans(id,site_id,code,title,equipment_id,frequency,counter_id,counter_interval,next_due_counter,created_at,updated_at) VALUES('p','s','P','Entretien','e','counter','c',100,200,'2026','2026')",
        "INSERT INTO maintenance_occurrences(id,site_id,plan_id,due_counter,status,created_at,updated_at) VALUES('o1','s','p',100,'done','2026','2026')",
    ] { sqlx::query(sql).execute(&pool).await.unwrap(); }
    crate::maintenance::record_counter_reading(
        &pool,
        &actor,
        "c",
        serde_json::from_value(json!({"value":0,"is_reset":true})).unwrap(),
        None,
    )
    .await
    .unwrap();
    crate::maintenance::record_counter_reading(
        &pool,
        &actor,
        "c",
        serde_json::from_value(json!({"value":100})).unwrap(),
        None,
    )
    .await
    .unwrap();
    assert_eq!(sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM maintenance_occurrences WHERE plan_id='p' AND status='generated' AND due_counter=100 AND intervention_id IS NOT NULL").fetch_one(&pool).await.unwrap(), 1);
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(DISTINCT counter_cycle) FROM maintenance_occurrences WHERE plan_id='p'"
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        2
    );
    assert!(sqlx::query("INSERT INTO maintenance_occurrences(id,site_id,plan_id,due_counter,status,created_at,updated_at) VALUES('o3','s','p',100,'done','2026','2026')").execute(&pool).await.is_err());
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

#[tokio::test]
async fn dates_d_intervention_analysees_et_normalisees() {
    let (pool, actor) = fixture().await;
    sqlx::query("INSERT INTO equipments(id,site_id,code,name,created_at,updated_at) VALUES('eq','s','EQ','Presse',?,?)")
        .bind(crate::store::now())
        .bind(crate::store::now())
        .execute(&pool)
        .await
        .unwrap();
    let posteriori = |started_at: &str, completed_at: &str| {
        serde_json::from_value::<crate::interventions::CreateIntervention>(json!({
            "title": "A posteriori",
            "type": "a_posteriori",
            "equipment_id": "eq",
            "solution_report": "Fait",
            "duration_minutes": 30,
            "reported_at": started_at,
            "started_at": started_at,
            "completed_at": completed_at,
        }))
        .unwrap()
    };
    // Une date non RFC 3339 (ici du HTML) est refusée avant toute écriture.
    let err = crate::interventions::create_intervention(
        &pool,
        &actor,
        posteriori("2026-10-01T08:00:00Z", "<b id='audit'>injecté</b>"),
        None,
    )
    .await
    .unwrap_err();
    assert_eq!(err.http_status(), 400);
    // Une date valide avec décalage est enregistrée en UTC, au format de store::now().
    let item = crate::interventions::create_intervention(
        &pool,
        &actor,
        posteriori("2026-10-01T08:00:00+02:00", "2026-10-01T10:30:00+02:00"),
        None,
    )
    .await
    .unwrap();
    assert_eq!(item.reported_at, "2026-10-01T06:00:00.000Z");
    assert_eq!(
        item.completed_at.as_deref(),
        Some("2026-10-01T08:30:00.000Z")
    );
    // Fin antérieure au début : refusée.
    let err = crate::interventions::create_intervention(
        &pool,
        &actor,
        posteriori("2026-10-01T10:00:00Z", "2026-10-01T09:00:00Z"),
        None,
    )
    .await
    .unwrap_err();
    assert_eq!(err.http_status(), 400);
}
