//! Contexte de synchronisation, stocké dans la même transaction que l'opération métier.
use crate::{
    store::{self, Audit},
    Actor, MaintError, Permission,
};
use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use serde_json::{json, Value};
use sqlx::{SqliteConnection, SqlitePool};

#[derive(Clone, Default)]
pub struct Operation {
    pub key: Option<String>,
    pub route: String,
    pub occurred_at: Option<String>,
    pub unmodified_since: Option<String>,
}
tokio::task_local! { pub static OPERATION: Operation; }

pub async fn record<T: Serialize>(
    conn: &mut SqliteConnection,
    actor: &Actor,
    value: &T,
    code: u16,
) -> Result<(), MaintError> {
    let context = OPERATION.try_with(Clone::clone).ok();
    if let Some(context) = context {
        if let Some(key) = context.key {
            sqlx::query("INSERT INTO idempotency_keys(key,user_id,site_id,route,response_code,response_body,created_at) VALUES(?,?,?,?,?,?,?)")
                .bind(key).bind(&actor.user_id).bind(&actor.site_id).bind(context.route).bind(i64::from(code))
                .bind(serde_json::to_string(value).map_err(|e| MaintError::Internal(e.to_string()))?).bind(store::now()).execute(conn).await?;
        }
    }
    Ok(())
}

pub async fn occurred(
    conn: &mut SqliteConnection,
    actor: &Actor,
    previous: Option<&str>,
) -> Result<Option<String>, MaintError> {
    let raw = OPERATION.try_with(|c| c.occurred_at.clone()).ok().flatten();
    let Some(raw) = raw else {
        return Ok(None);
    };
    let now = Utc::now();
    let date = DateTime::parse_from_rfc3339(&raw)
        .ok()
        .map(|d| d.with_timezone(&Utc));
    let previous = previous.and_then(|s| DateTime::parse_from_rfc3339(s).ok());
    if let Some(date) = date.filter(|d| {
        *d >= now - Duration::days(7)
            && *d <= now + Duration::minutes(5)
            && previous.is_none_or(|p| *d >= p)
    }) {
        return Ok(Some(
            date.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        ));
    }
    store::audit(conn,actor,Audit{action:"offline_date_replaced",entity:"intervention",entity_id:None,site_id:actor.site_id.as_deref(),before:None,after:Some(json!({"reason":"Date absente des bornes ou antérieure à l'étape précédente","received_at":store::now()})),ip:None}).await?;
    Ok(Some(store::now()))
}

pub fn check_unmodified(updated_at: &str) -> Result<(), MaintError> {
    if OPERATION
        .try_with(|c| {
            c.unmodified_since
                .as_deref()
                .is_some_and(|s| s != updated_at)
        })
        .unwrap_or(false)
    {
        return Err(MaintError::PreconditionFailed(
            "L'intervention a été modifiée depuis votre instantané".into(),
        ));
    }
    Ok(())
}

pub async fn require_assignment(
    conn: &mut SqliteConnection,
    actor: &Actor,
    id: &str,
) -> Result<(), MaintError> {
    if actor.role == crate::Role::Technicien
        && OPERATION.try_with(|c| c.key.is_some()).unwrap_or(false)
    {
        let assigned:i64=sqlx::query_scalar("SELECT count(*) FROM intervention_technicians t JOIN interventions i ON i.id=t.intervention_id WHERE i.id=? AND i.site_id=? AND t.user_id=?")
            .bind(id).bind(&actor.site_id).bind(&actor.user_id).fetch_one(conn).await?;
        if assigned == 0 {
            return Err(MaintError::Forbidden(
                "Vous n'êtes plus assigné à cette intervention".into(),
            ));
        }
    }
    Ok(())
}

pub async fn snapshot(
    pool: &SqlitePool,
    actor: &Actor,
    since: Option<&str>,
) -> Result<Value, MaintError> {
    actor.require_permission(Permission::InterventionsView)?;
    let generated = store::now();
    let since = since
        .map(|s| {
            DateTime::parse_from_rfc3339(s)
                .map_err(|_| MaintError::BadRequest("Date since invalide".into()))
        })
        .transpose()?
        .filter(|d| *d >= Utc::now() - Duration::hours(24) && *d <= Utc::now())
        .map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
    // Une transaction de lecture fixe la photographie commune à toutes les collections.
    let mut tx = pool.begin().await?;
    let mut result = json!({"generated_at":generated,"full":since.is_none()});
    for (name, select) in [
        ("equipments", "SELECT json_object('id',id,'site_id',site_id,'code',code,'name',name,'status',status,'production_line_id',production_line_id,'parent_id',parent_id,'updated_at',updated_at,'deleted_at',deleted_at) FROM equipments WHERE (? OR site_id=?) AND ((? IS NULL AND deleted_at IS NULL) OR updated_at>=?)"),
        ("production_lines", "SELECT json_object('id',id,'site_id',site_id,'code',code,'name',name,'updated_at',updated_at,'deleted_at',deleted_at) FROM production_lines WHERE (? OR site_id=?) AND ((? IS NULL AND deleted_at IS NULL) OR updated_at>=?)"),
        ("counters", "SELECT json_object('id',id,'site_id',site_id,'equipment_id',equipment_id,'name',name,'unit',unit,'current_value',current_value,'active',json(CASE active WHEN 1 THEN 'true' ELSE 'false' END),'updated_at',updated_at,'deleted_at',deleted_at) FROM equipment_counters WHERE (? OR site_id=?) AND ((? IS NULL AND deleted_at IS NULL) OR updated_at>=?)"),
        ("parts", "SELECT json_object('id',p.id,'site_id',p.site_id,'reference',p.reference,'name',p.name,'available',COALESCE((SELECT sum(quantity) FROM stock s WHERE s.part_id=p.id),0)-COALESCE((SELECT sum(quantity) FROM stock_reservations r WHERE r.part_id=p.id AND r.status='prepared'),0),'updated_at',p.updated_at,'deleted_at',p.deleted_at) FROM parts p WHERE (? OR p.site_id=?) AND ((? IS NULL AND p.deleted_at IS NULL) OR p.updated_at>=? OR EXISTS(SELECT 1 FROM stock s WHERE s.part_id=p.id AND s.updated_at>=?4) OR EXISTS(SELECT 1 FROM stock_reservations r WHERE r.part_id=p.id AND r.updated_at>=?4))"),
        ("users", "SELECT json_object('id',id,'display_name',display_name,'updated_at',updated_at,'deleted_at',CASE WHEN active=0 THEN updated_at ELSE deleted_at END) FROM users WHERE (? OR site_id=?) AND ((? IS NULL AND deleted_at IS NULL AND active=1) OR updated_at>=?)"),
    ] {
        let rows: Vec<String> = sqlx::query_scalar(select).bind(actor.is_admin()).bind(&actor.site_id).bind(&since).bind(&since).fetch_all(&mut *tx).await?;
        result[name] = Value::Array(rows.into_iter().map(|s| serde_json::from_str(&s).map_err(|e| MaintError::Internal(e.to_string()))).collect::<Result<_,_>>()?);
    }
    let rows: Vec<String> = sqlx::query_scalar("SELECT json_object('id',i.id,'site_id',i.site_id,'code',i.code,'title',i.title,'description',i.description,'type',i.type,'priority',i.priority,'status',i.status,'machine_state',i.machine_state,'equipment_id',i.equipment_id,'reported_at',i.reported_at,'started_at',i.started_at,'completed_at',i.completed_at,'solution_report',i.solution_report,'duration_minutes',i.duration_minutes,'updated_at',i.updated_at,'deleted_at',i.deleted_at) FROM interventions i WHERE (? OR i.site_id=?) AND (? OR EXISTS(SELECT 1 FROM intervention_technicians t WHERE t.intervention_id=i.id AND t.user_id=?)) AND ((? IS NULL AND i.deleted_at IS NULL AND (i.status IN ('open','in_progress') OR i.updated_at>=?)) OR i.updated_at>=?)")
        .bind(actor.is_admin()).bind(&actor.site_id).bind(actor.is_admin()).bind(&actor.user_id).bind(&since).bind((Utc::now()-Duration::days(7)).to_rfc3339()).bind(&since).fetch_all(&mut *tx).await?;
    result["interventions"] = Value::Array(
        rows.into_iter()
            .map(|s| serde_json::from_str(&s).map_err(|e| MaintError::Internal(e.to_string())))
            .collect::<Result<_, _>>()?,
    );
    let mut technicians = Vec::new();
    for intervention in result["interventions"].as_array().unwrap() {
        let rows: Vec<String> = sqlx::query_scalar("SELECT json_object('intervention_id',intervention_id,'user_id',user_id,'is_lead',json(CASE is_lead WHEN 1 THEN 'true' ELSE 'false' END),'assigned_at',assigned_at,'duration_minutes',duration_minutes) FROM intervention_technicians WHERE intervention_id=?").bind(intervention["id"].as_str()).fetch_all(&mut *tx).await?;
        for row in rows {
            technicians.push(
                serde_json::from_str::<Value>(&row)
                    .map_err(|e| MaintError::Internal(e.to_string()))?,
            );
        }
    }
    result["technicians"] = json!(technicians);
    let visible:Vec<String>=sqlx::query_scalar("SELECT i.id FROM interventions i WHERE i.deleted_at IS NULL AND (? OR i.site_id=?) AND (? OR EXISTS(SELECT 1 FROM intervention_technicians t WHERE t.intervention_id=i.id AND t.user_id=?)) AND (i.status IN ('open','in_progress') OR i.updated_at>=?) ORDER BY i.id")
        .bind(actor.is_admin()).bind(&actor.site_id).bind(actor.is_admin()).bind(&actor.user_id).bind((Utc::now()-Duration::days(7)).to_rfc3339()).fetch_all(&mut *tx).await?;
    result["intervention_ids"] = json!(visible);
    tx.commit().await?;
    Ok(result)
}

pub async fn purge_keys(pool: &SqlitePool, actor: &Actor) -> Result<u64, MaintError> {
    actor.require_permission(Permission::SettingsEdit)?;
    store::require_admin(actor)?;
    Ok(
        sqlx::query("DELETE FROM idempotency_keys WHERE created_at<?")
            .bind((Utc::now() - Duration::days(30)).to_rfc3339())
            .execute(pool)
            .await?
            .rows_affected(),
    )
}
