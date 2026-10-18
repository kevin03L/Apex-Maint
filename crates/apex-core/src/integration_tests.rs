//! Tests unitaires de la liaison Supervision (apex-core).
use crate::{
    domain::password::token_hash,
    integration::*,
    maintenance::{self, CreateMaintenancePlan},
    referentiel::{self, CreateEquipment},
    Actor, Permission, Role,
};
use chrono::{Duration, Utc};
use serde_json::json;

async fn fixture() -> (sqlx::SqlitePool, Actor, Actor) {
    let pool = crate::db::open(None).await.unwrap();
    for id in ["site-a", "site-b"] {
        sqlx::query("INSERT INTO sites(id,name,created_at,updated_at) VALUES(?,?,?,?)")
            .bind(id)
            .bind(format!("Site {id}"))
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
    let admin_id: String = sqlx::query_scalar("SELECT id FROM users LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();

    let admin = Actor {
        user_id: admin_id.clone(),
        site_id: None,
        role: Role::Admin,
        permissions: Permission::ALL.iter().copied().collect(),
    };
    let tech = Actor {
        user_id: admin_id,
        site_id: Some("site-a".into()),
        role: Role::Technicien,
        permissions: [
            Permission::SettingsView,
            Permission::EquipmentsView,
            Permission::MaintenanceView,
        ]
        .into_iter()
        .collect(),
    };
    (pool, admin, tech)
}

#[tokio::test]
async fn parametres_et_cles_cycle_complet() {
    let (pool, admin, tech) = fixture().await;

    // 1. Lecture des paramètres par défaut
    let s = settings(&pool, &admin).await.unwrap();
    assert_eq!(s["integration.supervision.enabled"], false);
    assert_eq!(s["integration.supervision.counters_enabled"], true);

    // Tech a SettingsView -> OK en lecture
    assert!(settings(&pool, &tech).await.is_ok());
    // Tech n'a pas SettingsEdit -> refus écriture
    assert!(save_settings(
        &pool,
        &tech,
        json!({"integration.supervision.enabled": true})
    )
    .await
    .is_err());

    // Activation par l'admin
    let updated = save_settings(
        &pool,
        &admin,
        json!({
            "integration.supervision.enabled": true,
            "integration.supervision.counter_min_interval_min": 10
        }),
    )
    .await
    .unwrap();
    assert_eq!(updated["integration.supervision.enabled"], true);
    assert_eq!(
        updated["integration.supervision.counter_min_interval_min"],
        10
    );

    // Paramètre invalide -> 400
    assert!(save_settings(
        &pool,
        &admin,
        json!({"integration.supervision.enabled": 42})
    )
    .await
    .is_err());

    // 2. Création de clés
    // Expiration passée refusée
    let past = (Utc::now() - Duration::hours(1)).to_rfc3339();
    assert!(create_key(
        &pool,
        &admin,
        KeyInput {
            site_id: Some("site-a".into()),
            label: "Clé expirée".into(),
            scopes: vec!["counters".into()],
            expires_at: Some(past),
        }
    )
    .await
    .is_err());

    // Clé valide avec portée counters
    let res = create_key(
        &pool,
        &admin,
        KeyInput {
            site_id: Some("site-a".into()),
            label: "Supervision Atelier A".into(),
            scopes: vec!["counters".into()],
            expires_at: None,
        },
    )
    .await
    .unwrap();
    let raw_key = res["key"].as_str().unwrap().to_string();
    assert!(raw_key.starts_with("apx_"));
    assert_eq!(raw_key.len(), 47);
    let key_id = res["item"]["id"].as_str().unwrap().to_string();
    assert_eq!(res["item"]["key_prefix"], &raw_key[..8]);

    // La clé hachée est bien en base
    let hash_in_db: String = sqlx::query_scalar("SELECT key_hash FROM integration_keys WHERE id=?")
        .bind(&key_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(hash_in_db, token_hash(&raw_key));

    // Authentification de la clé
    let principal = authenticate(&pool, &raw_key, "counters", "127.0.0.1")
        .await
        .unwrap();
    assert_eq!(principal.key_id, key_id);
    assert_eq!(principal.actor.site_id.as_deref(), Some("site-a"));

    // Échec sur mauvaise portée
    assert!(authenticate(&pool, &raw_key, "status", "127.0.0.1")
        .await
        .is_err());

    // Modification de portée vers ["counters", "status"]
    let updated_key = update_key(
        &pool,
        &admin,
        &key_id,
        Some(vec!["counters".into(), "status".into()]),
        false,
    )
    .await
    .unwrap();
    assert_eq!(updated_key.scopes, vec!["counters", "status"]);

    // Désormais l'authentification réussit pour les deux
    assert!(authenticate(&pool, &raw_key, "status", "127.0.0.1")
        .await
        .is_ok());

    // Révocation de la clé
    let revoked = update_key(&pool, &admin, &key_id, None, true)
        .await
        .unwrap();
    assert!(revoked.revoked_at.is_some());

    // La clé révoquée est refusée
    assert!(authenticate(&pool, &raw_key, "counters", "127.0.0.1")
        .await
        .is_err());

    // Vérification de l'audit
    let audits: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_log WHERE entity='integration_key' AND entity_id=?",
    )
    .bind(&key_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(audits >= 3); // create, update (scopes), revoke
}

#[tokio::test]
async fn ingestion_compteurs_seuil_reset_espacement_et_idempotence() {
    let (pool, admin, _) = fixture().await;
    save_settings(
        &pool,
        &admin,
        json!({
            "integration.supervision.enabled": true,
            "integration.supervision.counter_min_interval_min": 15
        }),
    )
    .await
    .unwrap();

    // Créer un équipement et un compteur sur site-a
    let eq = referentiel::create_equipment(
        &pool,
        &admin,
        CreateEquipment {
            site_id: Some("site-a".into()),
            code: "EQ-SUP-01".into(),
            name: "Machine d'essai".into(),
            status: Some("operational".into()),
            production_line_id: None,
            parent_id: None,
            manufacturer: None,
            model: None,
            serial_number: None,
            commissioned_on: None,
            description: None,
        },
        None,
    )
    .await
    .unwrap();

    // Associer une external_ref à l'équipement
    set_external_ref(&pool, &admin, "equipment", &eq.id, Some("EXT-EQ-01".into()))
        .await
        .unwrap();

    // Créer un compteur sur cet équipement
    let now = Utc::now() - Duration::hours(1);
    let counter_id = crate::store::id();
    sqlx::query("INSERT INTO equipment_counters(id,site_id,equipment_id,name,unit,current_value,active,external_ref,created_at,updated_at) VALUES(?,?,?,?,?,0,1,?,?,?)")
        .bind(&counter_id)
        .bind("site-a")
        .bind(&eq.id)
        .bind("Heures de broche")
        .bind("h")
        .bind("EXT-CNT-01")
        .bind(now.to_rfc3339())
        .bind(now.to_rfc3339())
        .execute(&pool)
        .await
        .unwrap();

    // Créer un planning préventif basé sur ce compteur : seuil 100, lead 10 (déclenchement à 90)
    let plan = maintenance::create_plan(
        &pool,
        &admin,
        CreateMaintenancePlan {
            site_id: Some("site-a".into()),
            code: "PLN-SUP-1".into(),
            title: "Vidange 100h".into(),
            equipment_id: eq.id.clone(),
            description: None,
            instructions: None,
            priority: Some("medium".into()),
            frequency: "counter".into(),
            interval_days: None,
            counter_id: Some(counter_id.clone()),
            counter_interval: Some(100),
            anchor_date: None,
            next_due_date: None,
            anchor_counter: Some(0),
            next_due_counter: Some(100),
            lead_days: None,
            lead_counter: Some(10),
            auto_generate: Some(true),
            active: Some(true),
            estimated_minutes: None,
            default_technician_id: None,
        },
        None,
    )
    .await
    .unwrap();

    // Créer une clé pour le site-a
    let key_res = create_key(
        &pool,
        &admin,
        KeyInput {
            site_id: Some("site-a".into()),
            label: "Supervision".into(),
            scopes: vec!["counters".into()],
            expires_at: None,
        },
    )
    .await
    .unwrap();
    let key_token = key_res["key"].as_str().unwrap();
    let principal = authenticate(&pool, key_token, "counters", "127.0.0.1")
        .await
        .unwrap();

    // 1. Lot mixte : 1 référence inconnue + 1 référence valide (valeur 50)
    let report = ingest_counters(
        &pool,
        &principal.actor,
        &principal.key_id,
        vec![
            Reading {
                counter_ref: "INCONNU".into(),
                value: 10,
                read_at: now.to_rfc3339(),
                reset: false,
            },
            Reading {
                counter_ref: "EXT-CNT-01".into(),
                value: 50,
                read_at: now.to_rfc3339(),
                reset: false,
            },
        ],
    )
    .await
    .unwrap();
    assert_eq!(report.accepted, 1);
    assert_eq!(report.rejected, 1);
    assert_eq!(report.errors.len(), 1);
    assert_eq!(report.errors[0]["counter_ref"], "INCONNU");

    // Valeur actuelle mise à jour à 50
    let curr: i64 = sqlx::query_scalar("SELECT current_value FROM equipment_counters WHERE id=?")
        .bind(&counter_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(curr, 50);

    // Pas encore de déclenchement (seuil à 90)
    let occs: i64 =
        sqlx::query_scalar("SELECT count(*) FROM maintenance_occurrences WHERE plan_id=?")
            .bind(&plan.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(occs, 0);

    // 2. Idempotence : renvoyer la même valeur 50 est accepté sans doublon
    let dup_report = ingest_counters(
        &pool,
        &principal.actor,
        &principal.key_id,
        vec![Reading {
            counter_ref: "EXT-CNT-01".into(),
            value: 50,
            read_at: now.to_rfc3339(),
            reset: false,
        }],
    )
    .await
    .unwrap();
    assert_eq!(dup_report.accepted, 1);
    assert_eq!(dup_report.rejected, 0);

    // 3. Valeur en baisse sans reset -> rejet
    let dec_report = ingest_counters(
        &pool,
        &principal.actor,
        &principal.key_id,
        vec![Reading {
            counter_ref: "EXT-CNT-01".into(),
            value: 40,
            read_at: (now + Duration::minutes(1)).to_rfc3339(),
            reset: false,
        }],
    )
    .await
    .unwrap();
    assert_eq!(dec_report.rejected, 1);
    assert!(dec_report.errors[0]["message"]
        .as_str()
        .unwrap()
        .contains("valeur en baisse"));

    // 4. Espacement : 2 relevés rapprochés (5 min) sans seuil franchi
    // Relevé à 60 (5 min après le premier)
    let t_plus_5 = (now + Duration::minutes(5)).to_rfc3339();
    let r_spaced = ingest_counters(
        &pool,
        &principal.actor,
        &principal.key_id,
        vec![Reading {
            counter_ref: "EXT-CNT-01".into(),
            value: 60,
            read_at: t_plus_5,
            reset: false,
        }],
    )
    .await
    .unwrap();
    assert_eq!(r_spaced.accepted, 1);
    // current_value mis à jour à 60
    let curr: i64 = sqlx::query_scalar("SELECT current_value FROM equipment_counters WHERE id=?")
        .bind(&counter_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(curr, 60);
    // Pas de nouvelle ligne dans counter_readings (espacement de 15 min non respecté)
    let readings_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM counter_readings WHERE counter_id=?")
            .bind(&counter_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(readings_count, 1); // seulement le premier relevé à 50

    // 5. Franchissement de seuil : relevé à 92 (seuil 90 atteint) même dans les 15 min
    let t_plus_10 = (now + Duration::minutes(10)).to_rfc3339();
    let r_threshold = ingest_counters(
        &pool,
        &principal.actor,
        &principal.key_id,
        vec![Reading {
            counter_ref: "EXT-CNT-01".into(),
            value: 92,
            read_at: t_plus_10,
            reset: false,
        }],
    )
    .await
    .unwrap();
    assert!(r_threshold.errors.is_empty(), "{:?}", r_threshold.errors);
    assert_eq!(r_threshold.accepted, 1);
    // La ligne a bien été insérée dans counter_readings malgré l'espacement
    let readings_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM counter_readings WHERE counter_id=?")
            .bind(&counter_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(readings_count, 2);

    // Et l'intervention préventive a été générée automatiquement !
    let occs: i64 =
        sqlx::query_scalar("SELECT count(*) FROM maintenance_occurrences WHERE plan_id=?")
            .bind(&plan.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(occs, 1);

    // 6. Reset : valeur 0 avec reset: true
    let r_reset = ingest_counters(
        &pool,
        &principal.actor,
        &principal.key_id,
        vec![Reading {
            counter_ref: "EXT-CNT-01".into(),
            value: 0,
            read_at: (now + Duration::minutes(20)).to_rfc3339(),
            reset: true,
        }],
    )
    .await
    .unwrap();
    assert_eq!(r_reset.accepted, 1);

    // Vérifier le journal des échanges
    let events = events(&pool, &admin, None, 10).await.unwrap();
    assert!(!events.items.is_empty());
    assert_eq!(events.items[0].kind, "counters");
}

#[tokio::test]
async fn ingestion_statuts_rapprochement_historique_html_et_peremption() {
    let (pool, admin, _) = fixture().await;
    save_settings(
        &pool,
        &admin,
        json!({
            "integration.supervision.enabled": true,
            "integration.supervision.status_stale_after_min": 30
        }),
    )
    .await
    .unwrap();

    // Équipement 1 avec external_ref
    let eq1 = referentiel::create_equipment(
        &pool,
        &admin,
        CreateEquipment {
            site_id: Some("site-a".into()),
            code: "EQ-STATUS-01".into(),
            name: "Ligne assemblage".into(),
            status: Some("operational".into()),
            production_line_id: None,
            parent_id: None,
            manufacturer: None,
            model: None,
            serial_number: None,
            commissioned_on: None,
            description: None,
        },
        None,
    )
    .await
    .unwrap();
    set_external_ref(
        &pool,
        &admin,
        "equipment",
        &eq1.id,
        Some("EXT-EQ-STAT".into()),
    )
    .await
    .unwrap();

    // Équipement 2 sans external_ref (rapprochement par code)
    let eq2 = referentiel::create_equipment(
        &pool,
        &admin,
        CreateEquipment {
            site_id: Some("site-a".into()),
            code: "EQ-BY-CODE".into(),
            name: "Machine secondaire".into(),
            status: Some("operational".into()),
            production_line_id: None,
            parent_id: None,
            manufacturer: None,
            model: None,
            serial_number: None,
            commissioned_on: None,
            description: None,
        },
        None,
    )
    .await
    .unwrap();

    let key_res = create_key(
        &pool,
        &admin,
        KeyInput {
            site_id: Some("site-a".into()),
            label: "Supervision".into(),
            scopes: vec!["status".into()],
            expires_at: None,
        },
    )
    .await
    .unwrap();
    let key_token = key_res["key"].as_str().unwrap();
    let principal = authenticate(&pool, key_token, "status", "127.0.0.1")
        .await
        .unwrap();

    let now = Utc::now();
    let html_message = "Alarme surchauffe : <b>T > 105°C</b> & coupure <script>alert(1)</script>";

    // Ingestion d'états
    let rep = ingest_statuses(
        &pool,
        &principal.actor,
        &principal.key_id,
        vec![
            StatusInput {
                equipment_ref: "EXT-EQ-STAT".into(),
                state: "alarm".into(),
                since: now.to_rfc3339(),
                message: Some(html_message.into()),
            },
            StatusInput {
                equipment_ref: "EQ-BY-CODE".into(),
                state: "stopped".into(),
                since: now.to_rfc3339(),
                message: Some("Arrêt opérateur".into()),
            },
            StatusInput {
                equipment_ref: "INCONNU".into(),
                state: "running".into(),
                since: now.to_rfc3339(),
                message: None,
            },
        ],
    )
    .await
    .unwrap();
    assert_eq!(rep.accepted, 2);
    assert_eq!(rep.rejected, 1);

    // 1. Statut Maint de l'équipement reste INCHANGÉ
    let eq1_after = referentiel::get_equipment(&pool, &admin, &eq1.id)
        .await
        .unwrap();
    assert_eq!(eq1_after.status, "operational");

    // 2. Message contenant du HTML restitué brut
    let st_list = statuses(&pool, &admin, Some(&eq1.id), None, None, 10)
        .await
        .unwrap();
    assert_eq!(st_list.items.len(), 1);
    assert_eq!(st_list.items[0].state, "alarm");
    assert_eq!(st_list.items[0].message.as_deref(), Some(html_message));
    assert!(!st_list.items[0].stale);

    // 3. Historique ne garde que les changements : renvoyer le même état alarm
    let rep2 = ingest_statuses(
        &pool,
        &principal.actor,
        &principal.key_id,
        vec![StatusInput {
            equipment_ref: "EXT-EQ-STAT".into(),
            state: "alarm".into(),
            since: (now + Duration::minutes(2)).to_rfc3339(),
            message: Some("Toujours en alarme".into()),
        }],
    )
    .await
    .unwrap();
    assert_eq!(rep2.accepted, 1);

    let hist = history(&pool, &admin, &eq1.id, None, 10).await.unwrap();
    assert_eq!(hist.items.len(), 1); // Toujours 1 seul changement historisé !

    // Changement d'état vers running
    ingest_statuses(
        &pool,
        &principal.actor,
        &principal.key_id,
        vec![StatusInput {
            equipment_ref: "EXT-EQ-STAT".into(),
            state: "running".into(),
            since: (now + Duration::minutes(5)).to_rfc3339(),
            message: Some("Reprise".into()),
        }],
    )
    .await
    .unwrap();

    let hist2 = history(&pool, &admin, &eq1.id, None, 10).await.unwrap();
    assert_eq!(hist2.items.len(), 2); // 2 changements historisés

    // 4. Widget supervision_status
    // eq2 est stopped, eq1 est running -> le widget ne contient que eq2
    let widget_res = supervision_status(&pool, &admin, None, 10).await.unwrap();
    assert_eq!(widget_res.items.len(), 1);
    assert_eq!(widget_res.items[0].equipment_id, eq2.id);
    // Même contenu servi par le tableau de bord de la phase 7.
    let dashboard =
        crate::pilotage::reports::widget(&pool, &admin, "supervision_status", &Default::default())
            .await
            .unwrap();
    let items = dashboard["data"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["equipment_id"], eq2.id.as_str());
    assert_eq!(items[0]["state"], "stopped");

    // 5. Test de purge
    // Simuler un événement et un historique vieux de 100 jours
    let old = (now - Duration::days(100)).to_rfc3339();
    sqlx::query("INSERT INTO integration_events(id,site_id,key_id,kind,received_at,items_count,accepted,rejected,errors,duration_ms) VALUES(?,?,?,?,?,?,?,?,?,?)")
        .bind(crate::store::id())
        .bind("site-a")
        .bind(&principal.key_id)
        .bind("status")
        .bind(&old)
        .bind(1)
        .bind(1)
        .bind(0)
        .bind("[]")
        .bind(10)
        .execute(&pool)
        .await
        .unwrap();

    sqlx::query("INSERT INTO equipment_external_status_history(id,equipment_id,site_id,source,state,since,message,received_at) VALUES(?,?,?,?,?,?,?,?)")
        .bind(crate::store::id())
        .bind(&eq1.id)
        .bind("site-a")
        .bind("supervision")
        .bind("stopped")
        .bind(&old)
        .bind(None::<String>)
        .bind(&old)
        .execute(&pool)
        .await
        .unwrap();

    let purge_res = purge_history(&pool, &admin).await.unwrap();
    assert_eq!(purge_res["history_deleted"], 1);
    assert_eq!(purge_res["events_deleted"], 1);
    let audited: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_log WHERE action='purge' AND entity='integration_history'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(audited, 1);
}
