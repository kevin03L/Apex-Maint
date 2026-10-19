//! Régressions des constats d'audit sur des bases temporaires.
use crate::{maintenance, stock, Actor, MaintError, Role};
use serde_json::json;

pub(crate) async fn fixture() -> (sqlx::SqlitePool, Actor) {
    let pool = crate::db::open(None).await.unwrap();
    crate::services::Auth::new(pool.clone(), 24)
        .initialize(false)
        .await
        .unwrap();
    let user: String = sqlx::query_scalar("SELECT id FROM users LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    for statement in [
        "INSERT INTO sites(id,name,created_at,updated_at) VALUES('a','A','2026','2026'),('b','B','2026','2026')",
        "INSERT INTO equipments(id,site_id,code,name,created_at,updated_at) VALUES('eq','a','EQ','Machine','2026','2026')",
        "INSERT INTO locations(id,site_id,code,name,kind,created_at,updated_at) VALUES('loc','a','LOC','Magasin','magasin','2026','2026')",
        "INSERT INTO parts(id,site_id,reference,name,created_at,updated_at) VALUES('part','a','P','Pièce','2026','2026'),('autre','b','P','Autre','2026','2026')",
        "INSERT INTO stock(id,site_id,part_id,location_id,quantity,updated_at) VALUES('stock','a','part','loc',1,'2026')",
    ] { sqlx::query(statement).execute(&pool).await.unwrap(); }
    (pool, Actor::new(&user, None, Role::Admin, []))
}

pub(crate) async fn plan(pool: &sqlx::SqlitePool, actor: &Actor) -> maintenance::MaintenancePlan {
    maintenance::create_plan(pool, actor, serde_json::from_value(json!({
        "site_id":"a", "equipment_id":"eq", "code":"PLAN", "title":"Entretien",
        "frequency":"daily", "anchor_date":"2020-01-01", "next_due_date":"2026-10-19", "auto_generate":true
    })).unwrap(), None).await.unwrap()
}

#[tokio::test]
async fn annulation_refuse_emplacement_supprime_et_piece_autre_site() {
    let (pool, actor) = fixture().await;
    let intervention = crate::interventions::create_intervention(
        &pool,
        &actor,
        serde_json::from_value(
            json!({"site_id":"a", "title":"Incident", "type":"hors_equipement"}),
        )
        .unwrap(),
        None,
    )
    .await
    .unwrap();
    let input = json!({"part_id":"autre", "location_id":"loc", "quantity":1});
    for error in [
        stock::consume_part_on_intervention(
            &pool,
            &actor,
            &intervention.id,
            serde_json::from_value(input.clone()).unwrap(),
            None,
        )
        .await
        .unwrap_err(),
        stock::create_stock_reservation(
            &pool,
            &actor,
            &intervention.id,
            serde_json::from_value(input).unwrap(),
            None,
        )
        .await
        .unwrap_err(),
    ] {
        assert!(
            matches!(error, MaintError::BadRequest(ref message) if message.contains("même site"))
        );
    }
    let consumed = stock::consume_part_on_intervention(
        &pool,
        &actor,
        &intervention.id,
        serde_json::from_value(json!({"part_id":"part", "location_id":"loc", "quantity":1}))
            .unwrap(),
        None,
    )
    .await
    .unwrap();
    sqlx::query("UPDATE locations SET deleted_at='2026-10-19' WHERE id='loc'")
        .execute(&pool)
        .await
        .unwrap();
    let error =
        stock::delete_intervention_part(&pool, &actor, &intervention.id, &consumed.id, None)
            .await
            .unwrap_err();
    assert!(matches!(error, MaintError::Conflict(ref message) if message.contains("emplacement")));
    let quantity: i64 = sqlx::query_scalar("SELECT quantity FROM stock WHERE id='stock'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(quantity, 0);
    let deleted: Option<String> =
        sqlx::query_scalar("SELECT deleted_at FROM intervention_parts WHERE id=?")
            .bind(&consumed.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(deleted.is_none());

    // Emplacement rétabli, mais pièce supprimée entre-temps : refus explicite également.
    sqlx::query("UPDATE locations SET deleted_at=NULL WHERE id='loc'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE parts SET deleted_at='2026-10-19' WHERE id='part'")
        .execute(&pool)
        .await
        .unwrap();
    let error =
        stock::delete_intervention_part(&pool, &actor, &intervention.id, &consumed.id, None)
            .await
            .unwrap_err();
    assert!(matches!(error, MaintError::Conflict(ref message) if message.contains("pièce")));
}

#[tokio::test]
async fn adresse_personnelle_modifiable_sans_gestion_des_comptes() {
    let (pool, _) = fixture().await;
    sqlx::query("INSERT INTO users(id,site_id,username,display_name,role,password_hash,created_at,updated_at) VALUES('tech','a','tech','Technicien','technicien','x','2026','2026')")
        .execute(&pool)
        .await
        .unwrap();
    let tech = Actor::new("tech", Some("a".into()), Role::Technicien, []);
    let user = crate::store::update_own_email(&pool, &tech, Some(" tech@exemple.fr ".into()))
        .await
        .unwrap();
    assert_eq!(user.email.as_deref(), Some("tech@exemple.fr"));
    assert!(
        crate::store::update_own_email(&pool, &tech, Some("pas une adresse".into()))
            .await
            .is_err()
    );
    let cleared = crate::store::update_own_email(&pool, &tech, Some(String::new()))
        .await
        .unwrap();
    assert!(cleared.email.is_none());
}

#[tokio::test]
async fn dates_invalides_refusees_et_equipement_immuable() {
    let (pool, actor) = fixture().await;
    for (anchor, next) in [("2026-02-30", "2026-03-01"), ("2026-01-01", "invalide")] {
        let result = maintenance::create_plan(
            &pool,
            &actor,
            serde_json::from_value(json!({
                "site_id":"a", "equipment_id":"eq", "code":"INVALIDE", "title":"Entretien",
                "frequency":"daily", "anchor_date":anchor, "next_due_date":next
            }))
            .unwrap(),
            None,
        )
        .await;
        assert!(matches!(result, Err(MaintError::BadRequest(_))));
    }
    let p = plan(&pool, &actor).await;
    for patch in [
        json!({"anchor_date":"2026-02-30"}),
        json!({"equipment_id":"autre"}),
    ] {
        assert!(matches!(
            maintenance::update_plan(
                &pool,
                &actor,
                &p.id,
                serde_json::from_value(patch).unwrap(),
                None
            )
            .await,
            Err(MaintError::BadRequest(_))
        ));
    }
    let updated = maintenance::update_plan(
        &pool,
        &actor,
        &p.id,
        serde_json::from_value(json!({"anchor_date":" 2026-02-28 "})).unwrap(),
        None,
    )
    .await
    .unwrap();
    assert_eq!(updated.anchor_date.as_deref(), Some("2026-02-28"));
}

#[tokio::test]
async fn projection_quotidienne_apres_plus_de_deux_mille_pas() {
    let (pool, actor) = fixture().await;
    plan(&pool, &actor).await;
    let entries =
        maintenance::get_schedule(&pool, &actor, "2026-10-19", "2026-10-22", Some("a"), None)
            .await
            .unwrap();
    assert_eq!(
        entries
            .iter()
            .map(|entry| entry.date.as_deref().unwrap())
            .collect::<Vec<_>>(),
        ["2026-10-19", "2026-10-20", "2026-10-21", "2026-10-22"]
    );
}

#[tokio::test]
async fn purge_conserve_equipement_avec_etat_externe() {
    let (pool, actor) = fixture().await;
    let temp = tempfile::tempdir().unwrap();
    let files = crate::documents::storage::FileStore::new(temp.path().into(), [0; 32]);
    sqlx::query("UPDATE equipments SET deleted_at='2000' WHERE id='eq'")
        .execute(&pool)
        .await
        .unwrap();
    for statement in [
        "INSERT INTO equipment_external_status(equipment_id,site_id,state,since,received_at) VALUES('eq','a','running','2026','2026')",
        "INSERT INTO equipment_external_status_history(id,equipment_id,site_id,state,since,received_at) VALUES('h','eq','a','running','2026','2026')",
    ] { sqlx::query(statement).execute(&pool).await.unwrap(); }
    for dry in [true, false] {
        let report = crate::exploitation::purge(&pool, &actor, &files, 5, dry)
            .await
            .unwrap();
        assert!(report
            .retained
            .iter()
            .any(|entry| entry.contains("equipments/eq")));
        assert_eq!(report.deleted.get("equipments").copied().unwrap_or(0), 0);
    }
}

#[tokio::test]
async fn lecture_session_recente_pendant_une_transaction_ecriture() {
    let temp = tempfile::tempdir().unwrap();
    let pool = crate::db::open(Some(&temp.path().join("session.db")))
        .await
        .unwrap();
    let auth = crate::services::Auth::new(pool.clone(), 24);
    auth.initialize(false).await.unwrap();
    let (_, token) = auth
        .login("admin", "admin123".into(), None, None)
        .await
        .unwrap();
    let writer = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
    let session =
        tokio::time::timeout(std::time::Duration::from_secs(1), auth.session(&token)).await;
    writer.rollback().await.unwrap();
    assert!(session
        .expect("La lecture de session ne doit pas attendre le verrou d'écriture")
        .is_ok());
    crate::db::close(&pool).await;
}

#[tokio::test]
async fn pagination_plannings_sans_perte_ni_doublon() {
    let (pool, actor) = fixture().await;
    plan(&pool, &actor).await;
    for id in ["p2", "p3"] {
        sqlx::query("INSERT INTO maintenance_plans(id,site_id,code,title,equipment_id,frequency,anchor_date,next_due_date,created_at,updated_at) VALUES(?,'a',?,'Plan','eq','daily','2020-01-01','2026-10-19','2026','2026')")
            .bind(id).bind(id).execute(&pool).await.unwrap();
    }
    let mut cursor = None;
    let mut ids = std::collections::HashSet::new();
    loop {
        let page = maintenance::list_plans(
            &pool,
            &actor,
            maintenance::PlanFilter {
                limit: 1,
                cursor,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(page.items.len(), 1);
        assert!(ids.insert(page.items[0].id.clone()));
        cursor = page
            .next_cursor
            .map(|value| crate::Cursor::decode(&value).unwrap());
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(ids.len(), 3);
}

#[tokio::test]
async fn techniciens_instantane_limites_aux_interventions_transmises() {
    let (pool, actor) = fixture().await;
    let since = crate::store::now();
    for (id, site, updated) in [
        ("visible", "a", since.as_str()),
        ("ancienne", "a", "2000"),
        ("hors-site", "b", since.as_str()),
    ] {
        sqlx::query("INSERT INTO interventions(id,site_id,code,title,type,reported_by_id,reported_at,created_at,updated_at) VALUES(?,?,?,'Travail','hors_equipement',?,'2000','2000',?)")
            .bind(id).bind(site).bind(id).bind(&actor.user_id).bind(updated).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO intervention_technicians(intervention_id,user_id,assigned_at) VALUES(?,?,'2000')")
            .bind(id).bind(&actor.user_id).execute(&pool).await.unwrap();
    }
    let technician = Actor::new(
        &actor.user_id,
        Some("a".into()),
        Role::Technicien,
        crate::Permission::ALL.iter().copied(),
    );
    let snapshot = crate::terrain::snapshot(&pool, &technician, Some(&since))
        .await
        .unwrap();
    assert_eq!(snapshot["interventions"].as_array().unwrap().len(), 1);
    assert_eq!(snapshot["technicians"].as_array().unwrap().len(), 1);
    assert_eq!(snapshot["technicians"][0]["intervention_id"], "visible");
}
