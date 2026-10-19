//! Routes HTTP de maintenance préventive (Phase 5).
//!
//! Expose l'API REST des compteurs d'équipements, relevés manuels, plannings
//! de maintenance, occurrences et vue planning projetée.

use std::net::SocketAddr;

use apex_core::{
    domain::pagination::Cursor,
    maintenance::{self, *},
    referentiel::Page,
    services::Session,
};
use axum::{
    extract::{ConnectInfo, Path, Query, State},
    http::StatusCode,
    routing::{get, patch, post},
    Extension, Json, Router,
};
use chrono::{Duration, Local};
use serde::{Deserialize, Serialize};

use super::{
    errors::{body, Body, Result},
    AppState,
};

pub fn routes() -> Router<AppState> {
    Router::new()
        // Compteurs d'équipement
        .route(
            "/equipments/:id/counters",
            get(list_counters).post(create_counter),
        )
        .route(
            "/counters/:id",
            patch(update_counter).delete(delete_counter),
        )
        // Relevés de compteur
        .route(
            "/counters/:id/readings",
            get(list_readings).post(record_reading),
        )
        // Plannings de maintenance
        .route("/maintenance/plans", get(list_plans).post(create_plan))
        .route(
            "/maintenance/plans/:id",
            get(get_plan).patch(update_plan).delete(delete_plan),
        )
        .route("/maintenance/plans/:id/generate", post(generate_plan))
        .route("/maintenance/plans/:id/skip", post(skip_plan))
        .route("/maintenance/plans/:id/occurrences", get(list_occurrences))
        // Vue planning
        .route("/maintenance/schedule", get(get_schedule))
}

// ============================================================================
// 1. COMPTEURS D'ÉQUIPEMENT
// ============================================================================

async fn list_counters(
    State(state): State<AppState>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
) -> Result<Json<Vec<EquipmentCounter>>> {
    let list = maintenance::list_equipment_counters(&state.pool, &session.actor, &id).await?;
    Ok(Json(list))
}

async fn create_counter(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
    input: Body<CreateCounter>,
) -> Result<(StatusCode, Json<EquipmentCounter>)> {
    let ip = peer.ip().to_string();
    let data = body(input)?;
    let created =
        maintenance::create_equipment_counter(&state.pool, &session.actor, &id, data, Some(&ip))
            .await?;
    Ok((StatusCode::CREATED, Json(created)))
}

async fn update_counter(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
    input: Body<UpdateCounter>,
) -> Result<Json<EquipmentCounter>> {
    let ip = peer.ip().to_string();
    let data = body(input)?;
    let updated =
        maintenance::update_counter(&state.pool, &session.actor, &id, data, Some(&ip)).await?;
    Ok(Json(updated))
}

async fn delete_counter(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    let ip = peer.ip().to_string();
    maintenance::delete_counter(&state.pool, &session.actor, &id, Some(&ip)).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ============================================================================
// 2. RELEVÉS DE COMPTEUR
// ============================================================================

#[derive(Debug, Deserialize)]
struct PaginationQuery {
    cursor: Option<String>,
    limit: Option<u32>,
}

async fn list_readings(
    State(state): State<AppState>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
    Query(pq): Query<PaginationQuery>,
) -> Result<Json<Page<CounterReading>>> {
    let cursor = match pq.cursor.as_deref() {
        Some(c) => Some(Cursor::decode(c)?),
        None => None,
    };
    let limit = pq.limit.unwrap_or(20);
    let page =
        maintenance::list_counter_readings(&state.pool, &session.actor, &id, cursor, limit).await?;
    Ok(Json(page))
}

async fn record_reading(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
    input: Body<CreateCounterReading>,
) -> Result<(StatusCode, Json<CounterReading>)> {
    let ip = peer.ip().to_string();
    let data = body(input)?;
    let created =
        maintenance::record_counter_reading(&state.pool, &session.actor, &id, data, Some(&ip))
            .await?;
    Ok((StatusCode::CREATED, Json(created)))
}

// ============================================================================
// 3. PLANNINGS DE MAINTENANCE
// ============================================================================

#[derive(Debug, Deserialize)]
struct ListPlansQuery {
    q: Option<String>,
    site_id: Option<String>,
    equipment_id: Option<String>,
    frequency: Option<String>,
    active: Option<bool>,
    due: Option<String>,
    cursor: Option<String>,
    limit: Option<u32>,
}

async fn list_plans(
    State(state): State<AppState>,
    Extension(session): Extension<Session>,
    Query(query): Query<ListPlansQuery>,
) -> Result<Json<Page<MaintenancePlan>>> {
    let cursor = match query.cursor.as_deref() {
        Some(c) => Some(Cursor::decode(c)?),
        None => None,
    };
    let limit = query.limit.unwrap_or(20);

    let filter = PlanFilter {
        q: query.q,
        site_id: query.site_id,
        equipment_id: query.equipment_id,
        frequency: query.frequency,
        active: query.active,
        due: query.due,
        cursor,
        limit,
    };

    let page = maintenance::list_plans(&state.pool, &session.actor, filter).await?;
    Ok(Json(page))
}

async fn get_plan(
    State(state): State<AppState>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
) -> Result<Json<MaintenancePlan>> {
    let plan = maintenance::get_plan(&state.pool, &session.actor, &id).await?;
    Ok(Json(plan))
}

async fn create_plan(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Extension(session): Extension<Session>,
    input: Body<CreateMaintenancePlan>,
) -> Result<(StatusCode, Json<MaintenancePlan>)> {
    let ip = peer.ip().to_string();
    let data = body(input)?;
    let created = maintenance::create_plan(&state.pool, &session.actor, data, Some(&ip)).await?;
    Ok((StatusCode::CREATED, Json(created)))
}

async fn update_plan(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
    input: Body<UpdateMaintenancePlan>,
) -> Result<Json<MaintenancePlan>> {
    let ip = peer.ip().to_string();
    let data = body(input)?;
    let updated =
        maintenance::update_plan(&state.pool, &session.actor, &id, data, Some(&ip)).await?;
    Ok(Json(updated))
}

async fn delete_plan(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    let ip = peer.ip().to_string();
    maintenance::delete_plan(&state.pool, &session.actor, &id, Some(&ip)).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Serialize)]
struct GeneratePlanResponse {
    occurrence: MaintenanceOccurrence,
    intervention: apex_core::interventions::Intervention,
}

async fn generate_plan(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
) -> Result<(StatusCode, Json<GeneratePlanResponse>)> {
    let ip = peer.ip().to_string();
    let (occurrence, intervention) =
        maintenance::generate_plan_occurrence(&state.pool, &session.actor, &id, Some(&ip)).await?;
    Ok((
        StatusCode::CREATED,
        Json(GeneratePlanResponse {
            occurrence,
            intervention,
        }),
    ))
}

async fn skip_plan(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
    input: Body<SkipPlanBody>,
) -> Result<Json<MaintenancePlan>> {
    let ip = peer.ip().to_string();
    let data = body(input)?;
    let updated = maintenance::skip_plan_occurrence(
        &state.pool,
        &session.actor,
        &id,
        &data.reason,
        Some(&ip),
    )
    .await?;
    Ok(Json(updated))
}

async fn list_occurrences(
    State(state): State<AppState>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
    Query(pq): Query<PaginationQuery>,
) -> Result<Json<Page<MaintenanceOccurrence>>> {
    let cursor = match pq.cursor.as_deref() {
        Some(c) => Some(Cursor::decode(c)?),
        None => None,
    };
    let limit = pq.limit.unwrap_or(20);
    let page =
        maintenance::list_plan_occurrences(&state.pool, &session.actor, &id, cursor, limit).await?;
    Ok(Json(page))
}

// ============================================================================
// 4. VUE PLANNING PROJETÉE
// ============================================================================

#[derive(Debug, Deserialize)]
struct ScheduleQuery {
    from: Option<String>,
    to: Option<String>,
    site_id: Option<String>,
    equipment_id: Option<String>,
}

async fn get_schedule(
    State(state): State<AppState>,
    Extension(session): Extension<Session>,
    Query(q): Query<ScheduleQuery>,
) -> Result<Json<Vec<ScheduleEntry>>> {
    let today = Local::now().date_naive();
    let from_default = today.format("%Y-%m-%d").to_string();
    let to_default = (today + Duration::days(30)).format("%Y-%m-%d").to_string();

    let from = q.from.as_deref().unwrap_or(&from_default);
    let to = q.to.as_deref().unwrap_or(&to_default);

    let entries = maintenance::get_schedule(
        &state.pool,
        &session.actor,
        from,
        to,
        q.site_id.as_deref(),
        q.equipment_id.as_deref(),
    )
    .await?;

    Ok(Json(entries))
}
