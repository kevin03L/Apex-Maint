//! Liaison entrante Supervision : clés limitées à un site et ingestion partielle de lots.
use crate::{
    store::{self, Audit},
    Actor, MaintError, Permission, Role,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use chrono::{DateTime, Duration, Utc};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{FromRow, SqliteConnection, SqlitePool};

mod ingest;
mod read;
pub use ingest::{ingest_counters, ingest_statuses, BatchReport, Reading, StatusInput};
pub use read::*;

#[derive(Clone, Debug, Serialize, FromRow)]
pub struct IntegrationKey {
    pub id: String,
    pub site_id: String,
    pub label: String,
    pub key_prefix: String,
    #[sqlx(json)]
    pub scopes: Vec<String>,
    pub created_by_id: String,
    pub created_at: String,
    pub last_used_at: Option<String>,
    pub last_used_ip: Option<String>,
    pub revoked_at: Option<String>,
    pub expires_at: Option<String>,
}
const KEY_COLUMNS:&str="id,site_id,label,key_prefix,scopes,created_by_id,created_at,last_used_at,last_used_ip,revoked_at,expires_at";
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyInput {
    pub site_id: Option<String>,
    pub label: String,
    pub scopes: Vec<String>,
    pub expires_at: Option<String>,
}

pub async fn settings(pool: &SqlitePool, actor: &Actor) -> Result<Value, MaintError> {
    actor.require_permission(Permission::SettingsView)?;
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT key,value FROM app_settings WHERE key LIKE 'integration.supervision.%'",
    )
    .fetch_all(pool)
    .await?;
    let mut result = json!({});
    for (key, value) in rows {
        result[key] = serde_json::from_str(&value).map_err(crate::exploitation::io)?;
    }
    Ok(result)
}
pub async fn save_settings(
    pool: &SqlitePool,
    actor: &Actor,
    patch: Value,
) -> Result<Value, MaintError> {
    actor.require_permission(Permission::SettingsEdit)?;
    let fields = patch
        .as_object()
        .ok_or_else(|| MaintError::BadRequest("Objet JSON attendu".into()))?;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    for (key, value) in fields {
        let valid = match key.as_str() {
            "integration.supervision.enabled"
            | "integration.supervision.counters_enabled"
            | "integration.supervision.status_enabled" => value.is_boolean(),
            "integration.supervision.counter_min_interval_min"
            | "integration.supervision.status_stale_after_min" => {
                value.as_u64().is_some_and(|n| (1..=1440).contains(&n))
            }
            "integration.supervision.status_history_days" => {
                value.as_u64().is_some_and(|n| (1..=3650).contains(&n))
            }
            _ => false,
        };
        if !valid {
            return Err(MaintError::BadRequest(format!(
                "Paramètre d'intégration invalide : {key}"
            )));
        }
        sqlx::query("UPDATE app_settings SET value=?,updated_at=? WHERE key=?")
            .bind(value.to_string())
            .bind(store::now())
            .bind(key)
            .execute(&mut *tx)
            .await?;
    }
    store::audit(
        &mut tx,
        actor,
        Audit {
            action: "update",
            entity: "integration_settings",
            entity_id: None,
            site_id: actor.site_id.as_deref(),
            before: None,
            after: Some(patch.clone()),
            ip: None,
        },
    )
    .await?;
    tx.commit().await?;
    settings(pool, actor).await
}
pub(super) async fn setting(conn: &mut SqliteConnection, key: &str) -> Result<Value, MaintError> {
    let value: String = sqlx::query_scalar("SELECT value FROM app_settings WHERE key=?")
        .bind(format!("integration.supervision.{key}"))
        .fetch_one(conn)
        .await?;
    serde_json::from_str(&value).map_err(crate::exploitation::io)
}
fn scopes(input: &[String]) -> Result<(), MaintError> {
    if input.is_empty()
        || input.len() > 2
        || input
            .iter()
            .any(|s| !["counters", "status"].contains(&s.as_str()))
        || (input.len() == 2 && input[0] == input[1])
    {
        return Err(MaintError::BadRequest(
            "Portées attendues : counters et/ou status".into(),
        ));
    }
    Ok(())
}
pub async fn create_key(
    pool: &SqlitePool,
    actor: &Actor,
    input: KeyInput,
) -> Result<Value, MaintError> {
    actor.require_permission(Permission::SettingsEdit)?;
    scopes(&input.scopes)?;
    let site = if actor.is_admin() {
        input.site_id.clone().or_else(|| actor.site_id.clone())
    } else {
        if input
            .site_id
            .as_ref()
            .is_some_and(|s| Some(s) != actor.site_id.as_ref())
        {
            return Err(MaintError::NotFound("Site".into()));
        }
        actor.site_id.clone()
    }
    .ok_or_else(|| MaintError::BadRequest("Site requis".into()))?;
    let label = input.label.trim();
    if label.is_empty() || label.chars().count() > 200 {
        return Err(MaintError::BadRequest(
            "Libellé de 1 à 200 caractères requis".into(),
        ));
    }
    let expires = input
        .expires_at
        .map(|s| {
            DateTime::parse_from_rfc3339(&s)
                .map(|d| d.with_timezone(&Utc))
                .map_err(|_| MaintError::BadRequest("Expiration invalide".into()))
        })
        .transpose()?;
    if expires.is_some_and(|d| d <= Utc::now()) {
        return Err(MaintError::BadRequest(
            "L'expiration doit être future".into(),
        ));
    }
    let expires = expires.map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
    let mut raw = [0; 32];
    rand::rngs::OsRng.fill_bytes(&mut raw);
    let key = format!("apx_{}", URL_SAFE_NO_PAD.encode(raw));
    let id = store::id();
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let exists: i64 =
        sqlx::query_scalar("SELECT count(*) FROM sites WHERE id=? AND deleted_at IS NULL")
            .bind(&site)
            .fetch_one(&mut *tx)
            .await?;
    if exists == 0 {
        return Err(MaintError::NotFound("Site".into()));
    }
    sqlx::query("INSERT INTO integration_keys(id,site_id,label,key_prefix,key_hash,scopes,created_by_id,created_at,expires_at) VALUES(?,?,?,?,?,?,?,?,?)")
        .bind(&id).bind(&site).bind(label).bind(&key[..8]).bind(crate::domain::password::token_hash(&key)).bind(json!(input.scopes).to_string()).bind(&actor.user_id).bind(store::now()).bind(expires).execute(&mut *tx).await?;
    store::audit(
        &mut tx,
        actor,
        Audit {
            action: "create",
            entity: "integration_key",
            entity_id: Some(&id),
            site_id: Some(&site),
            before: None,
            after: Some(json!({"label":label,"scopes":input.scopes})),
            ip: None,
        },
    )
    .await?;
    let item: IntegrationKey = sqlx::query_as(&format!(
        "SELECT {KEY_COLUMNS} FROM integration_keys WHERE id=?"
    ))
    .bind(&id)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(json!({"key":key,"item":item}))
}
pub async fn keys(
    pool: &SqlitePool,
    actor: &Actor,
    cursor: Option<&str>,
    limit: u32,
) -> Result<crate::referentiel::Page<IntegrationKey>, MaintError> {
    actor.require_permission(Permission::SettingsView)?;
    let limit = limit.clamp(1, 200) as usize;
    let mut items:Vec<IntegrationKey>=sqlx::query_as(&format!("SELECT {KEY_COLUMNS} FROM integration_keys WHERE (? OR site_id=?) AND (? IS NULL OR id<?) ORDER BY id DESC LIMIT ?"))
        .bind(actor.is_admin()).bind(&actor.site_id).bind(cursor).bind(cursor).bind((limit+1) as i64).fetch_all(pool).await?;
    let next_cursor = if items.len() > limit {
        items.truncate(limit);
        items.last().map(|r| r.id.clone())
    } else {
        None
    };
    Ok(crate::referentiel::Page { items, next_cursor })
}
pub async fn update_key(
    pool: &SqlitePool,
    actor: &Actor,
    id: &str,
    new_scopes: Option<Vec<String>>,
    revoke: bool,
) -> Result<IntegrationKey, MaintError> {
    actor.require_permission(Permission::SettingsEdit)?;
    if let Some(s) = &new_scopes {
        scopes(s)?;
    }
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let before: IntegrationKey = sqlx::query_as(&format!(
        "SELECT {KEY_COLUMNS} FROM integration_keys WHERE id=? AND (? OR site_id=?)"
    ))
    .bind(id)
    .bind(actor.is_admin())
    .bind(&actor.site_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| MaintError::NotFound("Clé d'intégration".into()))?;
    if revoke {
        sqlx::query("UPDATE integration_keys SET revoked_at=COALESCE(revoked_at,?) WHERE id=?")
            .bind(store::now())
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(s) = new_scopes {
        sqlx::query("UPDATE integration_keys SET scopes=? WHERE id=?")
            .bind(json!(s).to_string())
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    let item: IntegrationKey = sqlx::query_as(&format!(
        "SELECT {KEY_COLUMNS} FROM integration_keys WHERE id=?"
    ))
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    store::audit(
        &mut tx,
        actor,
        Audit {
            action: if revoke { "revoke" } else { "update" },
            entity: "integration_key",
            entity_id: Some(id),
            site_id: Some(&before.site_id),
            before: Some(json!(before)),
            after: Some(json!(item)),
            ip: None,
        },
    )
    .await?;
    tx.commit().await?;
    Ok(item)
}

#[derive(Clone)]
pub struct ServicePrincipal {
    pub key_id: String,
    pub actor: Actor,
}
/// L'authentification construit l'Actor ; aucune session ni permission d'administration.
pub async fn authenticate(
    pool: &SqlitePool,
    token: &str,
    scope: &str,
    ip: &str,
) -> Result<ServicePrincipal, MaintError> {
    let unauthorized = || MaintError::Unauthorized("Clé de service requise".into());
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    if setting(&mut tx, "enabled").await? != true {
        return Err(MaintError::NotFound("Liaison Supervision".into()));
    }
    if !token.starts_with("apx_") || token.len() != 47 {
        return Err(unauthorized());
    }
    let hash = crate::domain::password::token_hash(token);
    let row:Option<(String,String,String,String)>=sqlx::query_as("SELECT k.id,k.site_id,k.key_hash,k.scopes FROM integration_keys k JOIN sites s ON s.id=k.site_id WHERE k.key_hash=? AND k.revoked_at IS NULL AND (k.expires_at IS NULL OR k.expires_at>?) AND s.deleted_at IS NULL")
        .bind(&hash).bind(store::now()).fetch_optional(&mut *tx).await?;
    let (id, site, stored, scopes) = row.ok_or_else(unauthorized)?;
    use subtle::ConstantTimeEq;
    if !bool::from(hash.as_bytes().ct_eq(stored.as_bytes())) {
        return Err(unauthorized());
    }
    let scopes: Vec<String> = serde_json::from_str(&scopes).map_err(crate::exploitation::io)?;
    if !scopes.iter().any(|s| s == scope) {
        return Err(unauthorized());
    }
    if setting(&mut tx, &format!("{scope}_enabled")).await? != true {
        return Err(MaintError::NotFound("Type d'événement désactivé".into()));
    }
    sqlx::query("UPDATE integration_keys SET last_used_at=?,last_used_ip=? WHERE id=?")
        .bind(store::now())
        .bind(ip)
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(ServicePrincipal {
        key_id: id,
        actor: Actor::new(
            "",
            Some(site),
            Role::Technicien,
            [
                Permission::CountersRecord,
                Permission::MaintenanceManage,
                Permission::EquipmentsView,
            ],
        ),
    })
}
