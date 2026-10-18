//! Catalogue fixe et dispositions personnelles ou communes aux rôles.
use crate::{store, Actor, MaintError, Permission, Role};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::SqlitePool;

pub const WIDGETS: &[(&str, &str, &[Permission])] = &[
    (
        "interventions_summary",
        "Synthèse des interventions",
        &[Permission::InterventionsView],
    ),
    (
        "interventions_by_type",
        "Interventions par type",
        &[Permission::InterventionsView],
    ),
    (
        "interventions_by_priority",
        "Interventions par priorité",
        &[Permission::InterventionsView],
    ),
    (
        "my_interventions",
        "Mes interventions",
        &[Permission::InterventionsView],
    ),
    (
        "machines_down",
        "Machines à l'arrêt ou dégradées",
        &[Permission::EquipmentsView],
    ),
    (
        "response_time",
        "Délai de prise en charge",
        &[Permission::InterventionsView],
    ),
    ("mtbf_mttr", "Fiabilité", &[Permission::ReportingView]),
    ("downtime", "Temps d'arrêt", &[Permission::ReportingView]),
    (
        "preventive_compliance",
        "Réalisation préventive",
        &[Permission::MaintenanceView],
    ),
    (
        "maintenance_overdue",
        "Échéances préventives",
        &[Permission::MaintenanceView],
    ),
    (
        "parts_cost",
        "Coût des pièces",
        &[Permission::StockView, Permission::ReportingView],
    ),
    ("labor_time", "Temps passé", &[Permission::ReportingView]),
    ("stock_value", "Valeur du stock", &[Permission::StockView]),
    ("stock_alerts", "Alertes de stock", &[Permission::StockView]),
];
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layout {
    pub widgets: Vec<Widget>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Widget {
    pub id: String,
    pub widget: String,
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    #[serde(default = "params_default")]
    pub params: Value,
}
fn params_default() -> Value {
    json!({})
}
pub fn permissions(widget: &str) -> Result<&'static [Permission], MaintError> {
    WIDGETS
        .iter()
        .find(|w| w.0 == widget)
        .map(|w| w.2)
        .ok_or_else(|| MaintError::NotFound("Widget".into()))
}
pub fn validate(layout: &Layout) -> Result<(), MaintError> {
    if layout.widgets.len() > 24 {
        return Err(MaintError::BadRequest("24 widgets au maximum".into()));
    }
    let mut ids = std::collections::HashSet::new();
    for w in &layout.widgets {
        permissions(&w.widget).map_err(|_| MaintError::BadRequest("Widget inconnu".into()))?;
        if w.id.is_empty()
            || w.id.len() > 128
            || !ids.insert(&w.id)
            || w.w == 0
            || w.w > 12
            || w.x > 12 - w.w
            || w.h == 0
            || w.h > 12
            || w.y > 10000
        {
            return Err(MaintError::BadRequest(
                "Position, taille ou identifiant de widget invalide".into(),
            ));
        }
        let params = w
            .params
            .as_object()
            .ok_or_else(|| MaintError::BadRequest("Paramètres de widget invalides".into()))?;
        if params.keys().any(|k| {
            !["period", "site_id", "line_id", "equipment_id", "from", "to"].contains(&k.as_str())
        }) || params.values().any(|v| !v.is_null() && !v.is_string())
        {
            return Err(MaintError::BadRequest(
                "Paramètres de widget invalides".into(),
            ));
        }
        if let Some(p) = params.get("period").and_then(Value::as_str) {
            if !["7d", "30d", "90d", "12m", "month", "year", "custom"].contains(&p) {
                return Err(MaintError::BadRequest("Période inconnue".into()));
            }
        }
        let filter: super::reports::Filter = serde_json::from_value(w.params.clone())
            .map_err(|_| MaintError::BadRequest("Paramètres de widget invalides".into()))?;
        super::reports::period(&filter)?;
    }
    Ok(())
}
pub fn catalog(actor: &Actor) -> Result<Value, MaintError> {
    actor.require_permission(Permission::DashboardView)?;
    Ok(json!(WIDGETS.iter().filter(|w|w.2.iter().all(|p|actor.can(*p))).map(|w|json!({"widget":w.0,"title":w.1,"permissions":w.2,"sizes":[{"w":4,"h":2},{"w":6,"h":3},{"w":12,"h":4}],"params":{"period":"30d","site_id":null,"line_id":null,"equipment_id":null,"from":null,"to":null}})).collect::<Vec<_>>()))
}
fn builtin(role: Role) -> Layout {
    let names: &[&str] = match role {
        Role::Production => &["interventions_summary", "machines_down"],
        Role::Technicien => &["my_interventions", "maintenance_overdue", "machines_down"],
        _ => &[
            "interventions_summary",
            "mtbf_mttr",
            "preventive_compliance",
            "stock_alerts",
        ],
    };
    Layout {
        widgets: names
            .iter()
            .enumerate()
            .map(|(i, n)| Widget {
                id: format!("widget-{i}"),
                widget: (*n).into(),
                x: (i as u32 % 2) * 6,
                y: i as u32 / 2 * 3,
                w: 6,
                h: 3,
                params: json!({"period":"30d"}),
            })
            .collect(),
    }
}
pub async fn resolve(pool: &SqlitePool, actor: &Actor) -> Result<Value, MaintError> {
    actor.require_permission(Permission::DashboardView)?;
    let personal: Option<String> = if actor.can(Permission::DashboardCustomize) {
        sqlx::query_scalar("SELECT layout FROM dashboard_layouts WHERE scope='user' AND user_id=? AND deleted_at IS NULL").bind(&actor.user_id).fetch_optional(pool).await?
    } else {
        None
    };
    let role: Option<String> = sqlx::query_scalar(
        "SELECT layout FROM dashboard_layouts WHERE scope='role' AND role=? AND deleted_at IS NULL",
    )
    .bind(actor.role.as_str())
    .fetch_optional(pool)
    .await?;
    let (mut layout, source) = if let Some(s) = personal {
        (decode(&s)?, "user")
    } else if let Some(s) = role {
        (decode(&s)?, "role")
    } else {
        (builtin(actor.role), "builtin")
    };
    layout
        .widgets
        .retain(|w| permissions(&w.widget).is_ok_and(|ps| ps.iter().all(|p| actor.can(*p))));
    Ok(
        json!({"widgets":layout.widgets,"source":source,"can_customize":actor.can(Permission::DashboardCustomize)}),
    )
}
fn decode(s: &str) -> Result<Layout, MaintError> {
    serde_json::from_str(s)
        .map_err(|_| MaintError::Internal("Disposition enregistrée invalide".into()))
}
pub async fn defaults(pool: &SqlitePool, actor: &Actor, role: Role) -> Result<Layout, MaintError> {
    actor.require_permission(Permission::DashboardManageDefaults)?;
    let s: Option<String> = sqlx::query_scalar(
        "SELECT layout FROM dashboard_layouts WHERE scope='role' AND role=? AND deleted_at IS NULL",
    )
    .bind(role.as_str())
    .fetch_optional(pool)
    .await?;
    s.map(|s| decode(&s)).unwrap_or_else(|| Ok(builtin(role)))
}
pub async fn save(
    pool: &SqlitePool,
    actor: &Actor,
    role: Option<Role>,
    layout: Option<Layout>,
) -> Result<(), MaintError> {
    actor.require_permission(if role.is_some() {
        Permission::DashboardManageDefaults
    } else {
        Permission::DashboardCustomize
    })?;
    if let Some(l) = &layout {
        validate(l)?;
    }
    let scope = if role.is_some() { "role" } else { "user" };
    let key = role
        .map(|r| r.to_string())
        .unwrap_or_else(|| actor.user_id.clone());
    let column = if role.is_some() { "role" } else { "user_id" };
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let before: Option<String> = sqlx::query_scalar(&format!(
        "SELECT layout FROM dashboard_layouts WHERE scope=? AND {column}=? AND deleted_at IS NULL"
    ))
    .bind(scope)
    .bind(&key)
    .fetch_optional(&mut *tx)
    .await?;
    let value = layout.as_ref().map(|l| json!(l));
    sqlx::query(&format!("UPDATE dashboard_layouts SET deleted_at=?,updated_at=?,updated_by_id=? WHERE scope=? AND {column}=?"))
 .bind(store::now()).bind(store::now()).bind(&actor.user_id).bind(scope).bind(&key).execute(&mut *tx).await?;
    if let Some(l) = &value {
        sqlx::query(&format!("INSERT INTO dashboard_layouts(scope,{column},layout,updated_at,updated_by_id) VALUES(?,?,?,?,?) ON CONFLICT({column}) WHERE scope='{scope}' DO UPDATE SET layout=excluded.layout,updated_at=excluded.updated_at,updated_by_id=excluded.updated_by_id,deleted_at=NULL"))
  .bind(scope).bind(&key).bind(l.to_string()).bind(store::now()).bind(&actor.user_id).execute(&mut *tx).await?;
    }
    store::audit(
        &mut tx,
        actor,
        store::Audit {
            action: if layout.is_some() { "update" } else { "delete" },
            entity: "dashboard_layout",
            entity_id: Some(&key),
            site_id: actor.site_id.as_deref(),
            before: before.and_then(|s| serde_json::from_str(&s).ok()),
            after: value,
            ip: None,
        },
    )
    .await?;
    tx.commit().await?;
    Ok(())
}
