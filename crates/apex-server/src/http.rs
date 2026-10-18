//! Serveur web Axum : routes de santé, contrôle système, en-têtes de sécurité et fichiers statiques.

use std::{net::SocketAddr, path::Path as FsPath, sync::Arc, time::Duration};

use axum::{
    extract::{ConnectInfo, Request, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use tower_http::services::ServeDir;

mod auth;

/// En-têtes ajoutés par un proxy inverse (nginx, Caddy, Apache, IIS/ARR...).
const PROXY_HEADERS: &[&str] = &[
    "forwarded",
    "x-forwarded-for",
    "x-forwarded-host",
    "x-forwarded-proto",
    "x-real-ip",
];

/// Informations d'exécution publiées pour l'utilitaire desktop (avec jeton local).
#[derive(Debug, Clone, serde::Serialize)]
pub struct RuntimeInfo {
    pub instance_name: String,
    pub config_fingerprint: String,
    pub config_file: String,
    pub data_dir: String,
    pub log_dir: String,
    pub web_dir: String,
    pub run_mode: String,
    pub mode: String,
    pub pid: u32,
    pub started_at: String,
    pub full_version: String,
}

#[derive(Clone)]
pub struct AppState {
    pub pool: sqlx::SqlitePool,
    pub auth: apex_core::services::Auth,
    pub secure_cookie: bool,
    pub runtime: Arc<RuntimeInfo>,
    pub control_token: String,
    pub shutdown_notify: Arc<tokio::sync::Notify>,
    pub started_at: DateTime<Utc>,
}

/// Détermine si la requête provient d'un pair direct sur la boucle locale sans proxy inverse.
pub fn is_direct_local_peer(peer: &SocketAddr, headers: &HeaderMap) -> bool {
    peer.ip().to_canonical().is_loopback()
        && !PROXY_HEADERS.iter().any(|h| headers.contains_key(*h))
}

/// Extrait le jeton d'authentification depuis les en-têtes (X-Maint-Token ou Authorization).
pub fn extract_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-maint-token")
        .or_else(|| headers.get(header::AUTHORIZATION))
        .and_then(|v| v.to_str().ok())
        .map(|s| {
            if let Some(t) = s.strip_prefix("Bearer ") {
                t.trim().to_string()
            } else {
                s.trim().to_string()
            }
        })
}

/// Vérifie si la requête présente le jeton de contrôle local depuis la boucle locale directe.
pub fn is_valid_control_token(headers: &HeaderMap, peer: &SocketAddr, expected: &str) -> bool {
    if !is_direct_local_peer(peer, headers) {
        return false;
    }
    match extract_token(headers) {
        Some(t) => t == expected,
        None => false,
    }
}

/// Construit le routeur HTTP complet.
pub fn router(state: AppState, web_dir: &FsPath) -> Router {
    let api_router = Router::new()
        .merge(auth::routes())
        .route("/health", get(api_health))
        .route("/system/status", get(api_system_status))
        .route("/system/shutdown", post(api_system_shutdown))
        .fallback(api_fallback_404)
        .layer(axum::extract::DefaultBodyLimit::max(64 * 1024))
        .layer(middleware::from_fn_with_state(state.clone(), auth::guard));

    let static_service =
        ServeDir::new(web_dir).fallback(axum::routing::get(minimal_fallback_handler));

    Router::new()
        .nest("/api", api_router)
        .fallback_service(static_service)
        .layer(middleware::from_fn(security_headers_middleware))
        .with_state(state)
}

async fn minimal_fallback_handler() -> (
    StatusCode,
    [(header::HeaderName, &'static str); 1],
    &'static str,
) {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        "<!DOCTYPE html><html lang=\"fr\"><head><meta charset=\"utf-8\"><title>Apex Maint</title></head><body><h1>Apex Maint</h1><p>IHM web statique indisponible. Renseignez paths.web_dir dans la configuration.</p></body></html>",
    )
}

/// En-têtes de sécurité : CSP, nosniff, DENY, no-referrer, et Cache-Control: no-store sur l'API.
async fn security_headers_middleware(req: Request, next: Next) -> Response {
    let is_api = req.uri().path().starts_with("/api/") || req.uri().path() == "/api";
    let mut res = next.run(req).await;
    if is_api
        && (res.status().is_client_error() || res.status().is_server_error())
        && !res
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|h| h.to_str().ok())
            .is_some_and(|h| h.starts_with("application/json"))
    {
        let status = res.status();
        let message = match status {
            StatusCode::METHOD_NOT_ALLOWED => "Méthode HTTP non autorisée",
            StatusCode::PAYLOAD_TOO_LARGE => "Corps de requête trop volumineux",
            _ => "Requête refusée",
        };
        let replacement = (
            status,
            Json(serde_json::json!({"error":true,"message":message})),
        )
            .into_response();
        res.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        res.headers_mut().remove(header::CONTENT_LENGTH);
        *res.body_mut() = replacement.into_body();
    }
    let h = res.headers_mut();
    h.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    h.insert("x-frame-options", HeaderValue::from_static("DENY"));
    h.insert("referrer-policy", HeaderValue::from_static("no-referrer"));

    if is_api {
        h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    } else {
        if !h.contains_key(header::CACHE_CONTROL) {
            h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
        }
        h.insert(
            "content-security-policy",
            HeaderValue::from_static("default-src 'self'"),
        );
    }
    res
}

/// Santé du moteur (`GET /api/health`).
async fn api_health(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Json<serde_json::Value> {
    let mut body = serde_json::json!({
        "status": "ok",
        "product": apex_config::version::PRODUCT_NAME,
        "version": apex_config::version::VERSION,
    });
    if is_valid_control_token(&headers, &peer, &state.control_token) {
        body["runtime"] = serde_json::to_value(&*state.runtime).unwrap_or_default();
        body["started_at"] = serde_json::json!(state.started_at.to_rfc3339());
        body["pid"] = serde_json::json!(state.runtime.pid);
    }
    Json(body)
}

/// État système pour l'utilitaire desktop (`GET /api/system/status`).
async fn api_system_status(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if !is_valid_control_token(&headers, &peer, &state.control_token) {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({
                "error": true,
                "message": "Jeton de contrôle local requis ou pair non autorisé"
            })),
        ));
    }
    Ok(Json(serde_json::json!({
        "status": "ok",
        "product": apex_config::version::PRODUCT_NAME,
        "version": apex_config::version::VERSION,
        "pid": state.runtime.pid,
        "runtime": state.runtime.as_ref(),
        "started_at": state.started_at.to_rfc3339(),
    })))
}

/// Arrêt propre demandé depuis la boucle locale (`POST /api/system/shutdown`).
async fn api_system_shutdown(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if !is_valid_control_token(&headers, &peer, &state.control_token) {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({
                "error": true,
                "message": "Jeton de contrôle local requis ou pair non autorisé"
            })),
        ));
    }
    let notify = Arc::clone(&state.shutdown_notify);
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        notify.notify_waiters();
    });
    Ok(Json(serde_json::json!({
        "status": "shutting_down",
        "message": "Arrêt propre du serveur Apex Maint en cours..."
    })))
}

/// Réponse 404 JSON pour toute route API inconnue.
async fn api_fallback_404() -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({
            "error": true,
            "message": "Route API inconnue"
        })),
    )
}
