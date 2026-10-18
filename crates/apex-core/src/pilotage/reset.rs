//! Réinitialisation publique : réponse uniforme, jetons hachés et confirmation atomique.
use super::{notifications, settings::raw};
use crate::{domain::password, store, Actor, MaintError, Role};
use sqlx::SqlitePool;
async fn available(conn: &mut sqlx::SqliteConnection) -> Result<(), MaintError> {
    if raw(conn, "smtp.enabled").await? != "true" || raw(conn, "public_base_url").await?.is_empty()
    {
        return Err(MaintError::NotFound(
            "Réinitialisation par e-mail indisponible".into(),
        ));
    }
    Ok(())
}
pub async fn request(pool: &SqlitePool, identifier: &str, ip: &str) -> Result<(), MaintError> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    available(&mut tx).await?;
    let since = (chrono::Utc::now() - chrono::Duration::hours(1))
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM password_reset_requests WHERE requested_ip=? AND created_at>?",
    )
    .bind(ip)
    .bind(&since)
    .fetch_one(&mut *tx)
    .await?;
    if count >= 10 {
        return Ok(());
    }
    sqlx::query("INSERT INTO password_reset_requests(id,requested_ip,created_at) VALUES(?,?,?)")
        .bind(store::id())
        .bind(ip)
        .bind(store::now())
        .execute(&mut *tx)
        .await?;
    let user:Option<(String,Option<String>,String)>=sqlx::query_as("SELECT id,site_id,email FROM users WHERE (username=? COLLATE NOCASE OR email=? COLLATE NOCASE) AND active=1 AND deleted_at IS NULL AND email IS NOT NULL ORDER BY id LIMIT 1").bind(identifier.trim()).bind(identifier.trim()).fetch_optional(&mut *tx).await?;
    if let Some((uid, site, email)) = user {
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM password_reset_tokens WHERE user_id=? AND created_at>?",
        )
        .bind(&uid)
        .bind(&since)
        .fetch_one(&mut *tx)
        .await?;
        if count < 3 {
            let token = password::generate_token();
            let hash = password::token_hash(&token);
            sqlx::query(
                "UPDATE password_reset_tokens SET used_at=? WHERE user_id=? AND used_at IS NULL",
            )
            .bind(store::now())
            .bind(&uid)
            .execute(&mut *tx)
            .await?;
            let expires = (chrono::Utc::now() + chrono::Duration::minutes(30))
                .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
            sqlx::query("INSERT INTO password_reset_tokens(id,user_id,token_hash,expires_at,created_at,requested_ip) VALUES(?,?,?,?,?,?)").bind(store::id()).bind(&uid).bind(hash).bind(expires).bind(store::now()).bind(ip).execute(&mut *tx).await?;
            let base = raw(&mut tx, "public_base_url").await?;
            notifications::queue(
                &mut tx,
                site.as_deref(),
                &email,
                "password_reset",
                None,
                "Réinitialisation de votre mot de passe",
                "Ce lien est valable 30 minutes et ne peut être utilisé qu'une fois.",
                &format!(
                    "{}/#/reinitialiser?jeton={token}",
                    base.trim_end_matches('/')
                ),
            )
            .await?;
        }
    }
    tx.commit().await?;
    Ok(())
}
pub async fn confirm(
    pool: &SqlitePool,
    token: &str,
    new_password: String,
) -> Result<(), MaintError> {
    available(&mut *pool.acquire().await?).await?;
    let invalid = || MaintError::BadRequest("Lien de réinitialisation invalide ou expiré".into());
    let hash = password::token_hash(token);
    let snapshot:Option<(String,String)>=sqlx::query_as("SELECT u.id,u.password_hash FROM password_reset_tokens t JOIN users u ON u.id=t.user_id WHERE t.token_hash=? AND t.used_at IS NULL AND t.expires_at>? AND u.active=1 AND u.deleted_at IS NULL").bind(&hash).bind(store::now()).fetch_optional(pool).await?;
    let (uid, old) = snapshot.ok_or_else(invalid)?;
    password::validate_new_password(&new_password, None)?;
    if password::verify(new_password.clone(), Some(old.clone())).await? {
        return Err(MaintError::BadRequest(
            "Le nouveau mot de passe doit différer de l'ancien".into(),
        ));
    }
    let encoded = password::hash(new_password).await?;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    available(&mut tx).await?;
    let site:Option<Option<String>>=sqlx::query_scalar("SELECT u.site_id FROM password_reset_tokens t JOIN users u ON u.id=t.user_id WHERE t.token_hash=? AND t.used_at IS NULL AND t.expires_at>? AND u.id=? AND u.password_hash=? AND u.active=1 AND u.deleted_at IS NULL").bind(&hash).bind(store::now()).bind(&uid).bind(old).fetch_optional(&mut *tx).await?;
    let site = site.ok_or_else(invalid)?;
    sqlx::query("UPDATE password_reset_tokens SET used_at=? WHERE user_id=? AND used_at IS NULL")
        .bind(store::now())
        .bind(&uid)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE users SET password_hash=?,failed_attempts=0,locked_until=NULL,must_change_password=0,updated_at=? WHERE id=? AND active=1 AND deleted_at IS NULL").bind(encoded).bind(store::now()).bind(&uid).execute(&mut *tx).await?;
    let actor = Actor::new(&uid, site.clone(), Role::Admin, []);
    store::revoke(&mut tx, &actor, &uid, None).await?;
    store::audit(
        &mut tx,
        &actor,
        store::Audit {
            action: "password_reset_email",
            entity: "user",
            entity_id: Some(&uid),
            site_id: site.as_deref(),
            before: None,
            after: None,
            ip: None,
        },
    )
    .await?;
    tx.commit().await?;
    Ok(())
}
