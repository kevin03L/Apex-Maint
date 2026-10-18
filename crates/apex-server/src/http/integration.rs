//! Les routes d'ingestion n'utilisent jamais de cookie de session.
use super::{
    auth::{body, ApiError, Body},
    AppState,
};
use apex_core::{
    integration::{self, ServicePrincipal},
    services::Session,
    MaintError,
};
use axum::{
    extract::{ConnectInfo, DefaultBodyLimit, Path, Query, Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    routing::{get, patch, post},
    Extension, Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant},
};
pub type RateLimits = Arc<tokio::sync::Mutex<HashMap<String, (Instant, u32)>>>;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/integrations/supervision/counters",
            post(counters).layer(DefaultBodyLimit::max(1024 * 1024)),
        )
        .route(
            "/integrations/supervision/status",
            post(status).layer(DefaultBodyLimit::max(1024 * 1024)),
        )
        .route("/integration/settings", get(settings).put(save_settings))
        .route("/integration/keys", get(keys).post(create_key))
        .route("/integration/keys/:id", patch(scopes))
        .route("/integration/keys/:id/revoke", post(revoke))
        .route("/integration/events", get(events))
        .route("/integration/statuses", get(statuses))
        .route("/equipments/:id/supervision-status", get(equipment_status))
        .route("/equipments/:id/supervision-history", get(history))
        .route("/equipments/:id/external-ref", patch(equipment_ref))
        .route("/counters/:id/external-ref", patch(counter_ref))
        .route("/integration/supervision-status", get(widget))
}
/// Appelé avant la garde de session/CSRF et exclusivement pour les deux routes documentées.
pub async fn guard(state: &AppState, mut req: Request, next: Next) -> Response {
    let path = req
        .uri()
        .path()
        .strip_prefix("/api")
        .unwrap_or(req.uri().path());
    let scope = match path {
        "/integrations/supervision/counters" => "counters",
        "/integrations/supervision/status" => "status",
        _ => {
            return ApiError::from(MaintError::Unauthorized("Clé de service requise".into()))
                .into_response()
        }
    };
    let token = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    let ip = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(p)| p.ip().to_string())
        .unwrap_or_default();
    let principal = match integration::authenticate(&state.pool, token, scope, &ip).await {
        Ok(p) => p,
        Err(e) => return ApiError::from(e).into_response(),
    };
    let mut rates = state.integration_rates.lock().await;
    rates.retain(|_, (since, _)| since.elapsed() < Duration::from_secs(60));
    let (start, count) = rates
        .entry(principal.key_id.clone())
        .or_insert((Instant::now(), 0));
    if *count >= 120 {
        let retry = 60 - start.elapsed().as_secs().min(59);
        return (
            StatusCode::TOO_MANY_REQUESTS,
            [("retry-after", retry.to_string())],
            Json(json!({"error":true,"message":"Limite de 120 requêtes par minute atteinte"})),
        )
            .into_response();
    }
    *count += 1;
    drop(rates);
    req.extensions_mut().insert(principal);
    next.run(req).await
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Counters {
    readings: Vec<integration::Reading>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Statuses {
    statuses: Vec<integration::StatusInput>,
}
async fn counters(
    State(s): State<AppState>,
    Extension(p): Extension<ServicePrincipal>,
    input: Body<Counters>,
) -> Result<Json<integration::BatchReport>, ApiError> {
    Ok(Json(
        integration::ingest_counters(&s.pool, &p.actor, &p.key_id, body(input)?.readings).await?,
    ))
}
async fn status(
    State(s): State<AppState>,
    Extension(p): Extension<ServicePrincipal>,
    input: Body<Statuses>,
) -> Result<Json<integration::BatchReport>, ApiError> {
    Ok(Json(
        integration::ingest_statuses(&s.pool, &p.actor, &p.key_id, body(input)?.statuses).await?,
    ))
}
async fn settings(
    State(s): State<AppState>,
    Extension(p): Extension<Session>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(integration::settings(&s.pool, &p.actor).await?))
}
async fn save_settings(
    State(s): State<AppState>,
    Extension(p): Extension<Session>,
    input: Body<Value>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(
        integration::save_settings(&s.pool, &p.actor, body(input)?).await?,
    ))
}
#[derive(Deserialize)]
struct PageQuery {
    cursor: Option<String>,
    limit: Option<u32>,
    equipment_id: Option<String>,
    state: Option<String>,
}
async fn keys(
    State(s): State<AppState>,
    Extension(p): Extension<Session>,
    Query(q): Query<PageQuery>,
) -> Result<Json<apex_core::referentiel::Page<integration::IntegrationKey>>, ApiError> {
    Ok(Json(
        integration::keys(
            &s.pool,
            &p.actor,
            q.cursor.as_deref(),
            q.limit.unwrap_or(50),
        )
        .await?,
    ))
}
async fn create_key(
    State(s): State<AppState>,
    Extension(p): Extension<Session>,
    input: Body<integration::KeyInput>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    Ok((
        StatusCode::CREATED,
        Json(integration::create_key(&s.pool, &p.actor, body(input)?).await?),
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Scopes {
    scopes: Vec<String>,
}
async fn scopes(
    State(s): State<AppState>,
    Extension(p): Extension<Session>,
    Path(id): Path<String>,
    input: Body<Scopes>,
) -> Result<Json<integration::IntegrationKey>, ApiError> {
    Ok(Json(
        integration::update_key(&s.pool, &p.actor, &id, Some(body(input)?.scopes), false).await?,
    ))
}
async fn revoke(
    State(s): State<AppState>,
    Extension(p): Extension<Session>,
    Path(id): Path<String>,
) -> Result<Json<integration::IntegrationKey>, ApiError> {
    Ok(Json(
        integration::update_key(&s.pool, &p.actor, &id, None, true).await?,
    ))
}
async fn events(
    State(s): State<AppState>,
    Extension(p): Extension<Session>,
    Query(q): Query<PageQuery>,
) -> Result<Json<apex_core::referentiel::Page<integration::IntegrationEvent>>, ApiError> {
    Ok(Json(
        integration::events(
            &s.pool,
            &p.actor,
            q.cursor.as_deref(),
            q.limit.unwrap_or(50),
        )
        .await?,
    ))
}
async fn statuses(
    State(s): State<AppState>,
    Extension(p): Extension<Session>,
    Query(q): Query<PageQuery>,
) -> Result<Json<apex_core::referentiel::Page<integration::ExternalStatus>>, ApiError> {
    Ok(Json(
        integration::statuses(
            &s.pool,
            &p.actor,
            q.equipment_id.as_deref(),
            q.state.as_deref(),
            q.cursor.as_deref(),
            q.limit.unwrap_or(50),
        )
        .await?,
    ))
}
async fn equipment_status(
    State(s): State<AppState>,
    Extension(p): Extension<Session>,
    Path(id): Path<String>,
) -> Result<Json<Option<integration::ExternalStatus>>, ApiError> {
    let page = integration::statuses(&s.pool, &p.actor, Some(&id), None, None, 1).await?;
    Ok(Json(page.items.into_iter().next()))
}
async fn history(
    State(s): State<AppState>,
    Extension(p): Extension<Session>,
    Path(id): Path<String>,
    Query(q): Query<PageQuery>,
) -> Result<Json<apex_core::referentiel::Page<integration::StatusHistory>>, ApiError> {
    Ok(Json(
        integration::history(
            &s.pool,
            &p.actor,
            &id,
            q.cursor.as_deref(),
            q.limit.unwrap_or(50),
        )
        .await?,
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reference {
    external_ref: Option<String>,
}
async fn equipment_ref(
    State(s): State<AppState>,
    Extension(p): Extension<Session>,
    Path(id): Path<String>,
    input: Body<Reference>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(
        integration::set_external_ref(
            &s.pool,
            &p.actor,
            "equipment",
            &id,
            body(input)?.external_ref,
        )
        .await?,
    ))
}
async fn counter_ref(
    State(s): State<AppState>,
    Extension(p): Extension<Session>,
    Path(id): Path<String>,
    input: Body<Reference>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(
        integration::set_external_ref(&s.pool, &p.actor, "counter", &id, body(input)?.external_ref)
            .await?,
    ))
}
async fn widget(
    State(s): State<AppState>,
    Extension(p): Extension<Session>,
    Query(q): Query<PageQuery>,
) -> Result<Json<apex_core::referentiel::Page<integration::ExternalStatus>>, ApiError> {
    // Le filtrage a lieu avant pagination dans la fonction autonome.
    Ok(Json(
        integration::supervision_status(
            &s.pool,
            &p.actor,
            q.cursor.as_deref(),
            q.limit.unwrap_or(50),
        )
        .await?,
    ))
}
