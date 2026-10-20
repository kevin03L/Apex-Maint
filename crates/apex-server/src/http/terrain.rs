//! Routes terrain et idempotence des cinq opérations synchronisables.
use super::{auth::ApiError, AppState};
use apex_core::{
    services::Session,
    terrain::{self, Operation},
    MaintError, Permission,
};
use axum::{
    extract::{Path, Query, Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
    routing::get,
    Extension, Json, Router,
};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/tls/ca.pem", get(ca))
        .route("/offline/snapshot", get(snapshot))
        .route("/equipments/by-code/:code", get(by_code))
}
async fn ca(State(state): State<AppState>) -> Result<Response, ApiError> {
    if state.tls_mode != "self_signed" {
        return Err(MaintError::NotFound("Autorité locale".into()).into());
    }
    let data = std::fs::read(std::path::Path::new(&state.runtime.data_dir).join("tls/ca.pem"))
        .map_err(apex_core::exploitation::io)?;
    Ok((
        [
            ("content-type", "application/x-pem-file"),
            (
                "content-disposition",
                "attachment; filename=apex-maint-ca.pem",
            ),
        ],
        data,
    )
        .into_response())
}
#[derive(Deserialize)]
struct SnapshotQuery {
    since: Option<String>,
}
async fn snapshot(
    State(state): State<AppState>,
    Extension(session): Extension<Session>,
    Query(query): Query<SnapshotQuery>,
    headers: axum::http::HeaderMap,
) -> Result<Response, ApiError> {
    let value = terrain::snapshot(&state.pool, &session.actor, query.since.as_deref()).await?;
    if headers
        .get("accept-encoding")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(',').any(|v| v.trim() == "gzip"))
    {
        use std::io::Write;
        let bytes = serde_json::to_vec(&value).map_err(apex_core::exploitation::io)?;
        let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gzip.write_all(&bytes)
            .map_err(apex_core::exploitation::io)?;
        let bytes = gzip.finish().map_err(apex_core::exploitation::io)?;
        return Ok((
            [
                ("content-type", "application/json"),
                ("content-encoding", "gzip"),
                ("vary", "Accept-Encoding"),
            ],
            bytes,
        )
            .into_response());
    }
    Ok(Json(value).into_response())
}
async fn by_code(
    State(state): State<AppState>,
    Extension(session): Extension<Session>,
    Path(code): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    session
        .actor
        .require_permission(Permission::EquipmentsView)?;
    let mut equipments =
        apex_core::referentiel::equipments_by_code(&state.pool, &session.actor, &code).await?;
    if equipments.is_empty() {
        return Err(MaintError::NotFound("Équipement inconnu sur ce site".into()).into());
    }
    Ok(Json(if equipments.len() == 1 {
        json!(equipments.remove(0))
    } else {
        json!(equipments)
    }))
}

fn synchronizable(method: &str, path: &str) -> bool {
    let p: Vec<_> = path.trim_start_matches('/').split('/').collect();
    matches!(
        (method, p.as_slice()),
        ("POST", ["interventions"])
            | ("PATCH", ["interventions", _])
            | (
                "POST",
                [
                    "interventions",
                    _,
                    "take" | "complete" | "sub-interventions"
                ]
            )
            | ("PUT", ["interventions", _, "technicians", _, "time"])
            | ("POST", ["interventions", _, "parts"])
            | ("POST", ["reservations", _, "consume"])
    )
}

pub async fn synchronize(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let path = req
        .uri()
        .path()
        .strip_prefix("/api")
        .unwrap_or(req.uri().path())
        .to_owned();
    if !synchronizable(req.method().as_str(), &path) {
        return next.run(req).await;
    }
    let stock = path.ends_with("/parts") || path.starts_with("/reservations/");
    if stock && !req.headers().contains_key("idempotency-key") {
        return next.run(req).await;
    }
    let Some(session) = req.extensions().get::<Session>().cloned() else {
        return next.run(req).await;
    };
    let route = format!("{} {path}", req.method());
    let context = {
        let header = |name: &str| {
            req.headers()
                .get(name)
                .map(|v| {
                    v.to_str().map(str::to_owned).map_err(|_| {
                        MaintError::BadRequest("En-tête de synchronisation invalide".into())
                    })
                })
                .transpose()
        };
        (|| -> Result<Operation, MaintError> {
            let key = header("idempotency-key")?;
            if key
                .as_ref()
                .is_some_and(|s| s.parse::<uuid::Uuid>().is_err())
            {
                return Err(MaintError::Unprocessable("UUID attendu".into()));
            }
            Ok(Operation {
                key,
                route,
                request_hash: None,
                occurred_at: header("x-occurred-at")?,
                unmodified_since: header("if-unmodified-since")?,
            })
        })()
    };
    let mut context = match context {
        Ok(c) => c,
        Err(e) => return ApiError::from(e).into_response(),
    };
    // Le corps exact identifie la tentative et est restitué au handler.
    let req = if context.key.is_some() {
        let (parts, body) = req.into_parts();
        let bytes = match axum::body::to_bytes(body, 64 * 1024).await {
            Ok(bytes) => bytes,
            Err(_) => {
                return ApiError::from(MaintError::BadRequest("Corps limité à 64 Kio".into()))
                    .into_response()
            }
        };
        context.request_hash = Some(format!("{:x}", Sha256::digest(&bytes)));
        Request::from_parts(parts, axum::body::Body::from(bytes))
    } else {
        req
    };
    if stock {
        if let Err(e) = session.actor.require_permission(Permission::StockMovements) {
            return ApiError::from(e).into_response();
        }
    }
    if let Err(e) = session.actor.require_permission(
        if stock && context.route.starts_with("POST /reservations/") {
            Permission::StockMovements
        } else if req.method() == "POST"
            && (path == "/interventions" || path.ends_with("/sub-interventions"))
        {
            Permission::InterventionsCreate
        } else {
            Permission::InterventionsEdit
        },
    ) {
        return ApiError::from(e).into_response();
    }
    if req.method() == "POST" && path.ends_with("/sub-interventions") {
        if let Err(e) = session
            .actor
            .require_permission(Permission::InterventionsEdit)
        {
            return ApiError::from(e).into_response();
        }
    }
    // Une seule instance détient la base ; le verrou évite deux prélectures concurrentes
    // lorsqu'une clé d'idempotence est fournie.
    // La réponse réussie est persistée par le dépôt AVANT le commit métier.
    let _guard = if context.key.is_some() {
        Some(state.terrain_lock.lock().await)
    } else {
        None
    };
    if let Some(key) = &context.key {
        let cached = sqlx::query_as::<_, (String, Option<String>, String, i64, String, Option<String>)>(
            "SELECT user_id,site_id,route,response_code,response_body,request_hash FROM idempotency_keys WHERE key=?",
        )
        .bind(key)
        .fetch_optional(&state.pool)
        .await;
        match cached {
            Err(e) => return ApiError::from(e).into_response(),
            Ok(Some((user, site, route, code, body, request_hash))) => {
                if user != session.actor.user_id
                    || site != session.actor.site_id
                    || route != context.route
                    || request_hash.is_some_and(|hash| Some(&hash) != context.request_hash.as_ref())
                {
                    return ApiError::from(MaintError::Unprocessable(
                        "Clé déjà utilisée par un autre utilisateur, site, corps ou une autre route".into(),
                    ))
                    .into_response();
                }
                return (
                    StatusCode::from_u16(code as u16).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                    [(axum::http::header::CONTENT_TYPE, "application/json")],
                    body,
                )
                    .into_response();
            }
            Ok(None) => {}
        }
    }
    let response = terrain::OPERATION
        .scope(context.clone(), next.run(req))
        .await;
    if response.status().is_client_error() && context.key.is_some() {
        let (parts, body) = response.into_parts();
        let bytes = match axum::body::to_bytes(body, 64 * 1024).await {
            Ok(bytes) => bytes,
            Err(e) => return ApiError::from(MaintError::Internal(e.to_string())).into_response(),
        };
        if serde_json::from_slice::<serde_json::Value>(&bytes).is_err() {
            return Response::from_parts(parts, axum::body::Body::from(bytes));
        }
        let save = async {
            let value: serde_json::Value =
                serde_json::from_slice(&bytes).map_err(apex_core::exploitation::io)?;
            let mut tx = state.pool.begin_with("BEGIN IMMEDIATE").await?;
            terrain::record(&mut tx, &session.actor, &value, parts.status.as_u16()).await?;
            tx.commit().await?;
            Ok::<(), MaintError>(())
        };
        if let Err(e) = terrain::OPERATION.scope(context, save).await {
            return ApiError::from(e).into_response();
        }
        return Response::from_parts(parts, axum::body::Body::from(bytes));
    }
    response
}
