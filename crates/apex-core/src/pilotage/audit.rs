//! Consultation du journal et purge de conservation, seule suppression physique métier.
use crate::{store, Actor, Cursor, MaintError, Permission, Role};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{FromRow, QueryBuilder, Sqlite, SqlitePool};
#[derive(Debug, Serialize, FromRow)]
pub struct Entry {
    pub id: String,
    pub site_id: Option<String>,
    pub user_id: Option<String>,
    pub action: String,
    pub entity: String,
    pub entity_id: Option<String>,
    pub before_json: Option<String>,
    pub after_json: Option<String>,
    pub ip: Option<String>,
    pub created_at: String,
}
fn representation(e: Entry) -> Value {
    let before: Value = e
        .before_json
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(Value::Null);
    let after: Value = e
        .after_json
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(Value::Null);
    let mut keys = std::collections::BTreeSet::new();
    for v in [&before, &after] {
        if let Some(o) = v.as_object() {
            keys.extend(o.keys().cloned());
        }
    }
    let diff: Vec<_> = keys
        .into_iter()
        .filter(|k| before[k] != after[k])
        .map(|k| json!({"field":k,"before":before[&k],"after":after[&k]}))
        .collect();
    json!({"id":e.id,"site_id":e.site_id,"user_id":e.user_id,"action":e.action,"entity":e.entity,"entity_id":e.entity_id,"before":before,"after":after,"diff":diff,"ip":e.ip,"created_at":e.created_at})
}
#[derive(Default, Deserialize, Clone, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Filter {
    pub from: Option<String>,
    pub to: Option<String>,
    pub site_id: Option<String>,
    pub user_id: Option<String>,
    pub entity: Option<String>,
    pub action: Option<String>,
    pub entity_id: Option<String>,
    pub q: Option<String>,
    pub cursor: Option<String>,
    pub limit: Option<u32>,
}
pub fn like_pattern(q: &str) -> String {
    format!(
        "%{}%",
        q.replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    )
}
pub async fn list(pool: &SqlitePool, actor: &Actor, f: &Filter) -> Result<Value, MaintError> {
    actor.require_permission(Permission::AuditView)?;
    if !actor.is_admin()
        && f.site_id
            .as_ref()
            .is_some_and(|site| Some(site) != actor.site_id.as_ref())
    {
        return Err(MaintError::NotFound("Site".into()));
    }
    let limit = f.limit.unwrap_or(50).clamp(1, 1000);
    let mut q: QueryBuilder<Sqlite> = QueryBuilder::new("SELECT * FROM audit_log WHERE 1=1");
    if !actor.is_admin() {
        q.push(" AND site_id=").push_bind(&actor.site_id);
    }
    for (column, v) in [
        ("site_id", &f.site_id),
        ("user_id", &f.user_id),
        ("entity", &f.entity),
        ("action", &f.action),
        ("entity_id", &f.entity_id),
    ] {
        if let Some(v) = v {
            q.push(format!(" AND {column}=")).push_bind(v);
        }
    }
    if let Some(v) = &f.from {
        q.push(" AND created_at>=").push_bind(v);
    }
    if let Some(v) = &f.to {
        q.push(" AND created_at<=").push_bind(v);
    }
    if let Some(v) = &f.q {
        q.push(" AND (COALESCE(before_json,'')||COALESCE(after_json,'')) LIKE ")
            .push_bind(like_pattern(v))
            .push(" ESCAPE '\\'");
    }
    if let Some(c) = &f.cursor {
        let c = Cursor::decode(c)?;
        q.push(" AND (created_at,id)<(")
            .push_bind(
                c.timestamp
                    .ok_or_else(|| MaintError::BadRequest("Curseur temporel attendu".into()))?,
            )
            .push(",")
            .push_bind(c.id)
            .push(")");
    }
    q.push(" ORDER BY created_at DESC,id DESC LIMIT ")
        .push_bind(i64::from(limit) + 1);
    let mut rows = q.build_query_as::<Entry>().fetch_all(pool).await?;
    let more = rows.len() > limit as usize;
    rows.truncate(limit as usize);
    let cursor = if more {
        rows.last()
            .map(|e| Cursor::with_timestamp(&e.id, &e.created_at).encode())
    } else {
        None
    };
    Ok(
        json!({"items":rows.into_iter().map(representation).collect::<Vec<_>>(),"next_cursor":cursor}),
    )
}
pub async fn get(pool: &SqlitePool, actor: &Actor, id: &str) -> Result<Value, MaintError> {
    actor.require_permission(Permission::AuditView)?;
    let e = sqlx::query_as("SELECT * FROM audit_log WHERE id=? AND (? OR site_id=?)")
        .bind(id)
        .bind(actor.is_admin())
        .bind(&actor.site_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| MaintError::NotFound("Ligne d'audit".into()))?;
    Ok(representation(e))
}
pub async fn purge(pool: &SqlitePool) -> Result<(), MaintError> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let days = super::settings::raw(&mut tx, "audit.retention_days")
        .await?
        .parse::<i64>()
        .unwrap_or(1095)
        .max(365);
    let cutoff = (chrono::Utc::now() - chrono::Duration::days(days))
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let count = sqlx::query("DELETE FROM audit_log WHERE created_at<?")
        .bind(&cutoff)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    store::audit(
        &mut tx,
        &Actor::new("", None, Role::Admin, []),
        store::Audit {
            action: "purge",
            entity: "audit",
            entity_id: None,
            site_id: None,
            before: None,
            after: Some(json!({"deleted_count":count,"cutoff":cutoff})),
            ip: None,
        },
    )
    .await?;
    tx.commit().await?;
    Ok(())
}
