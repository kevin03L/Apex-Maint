use super::*;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reading {
    pub counter_ref: String,
    pub value: i64,
    pub read_at: String,
    #[serde(default)]
    pub reset: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatusInput {
    pub equipment_ref: String,
    pub state: String,
    pub since: String,
    pub message: Option<String>,
}
#[derive(Default, Serialize)]
pub struct BatchReport {
    pub accepted: u32,
    pub rejected: u32,
    pub errors: Vec<Value>,
}

pub async fn ingest_counters(
    pool: &SqlitePool,
    actor: &Actor,
    key: &str,
    readings: Vec<Reading>,
) -> Result<BatchReport, MaintError> {
    actor.require_permission(Permission::CountersRecord)?;
    if readings.len() > 1000 {
        return Err(MaintError::BadRequest("Mille éléments au maximum".into()));
    }
    let start = std::time::Instant::now();
    let count = readings.len();
    let mut report = BatchReport::default();
    for (index, reading) in readings.into_iter().enumerate() {
        match counter(pool, actor, &reading).await {
            Ok(()) => report.accepted += 1,
            Err(e) if e.http_status() < 500 => {
                report.rejected += 1;
                if report.errors.len() < 50 {
                    report.errors.push(json!({"index":index,"counter_ref":reading.counter_ref,"message":e.to_string()}));
                }
            }
            Err(e) => return Err(e),
        }
    }
    journal(
        pool,
        actor,
        key,
        "counters",
        count,
        &report,
        start.elapsed().as_millis() as i64,
    )
    .await?;
    Ok(report)
}

async fn counter(pool: &SqlitePool, actor: &Actor, r: &Reading) -> Result<(), MaintError> {
    if r.value < 0 || r.counter_ref.len() > 500 {
        return Err(MaintError::BadRequest(
            "Valeur ou référence invalide".into(),
        ));
    }
    let date = DateTime::parse_from_rfc3339(&r.read_at)
        .map_err(|_| MaintError::BadRequest("Date de relevé invalide".into()))?
        .with_timezone(&Utc);
    if date > Utc::now() + Duration::minutes(5) {
        return Err(MaintError::BadRequest("Date de relevé future".into()));
    }
    let read_at = date.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let now = store::now();
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let found:Option<(String,String,i64,Option<String>)>=sqlx::query_as("SELECT c.id,c.site_id,c.current_value,c.last_reading_at FROM equipment_counters c JOIN equipments e ON e.id=c.equipment_id AND e.site_id=c.site_id WHERE c.external_ref=? AND c.site_id=? AND c.deleted_at IS NULL AND c.active=1 AND e.deleted_at IS NULL")
        .bind(&r.counter_ref).bind(&actor.site_id).fetch_optional(&mut *tx).await?;
    let (id, site, current, last) = found
        .ok_or_else(|| MaintError::NotFound("Référence de compteur inconnue ou inactive".into()))?;
    let historical = last
        .as_deref()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .is_some_and(|d| date < d);
    let duplicate:i64=sqlx::query_scalar("SELECT count(*) FROM counter_readings WHERE counter_id=? AND site_id=? AND read_at=? AND value=? AND is_reset=? AND source='supervision'").bind(&id).bind(&site).bind(&read_at).bind(r.value).bind(r.reset).fetch_one(&mut *tx).await?;
    if !historical && last.is_some() && current == r.value {
        // Conserver l'ordre chronologique sans ajouter de doublon à l'historique.
        sqlx::query(
            "UPDATE equipment_counters SET last_reading_at=?,updated_at=? WHERE id=? AND site_id=? AND julianday(last_reading_at)<julianday(?)",
        )
        .bind(&read_at)
        .bind(&now)
        .bind(&id)
        .bind(&site)
        .bind(&read_at)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        return Ok(());
    }
    if duplicate > 0 {
        tx.rollback().await?;
        return Ok(());
    }
    if !historical && !r.reset && r.value < current {
        return Err(MaintError::BadRequest("valeur en baisse".into()));
    }
    if r.reset && !historical {
        let plans:Vec<(String,i64,i64)>=sqlx::query_as("SELECT id,next_due_counter,counter_interval FROM maintenance_plans WHERE counter_id=? AND site_id=? AND deleted_at IS NULL")
            .bind(&id).bind(&site).fetch_all(&mut *tx).await?;
        for (plan, due, interval) in plans {
            let due = r
                .value
                .checked_add(
                    due.checked_sub(current)
                        .ok_or_else(|| MaintError::BadRequest("Échéance hors limites".into()))?,
                )
                .ok_or_else(|| MaintError::BadRequest("Échéance hors limites".into()))?;
            let anchor = if due >= interval { due % interval } else { due };
            sqlx::query("UPDATE maintenance_plans SET next_due_counter=?,anchor_counter=?,counter_cycle=counter_cycle+1,updated_at=? WHERE id=? AND site_id=?")
                .bind(due).bind(anchor).bind(&now).bind(&plan).bind(&site).execute(&mut *tx).await?;
            store::audit(
                &mut tx,
                actor,
                Audit {
                    action: "reset_adjust",
                    entity: "maintenance_plan",
                    entity_id: Some(&plan),
                    site_id: Some(&site),
                    before: None,
                    after: Some(json!({"next_due_counter":due,"anchor_counter":anchor})),
                    ip: None,
                },
            )
            .await?;
        }
    }
    let crossed:i64=sqlx::query_scalar("SELECT count(*) FROM maintenance_plans WHERE counter_id=? AND site_id=? AND deleted_at IS NULL AND active=1 AND ?<next_due_counter-lead_counter AND ?>=next_due_counter-lead_counter")
        .bind(&id).bind(&site).bind(current).bind(r.value).fetch_one(&mut *tx).await?;
    let persisted:Option<String>=sqlx::query_scalar("SELECT MAX(read_at) FROM counter_readings WHERE counter_id=? AND site_id=? AND source='supervision' AND deleted_at IS NULL")
        .bind(&id).bind(&site).fetch_one(&mut *tx).await?;
    let minutes = setting(&mut tx, "counter_min_interval_min")
        .await?
        .as_i64()
        .unwrap_or(15);
    let spaced = persisted
        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
        .is_none_or(|last| date >= last + Duration::minutes(minutes));
    if historical || spaced || crossed > 0 || r.reset {
        sqlx::query("INSERT INTO counter_readings(id,site_id,counter_id,value,read_at,source,is_reset,recorded_by_id,created_at,updated_at) VALUES(?,?,?,?,?,'supervision',?,NULL,?,?)")
            .bind(store::id()).bind(&site).bind(&id).bind(r.value).bind(&read_at).bind(r.reset).bind(&now).bind(&now).execute(&mut *tx).await?;
    }
    if !historical {
        sqlx::query("UPDATE equipment_counters SET current_value=?,last_reading_at=?,updated_at=? WHERE id=? AND site_id=?")
            .bind(r.value).bind(&read_at).bind(&now).bind(&id).bind(&site).execute(&mut *tx).await?;
        crate::maintenance::generate_counter_in_tx(&mut tx, actor, &id).await?;
    }
    store::audit(
        &mut tx,
        actor,
        Audit {
            action: "supervision_reading",
            entity: "equipment_counter",
            entity_id: Some(&id),
            site_id: Some(&site),
            before: None,
            after: Some(
                json!({"value":r.value,"read_at":read_at,"historical":historical,"reset":r.reset}),
            ),
            ip: None,
        },
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

pub async fn ingest_statuses(
    pool: &SqlitePool,
    actor: &Actor,
    key: &str,
    statuses: Vec<StatusInput>,
) -> Result<BatchReport, MaintError> {
    actor.require_permission(Permission::EquipmentsView)?;
    if statuses.len() > 1000 {
        return Err(MaintError::BadRequest("Mille éléments au maximum".into()));
    }
    let start = std::time::Instant::now();
    let count = statuses.len();
    let mut report = BatchReport::default();
    for (index, status) in statuses.into_iter().enumerate() {
        match status_one(pool, actor, &status).await {
            Ok(()) => report.accepted += 1,
            Err(e) if e.http_status() < 500 => {
                report.rejected += 1;
                if report.errors.len() < 50 {
                    report.errors.push(json!({"index":index,"equipment_ref":status.equipment_ref,"message":e.to_string()}));
                }
            }
            Err(e) => return Err(e),
        }
    }
    journal(
        pool,
        actor,
        key,
        "status",
        count,
        &report,
        start.elapsed().as_millis() as i64,
    )
    .await?;
    Ok(report)
}
async fn status_one(pool: &SqlitePool, actor: &Actor, s: &StatusInput) -> Result<(), MaintError> {
    if !["running", "stopped", "degraded", "alarm", "unknown"].contains(&s.state.as_str())
        || s.message.as_ref().is_some_and(|m| m.chars().count() > 500)
        || s.equipment_ref.len() > 500
    {
        return Err(MaintError::BadRequest(
            "État invalide ou message de plus de 500 caractères".into(),
        ));
    }
    let since = DateTime::parse_from_rfc3339(&s.since)
        .map_err(|_| MaintError::BadRequest("Date d'état invalide".into()))?
        .with_timezone(&Utc);
    if since > Utc::now() + Duration::minutes(5) {
        return Err(MaintError::BadRequest("Date d'état future".into()));
    }
    let since = since.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let now = store::now();
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let id:Option<String>=sqlx::query_scalar("SELECT id FROM equipments WHERE site_id=? AND deleted_at IS NULL AND (external_ref=? OR (external_ref IS NULL AND code=? COLLATE NOCASE)) ORDER BY (external_ref=?) DESC LIMIT 1")
        .bind(&actor.site_id).bind(&s.equipment_ref).bind(&s.equipment_ref).bind(&s.equipment_ref).fetch_optional(&mut *tx).await?;
    let id = id.ok_or_else(|| MaintError::NotFound("Référence d'équipement inconnue".into()))?;
    let previous: Option<(String, String)> = sqlx::query_as(
        "SELECT state,COALESCE(last_event_at,since) FROM equipment_external_status WHERE equipment_id=? AND site_id=?",
    )
    .bind(&id)
    .bind(&actor.site_id)
    .fetch_optional(&mut *tx)
    .await?;
    // Un lot retransmis après une coupure ne remplace pas un état plus récent.
    if previous.as_ref().is_some_and(|(_, date)| date > &since) {
        tx.rollback().await?;
        return Ok(());
    }
    if previous.as_ref().is_none_or(|(state, _)| state != &s.state) {
        sqlx::query("INSERT INTO equipment_external_status_history(id,equipment_id,site_id,state,since,message,received_at) VALUES(?,?,?,?,?,?,?)")
            .bind(store::id()).bind(&id).bind(&actor.site_id).bind(&s.state).bind(&since).bind(&s.message).bind(&now).execute(&mut *tx).await?;
    }
    sqlx::query("INSERT INTO equipment_external_status(equipment_id,site_id,state,since,message,received_at,last_event_at) VALUES(?,?,?,?,?,?,?) ON CONFLICT(equipment_id) DO UPDATE SET state=excluded.state,since=CASE WHEN equipment_external_status.state=excluded.state THEN equipment_external_status.since ELSE excluded.since END,message=excluded.message,received_at=excluded.received_at,last_event_at=excluded.last_event_at")
        .bind(&id).bind(&actor.site_id).bind(&s.state).bind(&since).bind(&s.message).bind(&now).bind(&since).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

async fn journal(
    pool: &SqlitePool,
    actor: &Actor,
    key: &str,
    kind: &str,
    count: usize,
    report: &BatchReport,
    duration: i64,
) -> Result<(), MaintError> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    sqlx::query("INSERT INTO integration_events(id,site_id,key_id,kind,received_at,items_count,accepted,rejected,errors,duration_ms) VALUES(?,?,?,?,?,?,?,?,?,?)")
        .bind(store::id()).bind(&actor.site_id).bind(key).bind(kind).bind(store::now()).bind(count as i64).bind(i64::from(report.accepted)).bind(i64::from(report.rejected)).bind(json!(report.errors).to_string()).bind(duration).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}
