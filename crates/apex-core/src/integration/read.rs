//! Lectures destinées aux listes, fiches et au widget du pilotage.
use super::*;
use crate::referentiel::Page;

#[derive(Serialize, FromRow)]
pub struct ExternalStatus {
    pub equipment_id: String,
    pub site_id: String,
    pub equipment_code: String,
    pub equipment_name: String,
    pub external_ref: Option<String>,
    pub source: String,
    pub state: String,
    pub original_state: String,
    pub since: String,
    pub message: Option<String>,
    pub received_at: String,
    pub stale: bool,
}
pub async fn statuses(
    pool: &SqlitePool,
    actor: &Actor,
    equipment: Option<&str>,
    state: Option<&str>,
    cursor: Option<&str>,
    limit: u32,
) -> Result<Page<ExternalStatus>, MaintError> {
    actor.require_permission(Permission::EquipmentsView)?;
    let limit = limit.clamp(1, 200) as usize;
    let sql="WITH visible AS (SELECT e.id AS equipment_id,e.site_id,e.code AS equipment_code,e.name AS equipment_name,e.external_ref,s.source,CASE WHEN (julianday('now')-julianday(s.received_at))*1440>CAST((SELECT value FROM app_settings WHERE key='integration.supervision.status_stale_after_min') AS INTEGER) THEN 'unknown' ELSE s.state END AS state,s.state AS original_state,s.since,s.message,s.received_at,((julianday('now')-julianday(s.received_at))*1440>CAST((SELECT value FROM app_settings WHERE key='integration.supervision.status_stale_after_min') AS INTEGER)) AS stale FROM equipments e JOIN equipment_external_status s ON s.equipment_id=e.id AND s.site_id=e.site_id WHERE e.deleted_at IS NULL AND (? OR e.site_id=?) AND (? IS NULL OR e.id=?)) SELECT * FROM visible WHERE (? IS NULL OR state=? OR (?='attention' AND state IN ('stopped','alarm'))) AND (? IS NULL OR equipment_id>?) ORDER BY equipment_id LIMIT ?";
    let mut items: Vec<ExternalStatus> = sqlx::query_as(sql)
        .bind(actor.is_admin())
        .bind(&actor.site_id)
        .bind(equipment)
        .bind(equipment)
        .bind(state)
        .bind(state)
        .bind(state)
        .bind(cursor)
        .bind(cursor)
        .bind((limit + 1) as i64)
        .fetch_all(pool)
        .await?;
    let next_cursor = if items.len() > limit {
        items.truncate(limit);
        items.last().map(|i| i.equipment_id.clone())
    } else {
        None
    };
    Ok(Page { items, next_cursor })
}
pub async fn supervision_status(
    pool: &SqlitePool,
    actor: &Actor,
    cursor: Option<&str>,
    limit: u32,
) -> Result<Page<ExternalStatus>, MaintError> {
    statuses(pool, actor, None, Some("attention"), cursor, limit).await
}
#[derive(Serialize, FromRow)]
pub struct StatusHistory {
    pub id: String,
    pub equipment_id: String,
    pub site_id: String,
    pub source: String,
    pub state: String,
    pub since: String,
    pub message: Option<String>,
    pub received_at: String,
}
pub async fn history(
    pool: &SqlitePool,
    actor: &Actor,
    equipment: &str,
    cursor: Option<&str>,
    limit: u32,
) -> Result<Page<StatusHistory>, MaintError> {
    actor.require_permission(Permission::EquipmentsView)?;
    let visible: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM equipments WHERE id=? AND deleted_at IS NULL AND (? OR site_id=?)",
    )
    .bind(equipment)
    .bind(actor.is_admin())
    .bind(&actor.site_id)
    .fetch_one(pool)
    .await?;
    if visible == 0 {
        return Err(MaintError::NotFound("Équipement".into()));
    }
    let limit = limit.clamp(1, 200) as usize;
    let mut items:Vec<StatusHistory>=sqlx::query_as("SELECT id,equipment_id,site_id,source,state,since,message,received_at FROM equipment_external_status_history WHERE equipment_id=? AND (? OR site_id=?) AND (? IS NULL OR id<?) ORDER BY id DESC LIMIT ?")
        .bind(equipment).bind(actor.is_admin()).bind(&actor.site_id).bind(cursor).bind(cursor).bind((limit+1) as i64).fetch_all(pool).await?;
    let next_cursor = if items.len() > limit {
        items.truncate(limit);
        items.last().map(|i| i.id.clone())
    } else {
        None
    };
    Ok(Page { items, next_cursor })
}
#[derive(Serialize, FromRow)]
pub struct IntegrationEvent {
    pub id: String,
    pub site_id: String,
    pub key_id: String,
    pub kind: String,
    pub received_at: String,
    pub items_count: i64,
    pub accepted: i64,
    pub rejected: i64,
    #[sqlx(json)]
    pub errors: Vec<Value>,
    pub duration_ms: i64,
}
pub async fn events(
    pool: &SqlitePool,
    actor: &Actor,
    cursor: Option<&str>,
    limit: u32,
) -> Result<Page<IntegrationEvent>, MaintError> {
    actor.require_permission(Permission::SettingsView)?;
    let limit = limit.clamp(1, 200) as usize;
    let mut items:Vec<IntegrationEvent>=sqlx::query_as("SELECT id,site_id,key_id,kind,received_at,items_count,accepted,rejected,errors,duration_ms FROM integration_events WHERE (? OR site_id=?) AND (? IS NULL OR id<?) ORDER BY id DESC LIMIT ?")
        .bind(actor.is_admin()).bind(&actor.site_id).bind(cursor).bind(cursor).bind((limit+1) as i64).fetch_all(pool).await?;
    let next_cursor = if items.len() > limit {
        items.truncate(limit);
        items.last().map(|i| i.id.clone())
    } else {
        None
    };
    Ok(Page { items, next_cursor })
}
pub async fn set_external_ref(
    pool: &SqlitePool,
    actor: &Actor,
    kind: &str,
    id: &str,
    value: Option<String>,
) -> Result<Value, MaintError> {
    actor.require_permission(Permission::EquipmentsEdit)?;
    let table = match kind {
        "equipment" => "equipments",
        "counter" => "equipment_counters",
        _ => return Err(MaintError::BadRequest("Type de référence inconnu".into())),
    };
    let value = value
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    if value.as_ref().is_some_and(|s| s.chars().count() > 200) {
        return Err(MaintError::BadRequest(
            "Référence limitée à 200 caractères".into(),
        ));
    }
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let before:Option<(String,Option<String>)>=sqlx::query_as(&format!("SELECT site_id,external_ref FROM {table} WHERE id=? AND deleted_at IS NULL AND (? OR site_id=?)"))
        .bind(id).bind(actor.is_admin()).bind(&actor.site_id).fetch_optional(&mut *tx).await?;
    let (site, before) =
        before.ok_or_else(|| MaintError::NotFound("Équipement ou compteur".into()))?;
    sqlx::query(&format!(
        "UPDATE {table} SET external_ref=?,updated_at=? WHERE id=? AND site_id=?"
    ))
    .bind(&value)
    .bind(store::now())
    .bind(id)
    .bind(&site)
    .execute(&mut *tx)
    .await?;
    store::audit(
        &mut tx,
        actor,
        Audit {
            action: "external_ref",
            entity: kind,
            entity_id: Some(id),
            site_id: Some(&site),
            before: Some(json!({"external_ref":before})),
            after: Some(json!({"external_ref":value})),
            ip: None,
        },
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"id":id,"external_ref":value}))
}
pub async fn purge_history(pool: &SqlitePool, actor: &Actor) -> Result<Value, MaintError> {
    actor.require_permission(Permission::SettingsEdit)?;
    store::require_admin(actor)?;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let days = setting(&mut tx, "status_history_days")
        .await?
        .as_i64()
        .unwrap_or(90);
    let history = sqlx::query("DELETE FROM equipment_external_status_history WHERE received_at<?")
        .bind((Utc::now() - Duration::days(days)).to_rfc3339())
        .execute(&mut *tx)
        .await?
        .rows_affected();
    let events = sqlx::query("DELETE FROM integration_events WHERE received_at<?")
        .bind((Utc::now() - Duration::days(30)).to_rfc3339())
        .execute(&mut *tx)
        .await?
        .rows_affected();
    // Suppression physique autorisée par la conservation : elle reste tracée dans le journal.
    store::audit(
        &mut tx,
        actor,
        store::Audit {
            action: "purge",
            entity: "integration_history",
            entity_id: None,
            site_id: None,
            before: None,
            after: Some(
                json!({"history_deleted":history,"events_deleted":events,"history_days":days}),
            ),
            ip: None,
        },
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"history_deleted":history,"events_deleted":events}))
}
