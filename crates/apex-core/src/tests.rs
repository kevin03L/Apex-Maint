//! Vérifications métier sur des bases exclusivement en mémoire.
use crate::{
    db,
    services::Auth,
    store::{self, CreateUser},
    Actor, MaintError, Permission, Role,
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
