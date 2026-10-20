//! Routes HTTP du référentiel technique (lignes de production, équipements, emplacements, fournisseurs).
//!
//! Conforme à la spécification de la phase 2 : listes filtrables et paginées par curseur,
//! arborescences complètes, mise à jour partielle PATCH avec null explicite pour vider.

use std::net::SocketAddr;

use apex_core::{
    domain::pagination::Cursor,
    referentiel::{self, *},
    services::Session,
};
use axum::{
    extract::{ConnectInfo, Path, Query, State},
    http::StatusCode,
    routing::get,
    Extension, Json, Router,
};
use serde::Deserialize;

use super::{
    errors::{body, Body, Result},
    AppState,
};

pub fn routes() -> Router<AppState> {
    Router::new()
        // Lignes de production
        .route(
            "/production-lines",
            get(list_production_lines).post(create_production_line),
        )
        .route(
            "/production-lines/:id",
            get(get_production_line)
                .patch(update_production_line)
                .delete(delete_production_line),
        )
        // Équipements (noter l'arborescence avant la route paramétrée :id)
        .route("/equipments/tree", get(get_equipments_tree))
        .route("/equipments", get(list_equipments).post(create_equipment))
        .route(
            "/equipments/:id",
            get(get_equipment)
                .patch(update_equipment)
                .delete(delete_equipment),
        )
        // Emplacements (arborescence avant :id)
        .route("/locations/tree", get(get_locations_tree))
        .route("/locations", get(list_locations).post(create_location))
        .route(
            "/locations/:id",
            get(get_location)
                .patch(update_location)
                .delete(delete_location),
        )
        // Fournisseurs
        .route("/suppliers", get(list_suppliers).post(create_supplier))
        .route(
            "/suppliers/:id",
            get(get_supplier)
                .patch(update_supplier)
                .delete(delete_supplier),
        )
}

// ============================================================================
// 1. LIGNES DE PRODUCTION
// ============================================================================

#[derive(Debug, Deserialize)]
struct ProductionLinesQuery {
    q: Option<String>,
    site_id: Option<String>,
    cursor: Option<String>,
    limit: Option<u32>,
}

async fn list_production_lines(
    State(state): State<AppState>,
    Extension(session): Extension<Session>,
    Query(query): Query<ProductionLinesQuery>,
) -> Result<Json<Page<ProductionLine>>> {
    let cursor = match query.cursor.as_deref().filter(|s| !s.is_empty()) {
        Some(c) => Some(Cursor::decode(c)?),
        None => None,
    };
    let filter = ProductionLineFilter {
        q: query.q,
        site_id: query.site_id,
        cursor,
        limit: query.limit.unwrap_or(50),
    };
    let page = referentiel::list_production_lines(&state.pool, &session.actor, filter).await?;
    Ok(Json(page))
}

async fn get_production_line(
    State(state): State<AppState>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
) -> Result<Json<ProductionLine>> {
    let line = referentiel::get_production_line(&state.pool, &session.actor, &id).await?;
    Ok(Json(line))
}

async fn create_production_line(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Extension(session): Extension<Session>,
    input: Body<CreateProductionLine>,
) -> Result<(StatusCode, Json<ProductionLine>)> {
    let input = body(input)?;
    let ip = peer.ip().to_string();
    let line =
        referentiel::create_production_line(&state.pool, &session.actor, input, Some(&ip)).await?;
    Ok((StatusCode::CREATED, Json(line)))
}

async fn update_production_line(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
    input: Body<UpdateProductionLine>,
) -> Result<Json<ProductionLine>> {
    let input = body(input)?;
    let ip = peer.ip().to_string();
    let line =
        referentiel::update_production_line(&state.pool, &session.actor, &id, input, Some(&ip))
            .await?;
    Ok(Json(line))
}

async fn delete_production_line(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    let ip = peer.ip().to_string();
    referentiel::delete_production_line(&state.pool, &session.actor, &id, Some(&ip)).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ============================================================================
// 2. ÉQUIPEMENTS
// ============================================================================

#[derive(Debug, Deserialize)]
struct EquipmentsQuery {
    q: Option<String>,
    site_id: Option<String>,
    status: Option<String>,
    production_line_id: Option<String>,
    parent_id: Option<String>,
    cursor: Option<String>,
    limit: Option<u32>,
}

async fn list_equipments(
    State(state): State<AppState>,
    Extension(session): Extension<Session>,
    Query(query): Query<EquipmentsQuery>,
) -> Result<Json<Page<Equipment>>> {
    let cursor = match query.cursor.as_deref().filter(|s| !s.is_empty()) {
        Some(c) => Some(Cursor::decode(c)?),
        None => None,
    };
    let filter = EquipmentFilter {
        q: query.q,
        site_id: query.site_id,
        status: query.status,
        production_line_id: query.production_line_id,
        parent_id: query.parent_id,
        cursor,
        limit: query.limit.unwrap_or(50),
    };
    let page = referentiel::list_equipments(&state.pool, &session.actor, filter).await?;
    Ok(Json(page))
}

#[derive(Debug, Deserialize)]
struct TreeQuery {
    site_id: Option<String>,
}

async fn get_equipments_tree(
    State(state): State<AppState>,
    Extension(session): Extension<Session>,
    Query(query): Query<TreeQuery>,
) -> Result<Json<Vec<EquipmentTreeNode>>> {
    let tree =
        referentiel::get_equipments_tree(&state.pool, &session.actor, query.site_id.as_deref())
            .await?;
    Ok(Json(tree))
}

async fn get_equipment(
    State(state): State<AppState>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
) -> Result<Json<Equipment>> {
    let eq = referentiel::get_equipment(&state.pool, &session.actor, &id).await?;
    Ok(Json(eq))
}

async fn create_equipment(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Extension(session): Extension<Session>,
    input: Body<CreateEquipment>,
) -> Result<(StatusCode, Json<Equipment>)> {
    let input = body(input)?;
    let ip = peer.ip().to_string();
    let eq = referentiel::create_equipment(&state.pool, &session.actor, input, Some(&ip)).await?;
    Ok((StatusCode::CREATED, Json(eq)))
}

async fn update_equipment(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
    input: Body<UpdateEquipment>,
) -> Result<Json<Equipment>> {
    let input = body(input)?;
    let ip = peer.ip().to_string();
    let eq =
        referentiel::update_equipment(&state.pool, &session.actor, &id, input, Some(&ip)).await?;
    Ok(Json(eq))
}

async fn delete_equipment(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    let ip = peer.ip().to_string();
    referentiel::delete_equipment(&state.pool, &session.actor, &id, Some(&ip)).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ============================================================================
// 3. EMPLACEMENTS
// ============================================================================

#[derive(Debug, Deserialize)]
struct LocationsQuery {
    q: Option<String>,
    site_id: Option<String>,
    kind: Option<String>,
    parent_id: Option<String>,
    cursor: Option<String>,
    limit: Option<u32>,
}

async fn list_locations(
    State(state): State<AppState>,
    Extension(session): Extension<Session>,
    Query(query): Query<LocationsQuery>,
) -> Result<Json<Page<Location>>> {
    let cursor = match query.cursor.as_deref().filter(|s| !s.is_empty()) {
        Some(c) => Some(Cursor::decode(c)?),
        None => None,
    };
    let filter = LocationFilter {
        q: query.q,
        site_id: query.site_id,
        kind: query.kind,
        parent_id: query.parent_id,
        cursor,
        limit: query.limit.unwrap_or(50),
    };
    let page = referentiel::list_locations(&state.pool, &session.actor, filter).await?;
    Ok(Json(page))
}

async fn get_locations_tree(
    State(state): State<AppState>,
    Extension(session): Extension<Session>,
    Query(query): Query<TreeQuery>,
) -> Result<Json<Vec<LocationTreeNode>>> {
    let tree =
        referentiel::get_locations_tree(&state.pool, &session.actor, query.site_id.as_deref())
            .await?;
    Ok(Json(tree))
}

async fn get_location(
    State(state): State<AppState>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
) -> Result<Json<Location>> {
    let loc = referentiel::get_location(&state.pool, &session.actor, &id).await?;
    Ok(Json(loc))
}

async fn create_location(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Extension(session): Extension<Session>,
    input: Body<CreateLocation>,
) -> Result<(StatusCode, Json<Location>)> {
    let input = body(input)?;
    let ip = peer.ip().to_string();
    let loc = referentiel::create_location(&state.pool, &session.actor, input, Some(&ip)).await?;
    Ok((StatusCode::CREATED, Json(loc)))
}

async fn update_location(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
    input: Body<UpdateLocation>,
) -> Result<Json<Location>> {
    let input = body(input)?;
    let ip = peer.ip().to_string();
    let loc =
        referentiel::update_location(&state.pool, &session.actor, &id, input, Some(&ip)).await?;
    Ok(Json(loc))
}

async fn delete_location(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    let ip = peer.ip().to_string();
    referentiel::delete_location(&state.pool, &session.actor, &id, Some(&ip)).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ============================================================================
// 4. FOURNISSEURS
// ============================================================================

#[derive(Debug, Deserialize)]
struct SuppliersQuery {
    q: Option<String>,
    site_id: Option<String>,
    cursor: Option<String>,
    limit: Option<u32>,
}

async fn list_suppliers(
    State(state): State<AppState>,
    Extension(session): Extension<Session>,
    Query(query): Query<SuppliersQuery>,
) -> Result<Json<Page<Supplier>>> {
    let cursor = match query.cursor.as_deref().filter(|s| !s.is_empty()) {
        Some(c) => Some(Cursor::decode(c)?),
        None => None,
    };
    let filter = SupplierFilter {
        q: query.q,
        site_id: query.site_id,
        cursor,
        limit: query.limit.unwrap_or(50),
    };
    let page = referentiel::list_suppliers(&state.pool, &session.actor, filter).await?;
    Ok(Json(page))
}

async fn get_supplier(
    State(state): State<AppState>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
) -> Result<Json<Supplier>> {
    let supplier = referentiel::get_supplier(&state.pool, &session.actor, &id).await?;
    Ok(Json(supplier))
}

async fn create_supplier(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Extension(session): Extension<Session>,
    input: Body<CreateSupplier>,
) -> Result<(StatusCode, Json<Supplier>)> {
    let input = body(input)?;
    let ip = peer.ip().to_string();
    let supplier =
        referentiel::create_supplier(&state.pool, &session.actor, input, Some(&ip)).await?;
    Ok((StatusCode::CREATED, Json(supplier)))
}

async fn update_supplier(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
    input: Body<UpdateSupplier>,
) -> Result<Json<Supplier>> {
    let input = body(input)?;
    let ip = peer.ip().to_string();
    let supplier =
        referentiel::update_supplier(&state.pool, &session.actor, &id, input, Some(&ip)).await?;
    Ok(Json(supplier))
}

async fn delete_supplier(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    let ip = peer.ip().to_string();
    referentiel::delete_supplier(&state.pool, &session.actor, &id, Some(&ip)).await?;
    Ok(StatusCode::NO_CONTENT)
}
