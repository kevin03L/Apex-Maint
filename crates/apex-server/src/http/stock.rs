//! Routes HTTP du stock et des pièces détachées (Phase 4).
//!
//! Expose l'API REST pour :
//! - le catalogue de pièces détachées (CRUD, filtres et pagination par curseur)
//! - la consultation des niveaux de stock par emplacement
//! - les alertes dynamiques de stock bas
//! - l'enregistrement des mouvements (entrées avec PMP, transferts, ajustements d'inventaire)
//! - la consommation et l'annulation de pièces sur les interventions
//! - la préparation, consommation et libération des réservations de pièces
//! - la nomenclature et les pièces préconisées sur les équipements
//!
//! Les deux consommations acceptent Idempotency-Key via le middleware terrain commun ;
//! le dépôt stock enregistre la réponse avant le commit du mouvement et de l'audit.

use std::net::SocketAddr;

use apex_core::{
    domain::pagination::Cursor,
    services::Session,
    stock::{self, *},
};
use axum::{
    extract::{ConnectInfo, Path, Query, State},
    http::StatusCode,
    routing::{delete, get, post},
    Extension, Json, Router,
};
use serde::Deserialize;

use super::{
    errors::{body, Body, Result},
    AppState,
};

pub fn routes() -> Router<AppState> {
    Router::new()
        // Catalogue des pièces
        .route("/parts", get(list_parts).post(create_part))
        .route(
            "/parts/:id",
            get(get_part).patch(update_part).delete(delete_part),
        )
        // Niveaux de stock et alertes
        .route("/stock", get(list_stock))
        .route("/stock/alerts", get(list_stock_alerts))
        .route(
            "/stock/movements",
            get(list_stock_movements).post(create_stock_movement),
        )
        .route("/stock/adjustments", post(create_stock_adjustment))
        // Consommation et réservations sur les interventions
        .route(
            "/interventions/:id/parts",
            get(list_intervention_parts).post(consume_part_on_intervention),
        )
        .route(
            "/interventions/:id/parts/:item_id",
            delete(delete_intervention_part),
        )
        .route(
            "/interventions/:id/reservations",
            get(list_intervention_reservations).post(create_stock_reservation),
        )
        .route("/reservations/:id/consume", post(consume_stock_reservation))
        .route("/reservations/:id/release", post(release_stock_reservation))
        // Pièces associées aux équipements
        .route(
            "/equipments/:id/parts",
            get(list_equipment_parts).post(associate_equipment_part),
        )
        .route(
            "/equipments/:id/parts/:part_id",
            delete(disassociate_equipment_part),
        )
}

// ============================================================================
// 1. CATALOGUE DES PIÈCES
// ============================================================================

#[derive(Debug, Deserialize)]
struct PartsQuery {
    q: Option<String>,
    site_id: Option<String>,
    category: Option<String>,
    supplier_id: Option<String>,
    low_stock: Option<bool>,
    cursor: Option<String>,
    limit: Option<u32>,
}

async fn list_parts(
    State(state): State<AppState>,
    Extension(session): Extension<Session>,
    Query(query): Query<PartsQuery>,
) -> Result<Json<apex_core::referentiel::Page<PartDetail>>> {
    let cursor = match query.cursor.as_deref().filter(|s| !s.is_empty()) {
        Some(c) => Some(Cursor::decode(c)?),
        None => None,
    };
    let filter = PartFilter {
        q: query.q,
        site_id: query.site_id,
        category: query.category,
        supplier_id: query.supplier_id,
        low_stock: query.low_stock,
        cursor,
        limit: query.limit.unwrap_or(50),
    };
    let page = stock::list_parts(&state.pool, &session.actor, filter).await?;
    Ok(Json(page))
}

async fn get_part(
    State(state): State<AppState>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
) -> Result<Json<PartDetail>> {
    let item = stock::get_part(&state.pool, &session.actor, &id).await?;
    Ok(Json(item))
}

async fn create_part(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Extension(session): Extension<Session>,
    input: Body<CreatePart>,
) -> Result<(StatusCode, Json<PartDetail>)> {
    let input = body(input)?;
    let ip = peer.ip().to_string();
    let item = stock::create_part(&state.pool, &session.actor, input, Some(&ip)).await?;
    Ok((StatusCode::CREATED, Json(item)))
}

async fn update_part(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
    input: Body<UpdatePart>,
) -> Result<Json<PartDetail>> {
    let input = body(input)?;
    let ip = peer.ip().to_string();
    let item = stock::update_part(&state.pool, &session.actor, &id, input, Some(&ip)).await?;
    Ok(Json(item))
}

async fn delete_part(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    let ip = peer.ip().to_string();
    stock::delete_part(&state.pool, &session.actor, &id, Some(&ip)).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ============================================================================
// 2. ÉTAT DES STOCKS ET ALERTES
// ============================================================================

#[derive(Debug, Deserialize)]
struct StockQuery {
    site_id: Option<String>,
    part_id: Option<String>,
    location_id: Option<String>,
}

async fn list_stock(
    State(state): State<AppState>,
    Extension(session): Extension<Session>,
    Query(query): Query<StockQuery>,
) -> Result<Json<Vec<StockItem>>> {
    let filter = StockFilter {
        site_id: query.site_id,
        part_id: query.part_id,
        location_id: query.location_id,
    };
    let items = stock::list_stock(&state.pool, &session.actor, filter).await?;
    Ok(Json(items))
}

#[derive(Debug, Deserialize)]
struct AlertsQuery {
    site_id: Option<String>,
}

async fn list_stock_alerts(
    State(state): State<AppState>,
    Extension(session): Extension<Session>,
    Query(query): Query<AlertsQuery>,
) -> Result<Json<Vec<PartDetail>>> {
    let items =
        stock::list_stock_alerts(&state.pool, &session.actor, query.site_id.as_deref()).await?;
    Ok(Json(items))
}

// ============================================================================
// 3. MOUVEMENTS ET AJUSTEMENTS
// ============================================================================

#[derive(Debug, Deserialize)]
struct MovementsQuery {
    q: Option<String>,
    site_id: Option<String>,
    part_id: Option<String>,
    movement_type: Option<String>,
    intervention_id: Option<String>,
    location_id: Option<String>,
    user_id: Option<String>,
    date_from: Option<String>,
    date_to: Option<String>,
    cursor: Option<String>,
    limit: Option<u32>,
}

async fn list_stock_movements(
    State(state): State<AppState>,
    Extension(session): Extension<Session>,
    Query(query): Query<MovementsQuery>,
) -> Result<Json<apex_core::referentiel::Page<StockMovement>>> {
    let cursor = match query.cursor.as_deref().filter(|s| !s.is_empty()) {
        Some(c) => Some(Cursor::decode(c)?),
        None => None,
    };
    let filter = StockMovementFilter {
        q: query.q,
        site_id: query.site_id,
        part_id: query.part_id,
        movement_type: query.movement_type,
        intervention_id: query.intervention_id,
        location_id: query.location_id,
        user_id: query.user_id,
        date_from: query.date_from,
        date_to: query.date_to,
        cursor,
        limit: query.limit.unwrap_or(50),
    };
    let page = stock::list_stock_movements(&state.pool, &session.actor, filter).await?;
    Ok(Json(page))
}

async fn create_stock_movement(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Extension(session): Extension<Session>,
    input: Body<CreateStockMovement>,
) -> Result<(StatusCode, Json<StockMovement>)> {
    let input = body(input)?;
    let ip = peer.ip().to_string();
    let item = stock::create_stock_movement(&state.pool, &session.actor, input, Some(&ip)).await?;
    Ok((StatusCode::CREATED, Json(item)))
}

async fn create_stock_adjustment(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Extension(session): Extension<Session>,
    input: Body<CreateAdjustment>,
) -> Result<(StatusCode, Json<StockMovement>)> {
    let input = body(input)?;
    let ip = peer.ip().to_string();
    let item =
        stock::create_stock_adjustment(&state.pool, &session.actor, input, Some(&ip)).await?;
    Ok((StatusCode::CREATED, Json(item)))
}

// ============================================================================
// 4. PIÈCES CONSOMMÉES SUR INTERVENTION
// ============================================================================

async fn list_intervention_parts(
    State(state): State<AppState>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
) -> Result<Json<Vec<InterventionPart>>> {
    let items = stock::list_intervention_parts(&state.pool, &session.actor, &id).await?;
    Ok(Json(items))
}

async fn consume_part_on_intervention(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
    input: Body<ConsumePartInput>,
) -> Result<(StatusCode, Json<InterventionPart>)> {
    let input = body(input)?;
    let ip = peer.ip().to_string();
    let item =
        stock::consume_part_on_intervention(&state.pool, &session.actor, &id, input, Some(&ip))
            .await?;
    Ok((StatusCode::CREATED, Json(item)))
}

async fn delete_intervention_part(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Extension(session): Extension<Session>,
    Path((id, item_id)): Path<(String, String)>,
) -> Result<StatusCode> {
    let ip = peer.ip().to_string();
    stock::delete_intervention_part(&state.pool, &session.actor, &id, &item_id, Some(&ip)).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ============================================================================
// 5. RÉSERVATIONS DE PIÈCES PRÉPARÉES (§7.3)
// ============================================================================

async fn list_intervention_reservations(
    State(state): State<AppState>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
) -> Result<Json<Vec<StockReservation>>> {
    let items = stock::list_intervention_reservations(&state.pool, &session.actor, &id).await?;
    Ok(Json(items))
}

async fn create_stock_reservation(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
    input: Body<CreateReservation>,
) -> Result<(StatusCode, Json<StockReservation>)> {
    let input = body(input)?;
    let ip = peer.ip().to_string();
    let item =
        stock::create_stock_reservation(&state.pool, &session.actor, &id, input, Some(&ip)).await?;
    Ok((StatusCode::CREATED, Json(item)))
}

async fn consume_stock_reservation(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
    input: axum::extract::Request,
) -> Result<Json<InterventionPart>> {
    let input = super::errors::optional_body::<ConsumeReservationInput>(input)
        .await?
        .unwrap_or(ConsumeReservationInput { quantity: None });
    let ip = peer.ip().to_string();
    let item = stock::consume_stock_reservation(&state.pool, &session.actor, &id, input, Some(&ip))
        .await?;
    Ok(Json(item))
}

async fn release_stock_reservation(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
) -> Result<Json<StockReservation>> {
    let ip = peer.ip().to_string();
    let item =
        stock::release_stock_reservation(&state.pool, &session.actor, &id, Some(&ip)).await?;
    Ok(Json(item))
}

// ============================================================================
// 6. PIÈCES ASSOCIÉES AUX ÉQUIPEMENTS
// ============================================================================

async fn list_equipment_parts(
    State(state): State<AppState>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
) -> Result<Json<Vec<EquipmentPart>>> {
    let items = stock::list_equipment_parts(&state.pool, &session.actor, &id).await?;
    Ok(Json(items))
}

async fn associate_equipment_part(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
    input: Body<AssociateEquipmentPart>,
) -> Result<(StatusCode, Json<EquipmentPart>)> {
    let input = body(input)?;
    let ip = peer.ip().to_string();
    let item =
        stock::associate_equipment_part(&state.pool, &session.actor, &id, input, Some(&ip)).await?;
    Ok((StatusCode::CREATED, Json(item)))
}

async fn disassociate_equipment_part(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Extension(session): Extension<Session>,
    Path((id, part_id)): Path<(String, String)>,
) -> Result<StatusCode> {
    let ip = peer.ip().to_string();
    stock::disassociate_equipment_part(&state.pool, &session.actor, &id, &part_id, Some(&ip))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
