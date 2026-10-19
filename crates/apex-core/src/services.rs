//! Services d'authentification et initialisation contrôlée du socle.
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use serde_json::json;
use sqlx::SqlitePool;

use crate::{
    domain::password,
    store::{self, Audit, User, USER_COLUMNS},
    Actor, MaintError, Permission, Role,
};

pub const LOGIN_MAX_FAILURES: i64 = 5;
pub const LOGIN_LOCK_SECONDS: i64 = 60;

#[derive(Clone)]
pub struct Auth {
    pool: SqlitePool,
    ttl: Duration,
    lock_seconds: i64,
}

#[derive(Clone)]
pub struct Session {
    pub actor: Actor,
    pub user: User,
    pub token_hash: String,
}

fn system() -> Actor {
    Actor::new("", None, Role::Admin, Permission::ALL.iter().copied())
}
fn denied() -> MaintError {
    MaintError::Unauthorized("Identifiant ou mot de passe incorrect".into())
}
fn date(value: &str) -> Result<DateTime<Utc>, MaintError> {
    DateTime::parse_from_rfc3339(value)
        .map(|d| d.with_timezone(&Utc))
        .map_err(|_| MaintError::Internal("Date de session invalide".into()))
}
fn lock_error(user: &User) -> Option<MaintError> {
    user.locked_until
        .as_deref()
        .and_then(|s| date(s).ok())
        .filter(|d| *d > Utc::now())
        .map(|d| {
            MaintError::TooManyRequests(format!(
                "Compte verrouillé, réessayez dans {} s",
                (d - Utc::now()).num_seconds().max(0) + 1
            ))
        })
}

impl Auth {
    pub fn new(pool: SqlitePool, ttl_hours: u32) -> Self {
        // Surcharge réservée aux binaires de test/debug ; ignorée en production optimisée.
        let lock_seconds = if cfg!(debug_assertions) {
            std::env::var("APEX_MAINT_TEST_LOCK_SECONDS")
                .ok()
                .and_then(|s| s.parse::<i64>().ok())
                .filter(|n| *n > 0 && *n <= 60)
                .unwrap_or(LOGIN_LOCK_SECONDS)
        } else {
            LOGIN_LOCK_SECONDS
        };
        Self {
            pool,
            ttl: Duration::hours(ttl_hours.into()),
            lock_seconds,
        }
    }

    pub async fn initialize(&self, demo: bool) -> Result<(), MaintError> {
        tokio::task::spawn_blocking(password::prepare_dummy)
            .await
            .map_err(|_| MaintError::Internal("Préparation Argon2 interrompue".into()))?;
        let factory = password::hash("admin123".into()).await?;
        let demo_hash = if demo {
            Some(password::hash("demo1234".into()).await?)
        } else {
            None
        };
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM sites WHERE deleted_at IS NULL")
            .fetch_one(&mut *tx)
            .await?;
        if count == 0 {
            insert_site(&mut tx, "Site principal").await?;
        }
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM users WHERE role='admin' AND active=1 AND deleted_at IS NULL",
        )
        .fetch_one(&mut *tx)
        .await?;
        if count == 0 {
            insert_user(
                &mut tx,
                "admin",
                "Administrateur",
                Role::Admin,
                None,
                &factory,
                true,
            )
            .await?;
            tracing::warn!("Compte administrateur d'usine créé : changez le mot de passe admin123");
        }
        let seeded: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM app_settings WHERE key='phase1_demo_seeded')",
        )
        .fetch_one(&mut *tx)
        .await?;
        if let Some(hash) = demo_hash.filter(|_| !seeded) {
            let sid = insert_site(&mut tx, "Site de démonstration").await?;
            for role in [Role::Responsable, Role::Technicien, Role::Production] {
                insert_user(
                    &mut tx,
                    role.as_str(),
                    role.as_str(),
                    role,
                    Some(&sid),
                    &hash,
                    false,
                )
                .await?;
            }
            sqlx::query("INSERT INTO app_settings(key,value,updated_at) VALUES('phase1_demo_seeded','true',?)").bind(store::now()).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        let hashes:Vec<String>=sqlx::query_scalar("SELECT password_hash FROM users WHERE role='admin' AND active=1 AND deleted_at IS NULL").fetch_all(&self.pool).await?;
        for hash in hashes {
            if password::verify("admin123".into(), Some(hash)).await? {
                tracing::warn!("Un administrateur utilise encore le mot de passe d'usine admin123 : changement obligatoire");
            }
        }
        Ok(())
    }

    pub async fn login(
        &self,
        username: &str,
        plain: String,
        ip: Option<&str>,
        agent: Option<&str>,
    ) -> Result<(User, String), MaintError> {
        // L'authentification précède la construction d'Actor : aucune donnée métier n'est publiée ici.
        let snapshot:Option<User>=sqlx::query_as(&format!("SELECT {USER_COLUMNS} FROM users WHERE username=? COLLATE NOCASE AND deleted_at IS NULL"))
            .bind(username.trim()).fetch_optional(&self.pool).await?;
        let valid = password::verify(
            plain,
            snapshot
                .as_ref()
                .filter(|u| u.active)
                .map(|u| u.password_hash.clone()),
        )
        .await?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let current:Option<User>=sqlx::query_as(&format!("SELECT {USER_COLUMNS} FROM users WHERE username=? COLLATE NOCASE AND deleted_at IS NULL"))
            .bind(username.trim()).fetch_optional(&mut *tx).await?;
        let Some(mut user) = current.filter(|u| u.active) else {
            store::audit(
                &mut tx,
                &system(),
                Audit {
                    action: "login_failed",
                    entity: "session",
                    entity_id: None,
                    site_id: None,
                    before: None,
                    after: None,
                    ip,
                },
            )
            .await?;
            tx.commit().await?;
            return Err(denied());
        };
        let actor = Actor::new(
            &user.id,
            user.site_id.clone(),
            Role::parse(&user.role).map_err(|_| denied())?,
            [],
        );
        if let Some(error) = lock_error(&user) {
            return Err(error);
        }
        let valid = valid
            && snapshot
                .as_ref()
                .is_some_and(|s| s.id == user.id && s.password_hash == user.password_hash);
        if !valid {
            let until = (Utc::now() + Duration::seconds(self.lock_seconds))
                .to_rfc3339_opts(SecondsFormat::Millis, true);
            // Un seul UPDATE calcule le compteur et le verrou ; un verrou expiré démarre une nouvelle série.
            let (failures,): (i64,)=sqlx::query_as("UPDATE users SET failed_attempts=CASE WHEN locked_until IS NOT NULL AND locked_until<=? THEN 1 ELSE failed_attempts+1 END, locked_until=CASE WHEN (CASE WHEN locked_until IS NOT NULL AND locked_until<=? THEN 1 ELSE failed_attempts+1 END)>=? THEN ? ELSE NULL END WHERE id=? AND active=1 AND deleted_at IS NULL RETURNING failed_attempts")
                .bind(store::now()).bind(store::now()).bind(LOGIN_MAX_FAILURES).bind(until).bind(&user.id).fetch_one(&mut *tx).await?;
            store::audit(
                &mut tx,
                &actor,
                Audit {
                    action: "login_failed",
                    entity: "session",
                    entity_id: Some(&user.id),
                    site_id: user.site_id.as_deref(),
                    before: None,
                    after: None,
                    ip,
                },
            )
            .await?;
            if failures >= LOGIN_MAX_FAILURES {
                store::audit(
                    &mut tx,
                    &actor,
                    Audit {
                        action: "lock",
                        entity: "user",
                        entity_id: Some(&user.id),
                        site_id: user.site_id.as_deref(),
                        before: None,
                        after: None,
                        ip,
                    },
                )
                .await?;
            }
            tx.commit().await?;
            return Err(denied());
        }
        let token = password::generate_token();
        let now = store::now();
        sqlx::query(
            "UPDATE users SET failed_attempts=0,locked_until=NULL,last_login_at=? WHERE id=?",
        )
        .bind(&now)
        .bind(&user.id)
        .execute(&mut *tx)
        .await?;
        sqlx::query("INSERT INTO sessions(token_hash,user_id,created_at,last_seen_at,ip,user_agent) VALUES(?,?,?,?,?,?)")
            .bind(password::token_hash(&token)).bind(&user.id).bind(&now).bind(&now).bind(ip).bind(agent).execute(&mut *tx).await?;
        store::audit(
            &mut tx,
            &actor,
            Audit {
                action: "login",
                entity: "session",
                entity_id: Some(&user.id),
                site_id: user.site_id.as_deref(),
                before: None,
                after: None,
                ip,
            },
        )
        .await?;
        tx.commit().await?;
        user.last_login_at = Some(now);
        user.locked = false;
        user.locked_until = None;
        Ok((user, token))
    }

    pub async fn session(&self, token: &str) -> Result<Session, MaintError> {
        if token.len() != 64 || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(denied());
        }
        let hash = password::token_hash(token);
        let mut tx = self.pool.begin().await?;
        let row: Option<(String, String)> = sqlx::query_as(
            "SELECT user_id,last_seen_at FROM sessions WHERE token_hash=? AND revoked_at IS NULL",
        )
        .bind(&hash)
        .fetch_optional(&mut *tx)
        .await?;
        let (uid, last) = row.ok_or_else(denied)?;
        let timeout = crate::pilotage::settings::raw(&mut tx, "session_timeout_min")
            .await?
            .parse::<i64>()
            .ok()
            .filter(|n| (15..=10080).contains(n))
            .map(Duration::minutes)
            .unwrap_or(self.ttl);
        if Utc::now() - date(&last)? >= timeout {
            return Err(denied());
        }
        let user: User = sqlx::query_as(&format!(
            "SELECT {USER_COLUMNS} FROM users WHERE id=? AND deleted_at IS NULL AND active=1"
        ))
        .bind(&uid)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(denied)?;
        let role = Role::parse(&user.role).map_err(|_| denied())?;
        let permissions = if role == Role::Admin {
            Permission::ALL.to_vec()
        } else {
            let keys: Vec<String> =
                sqlx::query_scalar("SELECT permission FROM role_permissions WHERE role=?")
                    .bind(role.as_str())
                    .fetch_all(&mut *tx)
                    .await?;
            keys.iter()
                .map(|s| {
                    Permission::parse(s)
                        .map_err(|_| MaintError::Internal("Permission stockée inconnue".into()))
                })
                .collect::<Result<Vec<_>, _>>()?
        };
        tx.commit().await?;
        if (Utc::now() - date(&last)?).num_seconds() >= 60 {
            sqlx::query(
                "UPDATE sessions SET last_seen_at=? WHERE token_hash=? AND revoked_at IS NULL AND last_seen_at=? AND EXISTS(SELECT 1 FROM users WHERE id=sessions.user_id AND active=1 AND deleted_at IS NULL)",
            )
            .bind(store::now())
            .bind(&hash)
            .bind(&last)
            .execute(&self.pool)
            .await?;
        }
        Ok(Session {
            actor: Actor::new(&uid, user.site_id.clone(), role, permissions),
            user,
            token_hash: hash,
        })
    }

    pub async fn logout(&self, session: &Session) -> Result<(), MaintError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        sqlx::query("UPDATE sessions SET revoked_at=? WHERE token_hash=? AND user_id=? AND EXISTS(SELECT 1 FROM users WHERE id=sessions.user_id AND (? OR site_id=?))")
            .bind(store::now()).bind(&session.token_hash).bind(&session.actor.user_id).bind(session.actor.is_admin()).bind(&session.actor.site_id).execute(&mut *tx).await?;
        store::audit(
            &mut tx,
            &session.actor,
            Audit {
                action: "logout",
                entity: "session",
                entity_id: Some(&session.actor.user_id),
                site_id: session.actor.site_id.as_deref(),
                before: None,
                after: None,
                ip: None,
            },
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn change_password(
        &self,
        session: &Session,
        current: String,
        new: String,
    ) -> Result<(), MaintError> {
        password::validate_new_password(&new, Some(&current))?;
        if !password::verify(current, Some(session.user.password_hash.clone())).await? {
            return Err(denied());
        }
        let hash = password::hash(new).await?;
        let actor = &session.actor;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let fresh = store::scoped_user(&mut tx, actor, &actor.user_id).await?;
        let live: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM sessions WHERE token_hash=? AND revoked_at IS NULL)",
        )
        .bind(&session.token_hash)
        .fetch_one(&mut *tx)
        .await?;
        if !live || fresh.password_hash != session.user.password_hash || !fresh.active {
            return Err(denied());
        }
        sqlx::query("UPDATE users SET password_hash=?,must_change_password=0,updated_at=? WHERE id=? AND (? OR site_id=?)")
            .bind(hash).bind(store::now()).bind(&actor.user_id).bind(actor.is_admin()).bind(&actor.site_id).execute(&mut *tx).await?;
        store::revoke(&mut tx, actor, &actor.user_id, Some(&session.token_hash)).await?;
        let after = store::scoped_user(&mut tx, actor, &actor.user_id).await?;
        store::audit(
            &mut tx,
            actor,
            Audit {
                action: "password_change",
                entity: "user",
                entity_id: Some(&actor.user_id),
                site_id: actor.site_id.as_deref(),
                before: Some(json!(fresh)),
                after: Some(json!(after)),
                ip: None,
            },
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }
}

async fn insert_site(conn: &mut sqlx::SqliteConnection, name: &str) -> Result<String, MaintError> {
    let sid = store::id();
    sqlx::query("INSERT INTO sites(id,name,created_at,updated_at) VALUES(?,?,?,?)")
        .bind(&sid)
        .bind(name)
        .bind(store::now())
        .bind(store::now())
        .execute(&mut *conn)
        .await?;
    store::audit(
        conn,
        &system(),
        Audit {
            action: "create",
            entity: "site",
            entity_id: Some(&sid),
            site_id: Some(&sid),
            before: None,
            after: Some(json!({"id":sid,"name":name})),
            ip: None,
        },
    )
    .await?;
    Ok(sid)
}

async fn insert_user(
    conn: &mut sqlx::SqliteConnection,
    username: &str,
    name: &str,
    role: Role,
    site: Option<&str>,
    hash: &str,
    must_change: bool,
) -> Result<(), MaintError> {
    let uid = store::id();
    sqlx::query("INSERT INTO users(id,site_id,username,display_name,role,password_hash,must_change_password,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?)")
        .bind(&uid).bind(site).bind(username).bind(name).bind(role.as_str()).bind(hash).bind(must_change).bind(store::now()).bind(store::now()).execute(&mut *conn).await?;
    store::audit(
        conn,
        &system(),
        Audit {
            action: "create",
            entity: "user",
            entity_id: Some(&uid),
            site_id: site,
            before: None,
            after: Some(json!({"id":uid,"username":username,"role":role,"site_id":site})),
            ip: None,
        },
    )
    .await?;
    Ok(())
}
