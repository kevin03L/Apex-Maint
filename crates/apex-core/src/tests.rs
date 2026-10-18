//! Vérifications métier sur des bases exclusivement en mémoire.
use crate::{
    db,
    services::Auth,
    store::{self, CreateUser},
    Actor, Cursor, MaintError, Permission, Role,
};
use serde_json::{json, Value};

async fn setup() -> (sqlx::SqlitePool, Auth, crate::services::Session) {
    let pool = db::open(None).await.unwrap();
    let auth = Auth::new(pool.clone(), 12);
    auth.initialize(false).await.unwrap();
    let (_, token) = auth
        .login("admin", "admin123".into(), None, None)
        .await
        .unwrap();
    let session = auth.session(&token).await.unwrap();
    (pool, auth, session)
}
fn input(name: &str, role: Role, site: Option<String>) -> CreateUser {
    CreateUser {
        username: name.into(),
        display_name: name.into(),
        role,
        site_id: site,
        password: "Compte-test123".into(),
        active: true,
    }
}

#[tokio::test]
async fn depot_filtre_par_site_et_permissions_par_defaut() {
    let (pool, auth, admin) = setup().await;
    let a = store::save_site(&pool, &admin.actor, None, Some("A"), false)
        .await
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let b = store::save_site(&pool, &admin.actor, None, Some("B"), false)
        .await
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let ua = store::create_user(
        &pool,
        &admin.actor,
        input("responsable-a", Role::Responsable, Some(a.clone())),
    )
    .await
    .unwrap();
    let ub = store::create_user(
        &pool,
        &admin.actor,
        input("production-b", Role::Production, Some(b)),
    )
    .await
    .unwrap();
    let (_, token) = auth
        .login(&ua.username, "Compte-test123".into(), None, None)
        .await
        .unwrap();
    let session = auth.session(&token).await.unwrap();
    assert_eq!(session.actor.permissions.len(), Permission::ALL.len() - 1);
    assert!(!session.actor.can(Permission::SettingsEdit));
    let list = store::users(&pool, &session.actor).await.unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, ua.id);
    assert_eq!(store::sites(&pool, &session.actor).await.unwrap()[0].id, a);
    let result = store::user(&mut pool.acquire().await.unwrap(), &session.actor, &ub.id).await;
    assert!(matches!(result, Err(MaintError::NotFound(_))));
    let result = store::update_user(
        &pool,
        &session.actor,
        &ub.id,
        json!({"active":false}),
        false,
    )
    .await;
    assert!(matches!(result, Err(MaintError::NotFound(_))));
    let result = store::update_user(
        &pool,
        &session.actor,
        &ua.id,
        json!({"role":"admin"}),
        false,
    )
    .await;
    assert!(matches!(result, Err(MaintError::Forbidden(_))));
}

#[tokio::test]
async fn audit_atomique_et_sans_secrets() {
    let (pool, _auth, admin) = setup().await;
    let user = store::create_user(&pool, &admin.actor, input("cible", Role::Admin, None))
        .await
        .unwrap();
    let before: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_log")
        .fetch_one(&pool)
        .await
        .unwrap();
    // Force un échec après la mutation métier, au moment d'écrire l'audit.
    sqlx::query("CREATE TRIGGER refuse_audit BEFORE INSERT ON audit_log BEGIN SELECT RAISE(ABORT,'échec audit simulé'); END").execute(&pool).await.unwrap();
    assert!(store::update_user(
        &pool,
        &admin.actor,
        &user.id,
        json!({"display_name":"Ne doit pas persister"}),
        false
    )
    .await
    .is_err());
    let fresh = store::user(&mut pool.acquire().await.unwrap(), &admin.actor, &user.id)
        .await
        .unwrap();
    assert_eq!(fresh.display_name, "cible");
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_log")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, before);
    sqlx::query("DROP TRIGGER refuse_audit")
        .execute(&pool)
        .await
        .unwrap();
    store::update_user(
        &pool,
        &admin.actor,
        &user.id,
        json!({"display_name":"Nouveau nom"}),
        false,
    )
    .await
    .unwrap();
    let row: (String, String) = sqlx::query_as(
        "SELECT before_json,after_json FROM audit_log WHERE action='update' AND entity_id=?",
    )
    .bind(&user.id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&row.0).unwrap()["display_name"],
        "cible"
    );
    assert_eq!(
        serde_json::from_str::<Value>(&row.1).unwrap()["display_name"],
        "Nouveau nom"
    );
    let rows: Vec<(Option<String>, Option<String>)> =
        sqlx::query_as("SELECT before_json,after_json FROM audit_log")
            .fetch_all(&pool)
            .await
            .unwrap();
    for (before, after) in rows {
        for payload in [before, after].into_iter().flatten() {
            assert!(
                !payload.contains("password_hash")
                    && !payload.contains("$argon2")
                    && !payload.contains("Compte-test123")
                    && !payload.contains("admin123")
            );
        }
    }
}

#[tokio::test]
async fn dernier_admin_et_suppressions_logiques() {
    let (pool, _auth, admin) = setup().await;
    // Un acteur système permet de tester le dernier admin indépendamment du garde-fou personnel.
    let actor = Actor::new("maintenance-test", None, Role::Admin, []);
    for (patch, delete) in [
        (json!({}), true),
        (json!({"active":false}), false),
        (
            json!({"role":"technicien","site_id":store::sites(&pool,&actor).await.unwrap()[0].id}),
            false,
        ),
    ] {
        assert!(matches!(
            store::update_user(&pool, &actor, &admin.user.id, patch, delete).await,
            Err(MaintError::Conflict(_))
        ));
    }
    let other = store::create_user(&pool, &actor, input("autre-admin", Role::Admin, None))
        .await
        .unwrap();
    let (a, b) = tokio::join!(
        store::update_user(&pool, &actor, &admin.user.id, json!({}), true),
        store::update_user(&pool, &actor, &other.id, json!({}), true)
    );
    assert_ne!(a.is_ok(), b.is_ok());
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM users WHERE role='admin' AND active=1 AND deleted_at IS NULL",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 1);
    let deleted = if a.is_ok() { &admin.user } else { &other };
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM users WHERE id=? AND deleted_at IS NOT NULL)",
    )
    .bind(&deleted.id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(exists);
    let replacement = store::create_user(
        &pool,
        &actor,
        input(&deleted.username.to_uppercase(), Role::Admin, None),
    )
    .await
    .unwrap();
    assert_ne!(replacement.id, deleted.id);
}

#[tokio::test]
async fn verrou_atomique_concurrent_et_reinitialisation() {
    let (pool, auth, _admin) = setup().await;
    let tasks: Vec<_> = (0..10)
        .map(|_| {
            let auth = auth.clone();
            tokio::spawn(async move { auth.login("admin", "mauvais".into(), None, None).await })
        })
        .collect();
    let mut denied = 0;
    let mut locked = 0;
    for task in tasks {
        match task.await.unwrap() {
            Err(MaintError::Unauthorized(_)) => denied += 1,
            Err(MaintError::TooManyRequests(_)) => locked += 1,
            other => panic!("Résultat inattendu : {other:?}"),
        }
    }
    assert_eq!((denied, locked), (5, 5));
    let count: i64 = sqlx::query_scalar("SELECT failed_attempts FROM users WHERE username='admin'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 5);
    let locks: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_log WHERE action='lock'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(locks, 1);
    assert!(matches!(
        auth.login("admin", "admin123".into(), None, None).await,
        Err(MaintError::TooManyRequests(_))
    ));
    sqlx::query("UPDATE users SET locked_until='2000-01-01T00:00:00.000Z'")
        .execute(&pool)
        .await
        .unwrap();
    auth.login("admin", "admin123".into(), None, None)
        .await
        .unwrap();
    let count: i64 = sqlx::query_scalar("SELECT failed_attempts FROM users WHERE username='admin'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn expiration_inactivite_et_mise_a_jour_limitee() {
    let (pool, auth, session) = setup().await;
    let token = crate::domain::password::generate_token();
    let hash = crate::domain::password::token_hash(&token);
    let old = (chrono::Utc::now() - chrono::Duration::seconds(120))
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    sqlx::query("INSERT INTO sessions(token_hash,user_id,created_at,last_seen_at) VALUES(?,?,?,?)")
        .bind(&hash)
        .bind(&session.user.id)
        .bind(&old)
        .bind(&old)
        .execute(&pool)
        .await
        .unwrap();
    auth.session(&token).await.unwrap();
    let first: String = sqlx::query_scalar("SELECT last_seen_at FROM sessions WHERE token_hash=?")
        .bind(&hash)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_ne!(first, old);
    auth.session(&token).await.unwrap();
    let second: String = sqlx::query_scalar("SELECT last_seen_at FROM sessions WHERE token_hash=?")
        .bind(&hash)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(first, second);
    sqlx::query("UPDATE sessions SET last_seen_at='2000-01-01T00:00:00.000Z' WHERE token_hash=?")
        .bind(&hash)
        .execute(&pool)
        .await
        .unwrap();
    assert!(matches!(
        auth.session(&token).await,
        Err(MaintError::Unauthorized(_))
    ));
}

#[tokio::test]
async fn referentiel_equipements_cycle_et_profondeur() {
    use crate::referentiel::{self, CreateEquipment, UpdateEquipment};

    let (pool, _auth, admin) = setup().await;
    let site_id = store::save_site(&pool, &admin.actor, None, Some("Site Test Ref"), false)
        .await
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    // 1. Création de 8 niveaux d'équipements successifs (E1 -> E2 -> ... -> E8)
    let mut prev_id: Option<String> = None;
    let mut eq_ids = Vec::new();
    for i in 1..=8 {
        let code = format!("EQ-0{i}");
        let eq = referentiel::create_equipment(
            &pool,
            &admin.actor,
            CreateEquipment {
                site_id: Some(site_id.clone()),
                code: code.clone(),
                name: format!("Équipement {i}"),
                status: Some("operational".into()),
                production_line_id: None,
                parent_id: prev_id.clone(),
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
        prev_id = Some(eq.id.clone());
        eq_ids.push(eq.id);
    }

    // 2. Tenter d'ajouter un 9ème niveau (doit être refusé avec BadRequest 400)
    let err = referentiel::create_equipment(
        &pool,
        &admin.actor,
        CreateEquipment {
            site_id: Some(site_id.clone()),
            code: "EQ-09".into(),
            name: "Équipement 9 (trop profond)".into(),
            status: Some("operational".into()),
            production_line_id: None,
            parent_id: prev_id.clone(),
            manufacturer: None,
            model: None,
            serial_number: None,
            commissioned_on: None,
            description: None,
        },
        None,
    )
    .await
    .unwrap_err();
    assert!(matches!(err, MaintError::BadRequest(_)), "{err:?}");

    // 3. Détection de cycle : déplacer E1 sous E3 (doit être refusé avec BadRequest 400)
    let err_cycle = referentiel::update_equipment(
        &pool,
        &admin.actor,
        &eq_ids[0],
        UpdateEquipment {
            parent_id: crate::referentiel::PatchField::Value(eq_ids[2].clone()),
            ..Default::default()
        },
        None,
    )
    .await
    .unwrap_err();
    assert!(
        matches!(err_cycle, MaintError::BadRequest(_)),
        "{err_cycle:?}"
    );

    // 4. Refus de suppression avec enfants (E1 a E2 comme enfant -> 409 Conflict)
    let err_del = referentiel::delete_equipment(&pool, &admin.actor, &eq_ids[0], None)
        .await
        .unwrap_err();
    assert!(matches!(err_del, MaintError::Conflict(_)), "{err_del:?}");

    // 5. Suppression de la feuille E8 (pas d'enfants -> OK)
    assert!(
        referentiel::delete_equipment(&pool, &admin.actor, &eq_ids[7], None)
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn referentiel_emplacements_hierarchie_et_conflits() {
    use crate::referentiel::{self, CreateLocation};

    let (pool, _auth, admin) = setup().await;
    let site_id = store::save_site(&pool, &admin.actor, None, Some("Site Emplacements"), false)
        .await
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Magasin avec parent interdit (400)
    let err_mag_parent = referentiel::create_location(
        &pool,
        &admin.actor,
        CreateLocation {
            site_id: Some(site_id.clone()),
            code: "MAG-BAD".into(),
            name: "Magasin invalide".into(),
            kind: "magasin".into(),
            parent_id: Some("faux-id".into()),
        },
        None,
    )
    .await
    .unwrap_err();
    assert!(matches!(err_mag_parent, MaintError::BadRequest(_)));

    // Création d'un magasin valide
    let mag = referentiel::create_location(
        &pool,
        &admin.actor,
        CreateLocation {
            site_id: Some(site_id.clone()),
            code: "MAG-01".into(),
            name: "Magasin Principal".into(),
            kind: "magasin".into(),
            parent_id: None,
        },
        None,
    )
    .await
    .unwrap();

    // Allée sans parent interdit (400)
    let err_allee_noparent = referentiel::create_location(
        &pool,
        &admin.actor,
        CreateLocation {
            site_id: Some(site_id.clone()),
            code: "ALL-BAD".into(),
            name: "Allée sans parent".into(),
            kind: "allee".into(),
            parent_id: None,
        },
        None,
    )
    .await
    .unwrap_err();
    assert!(matches!(err_allee_noparent, MaintError::BadRequest(_)));

    // Allée valide rattachée au magasin
    let allee = referentiel::create_location(
        &pool,
        &admin.actor,
        CreateLocation {
            site_id: Some(site_id.clone()),
            code: "ALL-01".into(),
            name: "Allée 1".into(),
            kind: "allee".into(),
            parent_id: Some(mag.id.clone()),
        },
        None,
    )
    .await
    .unwrap();

    // Rack valide rattaché à l'allée
    let rack = referentiel::create_location(
        &pool,
        &admin.actor,
        CreateLocation {
            site_id: Some(site_id.clone()),
            code: "RCK-01".into(),
            name: "Rack A".into(),
            kind: "rack".into(),
            parent_id: Some(allee.id.clone()),
        },
        None,
    )
    .await
    .unwrap();

    // Bac valide rattaché au rack
    let bac = referentiel::create_location(
        &pool,
        &admin.actor,
        CreateLocation {
            site_id: Some(site_id.clone()),
            code: "BAC-01".into(),
            name: "Bac 01".into(),
            kind: "bac".into(),
            parent_id: Some(rack.id.clone()),
        },
        None,
    )
    .await
    .unwrap();

    // Bac rattaché directement à un magasin interdit (400)
    let err_bac_mag = referentiel::create_location(
        &pool,
        &admin.actor,
        CreateLocation {
            site_id: Some(site_id.clone()),
            code: "BAC-MAG".into(),
            name: "Bac sous magasin".into(),
            kind: "bac".into(),
            parent_id: Some(mag.id.clone()),
        },
        None,
    )
    .await
    .unwrap_err();
    assert!(matches!(err_bac_mag, MaintError::BadRequest(_)));

    // Doublon de code sur le même site interdit (409)
    let err_doublon = referentiel::create_location(
        &pool,
        &admin.actor,
        CreateLocation {
            site_id: Some(site_id.clone()),
            code: "BAC-01".into(),
            name: "Bac doublon".into(),
            kind: "bac".into(),
            parent_id: Some(rack.id.clone()),
        },
        None,
    )
    .await
    .unwrap_err();
    assert!(matches!(err_doublon, MaintError::Conflict(_)));

    // Refus de suppression du rack car il contient le bac (409)
    let err_del_rack = referentiel::delete_location(&pool, &admin.actor, &rack.id, None)
        .await
        .unwrap_err();
    assert!(matches!(err_del_rack, MaintError::Conflict(_)));

    // Suppression du bac puis du rack (OK)
    referentiel::delete_location(&pool, &admin.actor, &bac.id, None)
        .await
        .unwrap();
    referentiel::delete_location(&pool, &admin.actor, &rack.id, None)
        .await
        .unwrap();
}

#[tokio::test]
async fn referentiel_pagination_et_audit() {
    use crate::referentiel::{self, CreateSupplier, SupplierFilter};

    let (pool, _auth, admin) = setup().await;
    let site_id = store::save_site(&pool, &admin.actor, None, Some("Site Fournisseurs"), false)
        .await
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Création de 5 fournisseurs A, B, C, D, E
    for name in ["Alpha", "Bravo", "Charlie", "Delta", "Echo"] {
        referentiel::create_supplier(
            &pool,
            &admin.actor,
            CreateSupplier {
                site_id: Some(site_id.clone()),
                name: name.into(),
                contact_name: Some(format!("Contact {name}")),
                email: Some(format!("{name}@example.com")),
                phone: None,
                address: None,
                website: None,
                notes: None,
            },
            None,
        )
        .await
        .unwrap();
    }

    // Pagination par pages de 2 éléments
    let page1 = referentiel::list_suppliers(
        &pool,
        &admin.actor,
        SupplierFilter {
            site_id: Some(site_id.clone()),
            limit: 2,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(page1.items.len(), 2);
    assert_eq!(page1.items[0].name, "Alpha");
    assert_eq!(page1.items[1].name, "Bravo");
    assert!(page1.next_cursor.is_some());

    let c1 = Cursor::decode(page1.next_cursor.as_ref().unwrap()).unwrap();
    let page2 = referentiel::list_suppliers(
        &pool,
        &admin.actor,
        SupplierFilter {
            site_id: Some(site_id.clone()),
            cursor: Some(c1),
            limit: 2,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(page2.items.len(), 2);
    assert_eq!(page2.items[0].name, "Charlie");
    assert_eq!(page2.items[1].name, "Delta");
    assert!(page2.next_cursor.is_some());

    let c2 = Cursor::decode(page2.next_cursor.as_ref().unwrap()).unwrap();
    let page3 = referentiel::list_suppliers(
        &pool,
        &admin.actor,
        SupplierFilter {
            site_id: Some(site_id.clone()),
            cursor: Some(c2),
            limit: 2,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(page3.items.len(), 1);
    assert_eq!(page3.items[0].name, "Echo");
    assert!(page3.next_cursor.is_none());

    // Vérification de l'audit log
    let audit_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_log WHERE entity = 'supplier' AND site_id = ?",
    )
    .bind(&site_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(audit_count, 5);
}
