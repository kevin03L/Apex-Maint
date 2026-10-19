//! Routes HTTP des interventions de maintenance (Phase 3).
//!
//! Expose l'API REST des interventions : création, prise en charge, clôture,
//! annulation, gestion des techniciens, saisie des temps passés, sous-interventions
//! et listes paginées par curseur.

use std::net::SocketAddr;

use apex_core::{
    domain::pagination::Cursor,
    interventions::{self, *},
    services::Session,
};
use axum::{
    extract::{ConnectInfo, Path, Query, State},
    http::StatusCode,
    routing::{get, post},
    Extension, Json, Router,
};
use serde::Deserialize;

use super::{
    errors::{body, Body, Result},
    AppState,
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/interventions",
            get(list_interventions).post(create_intervention),
        )
        .route(
            "/interventions/:id",
            get(get_intervention)
                .patch(update_intervention)
                .delete(delete_intervention),
        )
        .route("/interventions/:id/take", post(take_intervention))
        .route("/interventions/:id/complete", post(complete_intervention))
        .route("/interventions/:id/cancel", post(cancel_intervention))
        .route(
            "/interventions/:id/sub-interventions",
            get(list_sub_interventions).post(create_sub_intervention),
        )
        .route("/interventions/:id/technicians", post(assign_technician))
        .route(
            "/interventions/:id/technicians/:user_id",
            axum::routing::delete(remove_technician),
        )
        .route(
            "/interventions/:id/technicians/:user_id/time",
            axum::routing::put(set_technician_time),
        )
}

#[derive(Debug, Deserialize)]
struct InterventionsQuery {
    q: Option<String>,
    site_id: Option<String>,
    status: Option<String>,
    #[serde(rename = "type")]
    r#type: Option<String>,
    priority: Option<String>,
    machine_state: Option<String>,
    equipment_id: Option<String>,
    assigned_to: Option<String>,
    parent_id: Option<String>,
    date_from: Option<String>,
    date_to: Option<String>,
    cursor: Option<String>,
    limit: Option<u32>,
}

async fn list_interventions(
    State(state): State<AppState>,
    Extension(session): Extension<Session>,
    Query(query): Query<InterventionsQuery>,
) -> Result<Json<apex_core::referentiel::Page<Intervention>>> {
    let cursor = match query.cursor.as_deref().filter(|s| !s.is_empty()) {
        Some(c) => Some(Cursor::decode(c)?),
        None => None,
    };
    let filter = InterventionFilter {
        q: query.q,
        site_id: query.site_id,
        status: query.status,
        r#type: query.r#type,
        priority: query.priority,
        machine_state: query.machine_state,
        equipment_id: query.equipment_id,
        assigned_to: query.assigned_to,
        parent_id: query.parent_id,
        date_from: query.date_from,
        date_to: query.date_to,
        cursor,
        limit: query.limit.unwrap_or(50),
    };
    let page = interventions::list_interventions(&state.pool, &session.actor, filter).await?;
    Ok(Json(page))
}

async fn get_intervention(
    State(state): State<AppState>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
) -> Result<Json<Intervention>> {
    let item = interventions::get_intervention(&state.pool, &session.actor, &id).await?;
    Ok(Json(item))
}

async fn create_intervention(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Extension(session): Extension<Session>,
    input: Body<CreateIntervention>,
) -> Result<(StatusCode, Json<Intervention>)> {
    let input = body(input)?;
    let ip = peer.ip().to_string();
    let item =
        interventions::create_intervention(&state.pool, &session.actor, input, Some(&ip)).await?;
    Ok((StatusCode::CREATED, Json(item)))
}

async fn update_intervention(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
    input: Body<UpdateIntervention>,
) -> Result<Json<Intervention>> {
    let input = body(input)?;
    let ip = peer.ip().to_string();
    let item =
        interventions::update_intervention(&state.pool, &session.actor, &id, input, Some(&ip))
            .await?;
    Ok(Json(item))
}

async fn delete_intervention(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    let ip = peer.ip().to_string();
    interventions::delete_intervention(&state.pool, &session.actor, &id, Some(&ip)).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn take_intervention(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
    input: Option<Json<TakeIntervention>>,
) -> Result<Json<Intervention>> {
    let input = input.map(|Json(v)| v).unwrap_or_default();
    let ip = peer.ip().to_string();
    let item = interventions::take_intervention(&state.pool, &session.actor, &id, input, Some(&ip))
        .await?;
    Ok(Json(item))
}

async fn complete_intervention(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
    input: Body<CompleteIntervention>,
) -> Result<Json<Intervention>> {
    let input = body(input)?;
    let ip = peer.ip().to_string();
    let item =
        interventions::complete_intervention(&state.pool, &session.actor, &id, input, Some(&ip))
            .await?;
    Ok(Json(item))
}

async fn cancel_intervention(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
    input: Body<CancelIntervention>,
) -> Result<Json<Intervention>> {
    let input = body(input)?;
    let ip = peer.ip().to_string();
    let item =
        interventions::cancel_intervention(&state.pool, &session.actor, &id, input, Some(&ip))
            .await?;
    Ok(Json(item))
}

async fn list_sub_interventions(
    State(state): State<AppState>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
) -> Result<Json<Vec<Intervention>>> {
    let items = interventions::list_sub_interventions(&state.pool, &session.actor, &id).await?;
    Ok(Json(items))
}

async fn create_sub_intervention(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
    input: Body<CreateIntervention>,
) -> Result<(StatusCode, Json<Intervention>)> {
    let input = body(input)?;
    let ip = peer.ip().to_string();
    let item =
        interventions::create_sub_intervention(&state.pool, &session.actor, &id, input, Some(&ip))
            .await?;
    Ok((StatusCode::CREATED, Json(item)))
}

async fn assign_technician(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
    input: Body<AssignTechnician>,
) -> Result<Json<AssignedTechnician>> {
    let input = body(input)?;
    let ip = peer.ip().to_string();
    let item = interventions::assign_technician(&state.pool, &session.actor, &id, input, Some(&ip))
        .await?;
    Ok(Json(item))
}

async fn remove_technician(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Extension(session): Extension<Session>,
    Path((id, user_id)): Path<(String, String)>,
) -> Result<StatusCode> {
    let ip = peer.ip().to_string();
    interventions::remove_technician(&state.pool, &session.actor, &id, &user_id, Some(&ip)).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn set_technician_time(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Extension(session): Extension<Session>,
    Path((id, user_id)): Path<(String, String)>,
    input: Body<SetTechnicianTime>,
) -> Result<Json<AssignedTechnician>> {
    let input = body(input)?;
    let ip = peer.ip().to_string();
    let item = interventions::set_technician_time(
        &state.pool,
        &session.actor,
        &id,
        &user_id,
        input,
        Some(&ip),
    )
    .await?;
    Ok(Json(item))
}
