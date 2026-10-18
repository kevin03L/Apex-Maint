//! Paramètres validés côté serveur, sans exposition des secrets.
use crate::{documents::storage::FileStore, store, Actor, MaintError, Permission};
use serde_json::{json, Value};
use sqlx::{SqliteConnection, SqlitePool};

pub async fn raw(conn: &mut SqliteConnection, key: &str) -> Result<String, MaintError> {
    Ok(
        sqlx::query_scalar("SELECT value FROM app_settings WHERE key=?")
            .bind(key)
            .fetch_optional(conn)
            .await?
            .unwrap_or_default(),
    )
}
pub async fn put(conn: &mut SqliteConnection, key: &str, value: &str) -> Result<(), MaintError> {
    sqlx::query("INSERT INTO app_settings(key,value,updated_at) VALUES(?,?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value,updated_at=excluded.updated_at")
 .bind(key).bind(value).bind(store::now()).execute(conn).await?;
    Ok(())
}
fn fields(section: &str) -> Result<&'static [(&'static str, &'static str)], MaintError> {
    Ok(match section {
        "general" => &[
            ("app_name", "app_name"),
            ("app_subtitle", "app_subtitle"),
            ("public_base_url", "public_base_url"),
        ],
        "security" => &[("session_timeout_min", "session_timeout_min")],
        "stock" => &[("stock_alert_threshold", "stock_alert_threshold")],
        "smtp" => &[
            ("host", "smtp.host"),
            ("port", "smtp.port"),
            ("security", "smtp.security"),
            ("username", "smtp.username"),
            ("from_address", "smtp.from_address"),
            ("from_name", "smtp.from_name"),
            ("enabled", "smtp.enabled"),
        ],
        "alerts" => &[
            ("overdue_digest_hour", "alerts.overdue_digest_hour"),
            ("defaults", "alerts.defaults"),
        ],
        "audit" => &[("retention_days", "audit.retention_days")],
        _ => return Err(MaintError::NotFound("Paramètres".into())),
    })
}
pub fn email(value: &str) -> Result<(), MaintError> {
    value
        .parse::<lettre::Address>()
        .map_err(|_| MaintError::BadRequest("Adresse e-mail invalide".into()))?;
    Ok(())
}
async fn read(conn: &mut SqliteConnection, section: &str) -> Result<Value, MaintError> {
    let mut obj = serde_json::Map::new();
    for (field, key) in fields(section)? {
        let v = raw(conn, key).await?;
        let value = match *field {
            "enabled" => json!(v == "true"),
            "port"
            | "session_timeout_min"
            | "stock_alert_threshold"
            | "overdue_digest_hour"
            | "retention_days" => json!(v.parse::<i64>().unwrap_or(0)),
            "defaults" => serde_json::from_str(&v).unwrap_or(json!({})),
            _ => json!(v),
        };
        obj.insert((*field).into(), value);
    }
    if section == "smtp" {
        obj.insert(
            "has_password".into(),
            json!(!raw(conn, "smtp.password_enc").await?.is_empty()),
        );
    }
    Ok(Value::Object(obj))
}
pub async fn get(pool: &SqlitePool, actor: &Actor, section: &str) -> Result<Value, MaintError> {
    actor.require_permission(Permission::SettingsView)?;
    read(&mut *pool.acquire().await?, section).await
}
fn validate(field: &str, v: &Value) -> Result<(), MaintError> {
    let invalid = || MaintError::BadRequest(format!("Paramètre invalide : {field}"));
    match field {
        "port"
        | "session_timeout_min"
        | "stock_alert_threshold"
        | "overdue_digest_hour"
        | "retention_days" => {
            let n = v.as_i64().ok_or_else(invalid)?;
            let (min, max) = match field {
                "port" => (1, 65535),
                "session_timeout_min" => (15, 10080),
                "stock_alert_threshold" => (0, 1_000_000_000),
                "overdue_digest_hour" => (0, 23),
                _ => (365, 36500),
            };
            if !(min..=max).contains(&n) {
                return Err(invalid());
            }
        }
        "enabled" => {
            if !v.is_boolean() {
                return Err(invalid());
            }
        }
        "defaults" => {
            let roles = v.as_object().ok_or_else(invalid)?;
            for (role, events) in roles {
                crate::Role::parse(role).map_err(|_| invalid())?;
                let events = events.as_object().ok_or_else(invalid)?;
                if events.iter().any(|(k, v)| {
                    !super::notifications::EVENTS.contains(&k.as_str()) || !v.is_boolean()
                }) {
                    return Err(invalid());
                }
            }
        }
        _ => {
            let s = v.as_str().ok_or_else(invalid)?;
            if s.len() > 2048 || s.chars().any(char::is_control) {
                return Err(invalid());
            }
            match field {
                "app_name" if s.trim().is_empty() || s.chars().count() > 128 => {
                    return Err(invalid())
                }
                "security" if !["starttls", "tls", "none"].contains(&s) => return Err(invalid()),
                "from_address" if !s.is_empty() => email(s)?,
                "public_base_url" if !s.is_empty() => {
                    let url = url::Url::parse(s).map_err(|_| invalid())?;
                    if !matches!(url.scheme(), "http" | "https")
                        || url.host_str().is_none()
                        || !url.username().is_empty()
                        || url.password().is_some()
                        || url.fragment().is_some()
                        || url.query().is_some()
                    {
                        return Err(invalid());
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}
pub async fn save(
    pool: &SqlitePool,
    files: &FileStore,
    actor: &Actor,
    section: &str,
    input: Value,
) -> Result<Value, MaintError> {
    actor.require_permission(Permission::SettingsEdit)?;
    let fs = fields(section)?;
    let map = input
        .as_object()
        .ok_or_else(|| MaintError::BadRequest("Objet JSON attendu".into()))?;
    for (k, v) in map {
        if !fs.iter().any(|f| f.0 == k) && !(section == "smtp" && k == "password") {
            return Err(MaintError::BadRequest("Paramètre inconnu".into()));
        }
        validate(k, v)?;
    }
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let before = read(&mut tx, section).await?;
    for (k, v) in map {
        if k == "password" {
            put(
                &mut tx,
                "smtp.password_enc",
                &files.encrypt(v.as_str().unwrap_or_default())?,
            )
            .await?;
        } else {
            let key = fs.iter().find(|f| f.0 == k).expect("champ validé").1;
            let text = v
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| v.to_string());
            put(&mut tx, key, &text).await?;
        }
    }
    let after = read(&mut tx, section).await?;
    if section == "smtp"
        && after["enabled"] == true
        && (after["host"].as_str().unwrap_or_default().is_empty()
            || after["from_address"]
                .as_str()
                .unwrap_or_default()
                .is_empty())
    {
        return Err(MaintError::BadRequest(
            "Serveur et expéditeur SMTP obligatoires".into(),
        ));
    }
    store::audit(
        &mut tx,
        actor,
        store::Audit {
            action: "update",
            entity: "settings",
            entity_id: Some(section),
            site_id: actor.site_id.as_deref(),
            before: Some(before),
            after: Some(after.clone()),
            ip: None,
        },
    )
    .await?;
    tx.commit().await?;
    Ok(after)
}
pub async fn branding(pool: &SqlitePool) -> Result<Value, MaintError> {
    let mut conn = pool.acquire().await?;
    Ok(
        json!({"app_name":raw(&mut conn,"app_name").await?,"app_subtitle":raw(&mut conn,"app_subtitle").await?,"password_reset_available":raw(&mut conn,"smtp.enabled").await?=="true"&&!raw(&mut conn,"public_base_url").await?.is_empty()}),
    )
}
