//! Agrégats de pilotage sur des périodes locales, avec périmètre de site obligatoire.
use crate::{Actor, MaintError, Permission};
use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{QueryBuilder, Row, Sqlite, SqlitePool};

#[derive(Clone, Default, Deserialize, Serialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct Filter {
    pub period: Option<String>,
    pub site_id: Option<String>,
    pub line_id: Option<String>,
    pub equipment_id: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
}
fn boundary(value: &str, end: bool) -> Result<DateTime<Utc>, MaintError> {
    if let Ok(d) = DateTime::parse_from_rfc3339(value) {
        return Ok(d.with_timezone(&Utc));
    }
    let date = NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|_| MaintError::BadRequest("Date de période invalide".into()))?;
    let date = if end {
        date.succ_opt()
            .ok_or_else(|| MaintError::BadRequest("Date invalide".into()))?
    } else {
        date
    };
    Local
        .from_local_datetime(&date.and_hms_opt(0, 0, 0).expect("minuit"))
        .earliest()
        .map(|d| d.with_timezone(&Utc))
        .ok_or_else(|| MaintError::BadRequest("Date locale invalide".into()))
}
pub fn period(f: &Filter) -> Result<(DateTime<Utc>, DateTime<Utc>), MaintError> {
    let now = Local::now();
    let end = now.with_timezone(&Utc);
    let (from, to) = if f.from.is_some() || f.to.is_some() {
        (
            boundary(
                f.from
                    .as_deref()
                    .ok_or_else(|| MaintError::BadRequest("Date de début requise".into()))?,
                false,
            )?,
            boundary(
                f.to.as_deref()
                    .ok_or_else(|| MaintError::BadRequest("Date de fin requise".into()))?,
                true,
            )?,
        )
    } else {
        match f.period.as_deref().unwrap_or("30d") {
            "7d" => (end - Duration::days(7), end),
            "30d" => (end - Duration::days(30), end),
            "90d" => (end - Duration::days(90), end),
            "12m" => {
                let date = now
                    .date_naive()
                    .checked_sub_months(chrono::Months::new(12))
                    .ok_or_else(|| MaintError::BadRequest("Période invalide".into()))?;
                (boundary(&date.to_string(), false)?, end)
            }
            "month" => (
                boundary(&format!("{}-{:02}-01", now.year(), now.month()), false)?,
                end,
            ),
            "year" => (boundary(&format!("{}-01-01", now.year()), false)?, end),
            _ => return Err(MaintError::BadRequest("Période inconnue".into())),
        }
    };
    if from >= to || to - from > Duration::days(3660) {
        return Err(MaintError::BadRequest(
            "Période invalide (10 ans au maximum)".into(),
        ));
    }
    Ok((from, to))
}
pub async fn check(pool: &SqlitePool, actor: &Actor, f: &Filter) -> Result<(), MaintError> {
    if !actor.is_admin()
        && f.site_id
            .as_ref()
            .is_some_and(|s| Some(s) != actor.site_id.as_ref())
    {
        return Err(MaintError::NotFound("Site".into()));
    }
    for (table, id) in [
        ("sites", &f.site_id),
        ("production_lines", &f.line_id),
        ("equipments", &f.equipment_id),
    ] {
        if let Some(id) = id {
            let column = if table == "sites" { "id" } else { "site_id" };
            let exists:bool=sqlx::query_scalar(&format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE id=? AND deleted_at IS NULL AND (? OR {column}=?))")).bind(id).bind(actor.is_admin()).bind(&actor.site_id).fetch_one(pool).await?;
            if !exists {
                return Err(MaintError::NotFound(
                    "Filtre de site, ligne ou équipement".into(),
                ));
            }
        }
    }
    Ok(())
}
fn scope(q: &mut QueryBuilder<'_, Sqlite>, actor: &Actor, f: &Filter, alias: &str) {
    if !actor.is_admin() {
        q.push(format!(" AND {alias}.site_id="))
            .push_bind(actor.site_id.clone());
    }
    if let Some(v) = &f.site_id {
        q.push(format!(" AND {alias}.site_id="))
            .push_bind(v.clone());
    }
}
fn equipment_scope(q: &mut QueryBuilder<'_, Sqlite>, f: &Filter, column: &str) {
    if let Some(v) = &f.equipment_id {
        q.push(format!(" AND {column}=")).push_bind(v.clone());
    }
    if let Some(v) = &f.line_id {
        q.push(format!(" AND EXISTS(SELECT 1 FROM equipments eq WHERE eq.id={column} AND eq.deleted_at IS NULL AND eq.production_line_id=")).push_bind(v.clone()).push(")");
    }
}
fn intervention_query(
    select: &str,
    actor: &Actor,
    f: &Filter,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    overlap: bool,
) -> QueryBuilder<'static, Sqlite> {
    let mut q = QueryBuilder::new(format!(
        "{select} FROM interventions i WHERE i.deleted_at IS NULL AND i.status<>'cancelled'"
    ));
    scope(&mut q, actor, f, "i");
    equipment_scope(&mut q, f, "i.equipment_id");
    if let Some(v) = &f.kind {
        q.push(" AND i.type=").push_bind(v.clone());
    }
    q.push(" AND i.reported_at<").push_bind(to.to_rfc3339());
    if overlap {
        q.push(" AND (i.completed_at IS NULL OR i.completed_at>=")
            .push_bind(from.to_rfc3339())
            .push(")");
    } else {
        q.push(" AND i.reported_at>=").push_bind(from.to_rfc3339());
    }
    q
}
async fn grouped(
    pool: &SqlitePool,
    actor: &Actor,
    f: &Filter,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    column: &str,
) -> Result<Value, MaintError> {
    let mut q = intervention_query(
        &format!("SELECT {column} AS key,count(*) AS count"),
        actor,
        f,
        from,
        to,
        false,
    );
    q.push(format!(" GROUP BY {column} ORDER BY {column}"));
    let rows = q.build().fetch_all(pool).await?;
    Ok(json!(rows
        .iter()
        .map(|r| json!({"key":r.get::<String,_>("key"),"count":r.get::<i64,_>("count")}))
        .collect::<Vec<_>>()))
}
pub async fn report(
    pool: &SqlitePool,
    actor: &Actor,
    name: &str,
    f: &Filter,
) -> Result<Value, MaintError> {
    actor.require_permission(Permission::ReportingView)?;
    if ["costs", "stock"].contains(&name) {
        actor.require_permission(Permission::StockView)?;
    }
    if !["activity", "reliability", "preventive", "costs", "stock"].contains(&name) {
        return Err(MaintError::NotFound("Rapport".into()));
    }
    check(pool, actor, f).await?;
    let (from, to) = period(f)?;
    let mut data = data(pool, actor, name, f, from, to).await?;
    enrich(pool, &mut data).await?;
    Ok(json!({"report":name,"from":from.to_rfc3339(),"to":to.to_rfc3339(),"data":data}))
}
async fn data(
    pool: &SqlitePool,
    actor: &Actor,
    name: &str,
    f: &Filter,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> Result<Value, MaintError> {
    match name {
        "activity" => {
            let mut q = intervention_query(
                "SELECT (julianday(i.started_at)-julianday(i.reported_at))*1440.0 AS delay",
                actor,
                f,
                from,
                to,
                false,
            );
            q.push(" AND i.started_at IS NOT NULL ORDER BY delay");
            let delays: Vec<f64> = q.build_query_scalar().fetch_all(pool).await?;
            let mean = if delays.is_empty() {
                None
            } else {
                Some(delays.iter().sum::<f64>() / delays.len() as f64)
            };
            let median = if delays.is_empty() {
                None
            } else {
                Some((delays[(delays.len() - 1) / 2] + delays[delays.len() / 2]) / 2.)
            };
            Ok(
                json!({"by_status":grouped(pool,actor,f,from,to,"i.status").await?,"by_type":grouped(pool,actor,f,from,to,"i.type").await?,"by_priority":grouped(pool,actor,f,from,to,"i.priority").await?,"response_mean_minutes":mean,"response_median_minutes":median}),
            )
        }
        "reliability" => {
            let mut q=QueryBuilder::new("SELECT i.equipment_id,count(*) AS failures,sum(CASE WHEN i.completed_at IS NOT NULL AND i.completed_at>=");
            q.push_bind(from.to_rfc3339()).push(" AND i.completed_at<").push_bind(to.to_rfc3339()).push(" THEN 1 ELSE 0 END) AS closed_failures,sum(MAX(0,(julianday(MIN(COALESCE(i.completed_at,").push_bind(to.to_rfc3339()).push("),").push_bind(to.to_rfc3339()).push("))-julianday(MAX(i.reported_at,").push_bind(from.to_rfc3339()).push("))))*1440.0) AS downtime_minutes FROM interventions i WHERE i.deleted_at IS NULL AND i.status<>'cancelled' AND i.type='corrective' AND i.machine_state='ARRET'");

            scope(&mut q, actor, f, "i");
            equipment_scope(&mut q, f, "i.equipment_id");
            if let Some(kind) = &f.kind {
                q.push(" AND i.type=").push_bind(kind.clone());
            }
            q.push(" AND i.reported_at<")
                .push_bind(to.to_rfc3339())
                .push(" AND (i.completed_at IS NULL OR i.completed_at>=")
                .push_bind(from.to_rfc3339())
                .push(") GROUP BY i.equipment_id ORDER BY downtime_minutes DESC");
            let rows = q.build().fetch_all(pool).await?;
            let calendar = (to - from).num_seconds() as f64 / 60.;
            let items:Vec<_>=rows.iter().map(|r|{let failures:i64=r.get("failures");let closed:i64=r.get("closed_failures");let downtime:f64=r.get("downtime_minutes");json!({"equipment_id":r.get::<Option<String>,_>("equipment_id"),"failures":failures,"closed_failures":closed,"downtime_minutes":downtime,"mttr_minutes":if closed==0{None}else{Some(downtime/closed as f64)},"mtbf_minutes":Some((calendar-downtime)/failures as f64)})}).collect();
            let failures = rows
                .iter()
                .map(|r| r.get::<i64, _>("failures"))
                .sum::<i64>();
            let closed = rows
                .iter()
                .map(|r| r.get::<i64, _>("closed_failures"))
                .sum::<i64>();
            let downtime = rows
                .iter()
                .map(|r| r.get::<f64, _>("downtime_minutes"))
                .sum::<f64>();
            Ok(
                json!({"items":items,"failures":failures,"closed_failures":closed,"downtime_minutes":downtime,"mttr_minutes":if closed==0{None}else{Some(downtime/closed as f64)},"mtbf_minutes":if failures==0{None}else{Some((calendar-downtime)/failures as f64)},"no_failures":rows.is_empty(),"message":if rows.is_empty(){Some("aucune panne sur la période")}else{None::<&str>}}),
            )
        }
        "preventive" => {
            let mut q=QueryBuilder::new("SELECT count(*) AS due_count,COALESCE(sum(CASE WHEN o.status='done' AND substr(i.completed_at,1,10)<=o.due_date THEN 1 ELSE 0 END),0) AS on_time_count FROM maintenance_occurrences o JOIN maintenance_plans p ON p.id=o.plan_id LEFT JOIN interventions i ON i.id=o.intervention_id AND i.deleted_at IS NULL WHERE o.deleted_at IS NULL AND p.deleted_at IS NULL");
            scope(&mut q, actor, f, "o");
            equipment_scope(&mut q, f, "p.equipment_id");
            if let Some(kind) = &f.kind {
                q.push(" AND i.type=").push_bind(kind.clone());
            }
            q.push(" AND o.due_date>=")
                .push_bind(from.with_timezone(&Local).date_naive().to_string())
                .push(" AND o.due_date<")
                .push_bind(to.with_timezone(&Local).date_naive().to_string());
            q.push(" AND o.due_date<=")
                .push_bind(Local::now().date_naive().to_string());
            let r = q.build().fetch_one(pool).await?;
            let due: i64 = r.get("due_count");
            let on_time: i64 = r.get("on_time_count");
            Ok(
                json!({"due_count":due,"on_time_count":on_time,"compliance_percent":if due==0{None}else{Some(on_time as f64*100./due as f64)},"by_type":grouped(pool,actor,f,from,to,"i.type").await?}),
            )
        }
        "costs" => {
            let mut q=QueryBuilder::new("SELECT i.equipment_id,COALESCE(sum(ip.quantity*ip.unit_price_cents),0) AS cost_cents FROM intervention_parts ip JOIN interventions i ON i.id=ip.intervention_id WHERE i.deleted_at IS NULL AND ip.deleted_at IS NULL AND i.status<>'cancelled'");
            scope(&mut q, actor, f, "ip");
            equipment_scope(&mut q, f, "i.equipment_id");
            if let Some(kind) = &f.kind {
                q.push(" AND i.type=").push_bind(kind.clone());
            }
            q.push(" AND ip.created_at>=")
                .push_bind(from.to_rfc3339())
                .push(" AND ip.created_at<")
                .push_bind(to.to_rfc3339())
                .push(" GROUP BY i.equipment_id ORDER BY i.equipment_id");
            let parts = q.build().fetch_all(pool).await?;
            let mut labor=QueryBuilder::new("SELECT i.equipment_id,t.user_id,COALESCE(sum(t.duration_minutes),0) AS duration_minutes FROM intervention_technicians t JOIN interventions i ON i.id=t.intervention_id WHERE i.deleted_at IS NULL AND i.status<>'cancelled'");
            scope(&mut labor, actor, f, "i");
            equipment_scope(&mut labor, f, "i.equipment_id");
            if let Some(kind) = &f.kind {
                labor.push(" AND i.type=").push_bind(kind.clone());
            }
            labor
                .push(" AND i.reported_at>=")
                .push_bind(from.to_rfc3339())
                .push(" AND i.reported_at<")
                .push_bind(to.to_rfc3339())
                .push(" GROUP BY i.equipment_id,t.user_id ORDER BY i.equipment_id,t.user_id");
            let time = labor.build().fetch_all(pool).await?;
            Ok(
                json!({"parts_cost_cents":parts.iter().map(|r|r.get::<i64,_>("cost_cents")).sum::<i64>(),"by_equipment":parts.iter().map(|r|json!({"equipment_id":r.get::<Option<String>,_>("equipment_id"),"cost_cents":r.get::<i64,_>("cost_cents")})).collect::<Vec<_>>(),"labor":time.iter().map(|r|json!({"equipment_id":r.get::<Option<String>,_>("equipment_id"),"user_id":r.get::<String,_>("user_id"),"duration_minutes":r.get::<i64,_>("duration_minutes")})).collect::<Vec<_>>()}),
            )
        }
        "stock" => {
            let mut q=QueryBuilder::new("SELECT p.id AS part_id,p.reference,p.name,p.unit_price_cents,COALESCE((SELECT sum(s.quantity) FROM stock s WHERE s.part_id=p.id AND s.site_id=p.site_id),0) AS quantity,COALESCE(p.alert_threshold,CAST((SELECT value FROM app_settings WHERE key='stock_alert_threshold') AS INTEGER)) AS threshold FROM parts p WHERE p.deleted_at IS NULL");
            scope(&mut q, actor, f, "p");
            if f.equipment_id.is_some() || f.line_id.is_some() {
                q.push(" AND EXISTS(SELECT 1 FROM equipment_parts ep WHERE ep.part_id=p.id");
                equipment_scope(&mut q, f, "ep.equipment_id");
                q.push(")");
            }
            q.push(" ORDER BY p.reference,p.id");
            let rows = q.build().fetch_all(pool).await?;
            let items:Vec<_>=rows.iter().map(|r|{let qty:i64=r.get("quantity");let price:i64=r.get("unit_price_cents");let threshold:i64=r.get("threshold");json!({"part_id":r.get::<String,_>("part_id"),"reference":r.get::<String,_>("reference"),"name":r.get::<String,_>("name"),"quantity":qty,"unit_price_cents":price,"value_cents":qty*price,"threshold":threshold,"low":qty<=threshold})}).collect();
            Ok(
                json!({"value_cents":items.iter().map(|v|v["value_cents"].as_i64().unwrap_or(0)).sum::<i64>(),"items":items}),
            )
        }
        _ => Err(MaintError::NotFound("Rapport".into())),
    }
}
pub async fn widget(
    pool: &SqlitePool,
    actor: &Actor,
    name: &str,
    f: &Filter,
) -> Result<Value, MaintError> {
    actor.require_permission(Permission::DashboardView)?;
    for p in super::dashboard::permissions(name)? {
        actor.require_permission(*p)?;
    }
    check(pool, actor, f).await?;
    let (from, to) = period(f)?;
    let result = match name {
        "interventions_summary" => json!({"items":grouped(pool,actor,f,from,to,"i.status").await?}),
        "interventions_by_type" => json!({"items":grouped(pool,actor,f,from,to,"i.type").await?}),
        "interventions_by_priority" => {
            json!({"items":grouped(pool,actor,f,from,to,"i.priority").await?})
        }
        "response_time" => {
            let d = data(pool, actor, "activity", f, from, to).await?;
            json!({"mean_minutes":d["response_mean_minutes"],"median_minutes":d["response_median_minutes"]})
        }
        "mtbf_mttr" => data(pool, actor, "reliability", f, from, to).await?,
        "downtime" => {
            let mut d = data(pool, actor, "reliability", f, from, to).await?;
            if let Some(items) = d["items"].as_array_mut() {
                items.truncate(10);
            }
            d
        }
        "preventive_compliance" => data(pool, actor, "preventive", f, from, to).await?,
        "parts_cost" => data(pool, actor, "costs", f, from, to).await?,
        "labor_time" => {
            let d = data(pool, actor, "costs", f, from, to).await?;
            json!({"labor":d["labor"]})
        }
        "stock_value" => data(pool, actor, "stock", f, from, to).await?,
        "stock_alerts" => {
            let mut d = data(pool, actor, "stock", f, from, to).await?;
            if let Some(items) = d["items"].as_array_mut() {
                items.retain(|v| v["low"] == true);
            }
            d
        }
        "my_interventions" => {
            let mut q = intervention_query(
                "SELECT i.id,i.code,i.title,i.status,i.priority",
                actor,
                f,
                from,
                to,
                false,
            );
            q.push(" AND EXISTS(SELECT 1 FROM intervention_technicians t WHERE t.intervention_id=i.id AND t.user_id=").push_bind(actor.user_id.clone()).push(") ORDER BY i.reported_at DESC,i.id DESC LIMIT 100");
            let rows = q.build().fetch_all(pool).await?;
            json!({"items":rows.iter().map(|r|json!({"id":r.get::<String,_>("id"),"code":r.get::<String,_>("code"),"title":r.get::<String,_>("title"),"status":r.get::<String,_>("status"),"priority":r.get::<String,_>("priority")})).collect::<Vec<_>>()})
        }
        "machines_down" => {
            let mut q=QueryBuilder::new("SELECT e.id,e.code,e.name,e.status FROM equipments e WHERE e.deleted_at IS NULL AND e.status IN ('degraded','out_of_service')");
            scope(&mut q, actor, f, "e");
            equipment_scope(&mut q, f, "e.id");
            q.push(" ORDER BY e.code,e.id");
            let rows = q.build().fetch_all(pool).await?;
            json!({"items":rows.iter().map(|r|json!({"id":r.get::<String,_>("id"),"code":r.get::<String,_>("code"),"name":r.get::<String,_>("name"),"status":r.get::<String,_>("status")})).collect::<Vec<_>>()})
        }
        "maintenance_overdue" => {
            let today = Local::now().date_naive();
            let horizon = if f.to.is_some() {
                to.with_timezone(&Local).date_naive()
            } else {
                today + Duration::days((to - from).num_days().max(1))
            };
            let mut q=QueryBuilder::new("SELECT p.id,p.code,p.title,p.next_due_date,p.next_due_counter,c.current_value FROM maintenance_plans p LEFT JOIN equipment_counters c ON c.id=p.counter_id AND c.active=1 AND c.deleted_at IS NULL WHERE p.deleted_at IS NULL AND p.active=1");
            scope(&mut q, actor, f, "p");
            equipment_scope(&mut q, f, "p.equipment_id");
            q.push(" AND (p.next_due_date<=")
                .push_bind(horizon.to_string())
                .push(" OR c.current_value>=p.next_due_counter-p.lead_counter) ORDER BY p.next_due_date,p.id");
            let rows = q.build().fetch_all(pool).await?;
            json!({"items":rows.iter().map(|r|{let due=r.get::<Option<String>,_>("next_due_date");let counter=r.get::<Option<i64>,_>("next_due_counter");let overdue=due.as_deref().is_some_and(|d|d<today.to_string().as_str())||counter.zip(r.get::<Option<i64>,_>("current_value")).is_some_and(|(due,current)|current>=due);json!({"id":r.get::<String,_>("id"),"code":r.get::<String,_>("code"),"title":r.get::<String,_>("title"),"next_due_date":due,"next_due_counter":counter,"overdue":overdue})}).collect::<Vec<_>>()})
        }
        "supervision_status" => {
            // Phase 10 : état à titre d'information, équipements à l'arrêt ou en alarme.
            let page =
                crate::integration::statuses(pool, actor, None, Some("attention"), None, 200)
                    .await?;
            json!({"items":page.items})
        }
        _ => return Err(MaintError::NotFound("Widget".into())),
    };
    let mut result = result;
    enrich(pool, &mut result).await?;
    Ok(json!({"widget":name,"from":from.to_rfc3339(),"to":to.to_rfc3339(),"data":result}))
}

/// Complète les lignes qui ne portent qu'un identifiant : code et nom de l'équipement, nom de
/// l'utilisateur. Les identifiants viennent de requêtes déjà limitées au périmètre de l'acteur.
async fn enrich(pool: &SqlitePool, data: &mut Value) -> Result<(), MaintError> {
    let mut equipments: std::collections::HashMap<String, (String, String)> = Default::default();
    let mut users: std::collections::HashMap<String, String> = Default::default();
    let Some(sections) = data.as_object_mut() else {
        return Ok(());
    };
    for section in sections.values_mut() {
        let Some(items) = section.as_array_mut() else {
            continue;
        };
        for item in items.iter_mut() {
            let Some(row) = item.as_object_mut() else {
                continue;
            };
            if let Some(id) = row
                .get("equipment_id")
                .and_then(Value::as_str)
                .map(str::to_owned)
            {
                if !equipments.contains_key(&id) {
                    let found: Option<(String, String)> =
                        sqlx::query_as("SELECT code,name FROM equipments WHERE id=?")
                            .bind(&id)
                            .fetch_optional(pool)
                            .await?;
                    equipments.insert(id.clone(), found.unwrap_or_default());
                }
                let (code, name) = &equipments[&id];
                row.insert("equipment_code".into(), json!(code));
                row.insert("equipment_name".into(), json!(name));
            }
            if let Some(id) = row
                .get("user_id")
                .and_then(Value::as_str)
                .map(str::to_owned)
            {
                if !users.contains_key(&id) {
                    let found: Option<String> =
                        sqlx::query_scalar("SELECT display_name FROM users WHERE id=?")
                            .bind(&id)
                            .fetch_optional(pool)
                            .await?;
                    users.insert(id.clone(), found.unwrap_or_default());
                }
                row.insert("user_display_name".into(), json!(users[&id]));
            }
        }
    }
    Ok(())
}
