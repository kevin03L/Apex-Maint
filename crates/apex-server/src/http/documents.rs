//! API des documents, téléchargements et paramètres de la phase 6.
use std::collections::HashMap;

use apex_core::{
    documents::{self, storage, types, CreateDocument, SearchFilter, Upload},
    services::Session,
    Cursor, MaintError, Permission,
};
use axum::{
    body::Body,
    extract::{DefaultBodyLimit, Multipart, Path, Query, Request, State},
    http::{header, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    routing::{get, post},
    Extension, Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use super::{
    errors::{body, ApiError, Body as JsonBody, Result},
    AppState,
};
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/documents",
            get(list).post(create).layer(DefaultBodyLimit::disable()),
        )
        .route("/documents/search", get(search))
        .route(
            "/documents/:id",
            get(get_document).patch(update).delete(delete),
        )
        .route(
            "/documents/:id/versions",
            get(versions)
                .post(new_version)
                .layer(DefaultBodyLimit::disable()),
        )
        .route("/documents/:id/versions/:n/restore", post(restore))
        .route("/documents/:id/download", get(download))
        .route("/equipments/:id/qr.svg", get(qr))
        .route("/settings/documents", get(settings).put(set_settings))
        .route("/settings/labels", get(labels).put(set_labels))
        .route("/settings/storage", get(list_storage).post(create_storage))
        .route(
            "/settings/storage/:id",
            axum::routing::patch(update_storage).delete(delete_storage),
        )
        .route("/settings/storage/:id/activate", post(activate_storage))
        .route("/settings/storage/:id/test", post(test_storage))
}

/// Appliqué après la garde de session : la limite dynamique concerne seulement les envois.
pub async fn upload_limit(
    State(state): State<AppState>,
    session: Option<Extension<Session>>,
    mut req: Request,
    next: Next,
) -> Response {
    let path = req.uri().path();
    let is_upload = req.method() == axum::http::Method::POST
        && (path == "/documents" || path == "/api/documents" || path.ends_with("/versions"));
    if is_upload {
        let Some(Extension(session)) = session else {
            return ApiError::from(MaintError::Unauthorized("Session requise".into()))
                .into_response();
        };
        if let Err(e) = session
            .actor
            .require_permission(Permission::DocumentsCreate)
        {
            return ApiError::from(e).into_response();
        }
        let settings = match documents::settings_internal(&state.pool, &session.actor).await {
            Ok(s) => s,
            Err(e) => return ApiError::from(e).into_response(),
        };
        let effective_max_mb = settings.max_size_mb.min(state.files_max_size_mb);
        req.extensions_mut().insert(UploadLimit(effective_max_mb));
        let max = effective_max_mb as usize * 1024 * 1024 + 64 * 1024;
        if req
            .headers()
            .get(header::CONTENT_LENGTH)
            .and_then(|h| h.to_str().ok())
            .and_then(|s| s.parse::<usize>().ok())
            .is_some_and(|n| n > max)
        {
            return ApiError::from(MaintError::PayloadTooLarge(
                "Fichier trop volumineux".into(),
            ))
            .into_response();
        }
        // L'extension est lue par l'extracteur Multipart, y compris pour un corps chunked.
        let req = req.map(|body| Body::new(http_body_util::Limited::new(body, max)));
        return next.run(req).await;
    }
    next.run(req).await
}

#[derive(Deserialize)]
struct EntityQuery {
    entity_type: String,
    entity_id: String,
}
async fn list(
    State(s): State<AppState>,
    Extension(session): Extension<Session>,
    Query(q): Query<EntityQuery>,
) -> Result<Json<Vec<documents::Document>>> {
    Ok(Json(
        documents::list_documents(&s.pool, &session.actor, &q.entity_type, &q.entity_id).await?,
    ))
}
#[derive(Deserialize)]
struct SearchQuery {
    q: Option<String>,
    entity_type: Option<String>,
    category: Option<String>,
    site_id: Option<String>,
    cursor: Option<String>,
    limit: Option<u32>,
}
async fn search(
    State(s): State<AppState>,
    Extension(session): Extension<Session>,
    Query(q): Query<SearchQuery>,
) -> Result<Json<apex_core::referentiel::Page<documents::Document>>> {
    Ok(Json(
        documents::search(
            &s.pool,
            &session.actor,
            SearchFilter {
                q: q.q,
                entity_type: q.entity_type,
                category: q.category,
                site_id: q.site_id,
                cursor: q.cursor.as_deref().map(Cursor::decode).transpose()?,
                limit: q.limit.unwrap_or(50),
            },
        )
        .await?,
    ))
}
async fn get_document(
    State(s): State<AppState>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
) -> Result<Json<documents::Document>> {
    Ok(Json(
        documents::get_document(&s.pool, &session.actor, &id).await?,
    ))
}
async fn update(
    State(s): State<AppState>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
    input: JsonBody<Value>,
) -> Result<Json<documents::Document>> {
    Ok(Json(
        documents::update_document(&s.pool, &session.actor, &id, body(input)?, false).await?,
    ))
}
async fn delete(
    State(s): State<AppState>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    documents::update_document(&s.pool, &session.actor, &id, json!({}), true).await?;
    Ok(StatusCode::NO_CONTENT)
}
async fn versions(
    State(s): State<AppState>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
) -> Result<Json<Vec<documents::Version>>> {
    Ok(Json(
        documents::versions(&s.pool, &session.actor, &id).await?,
    ))
}
async fn restore(
    State(s): State<AppState>,
    Extension(session): Extension<Session>,
    Path((id, n)): Path<(String, i64)>,
) -> Result<(StatusCode, Json<documents::Version>)> {
    Ok((
        StatusCode::CREATED,
        Json(documents::restore(&s.pool, &session.actor, &id, n).await?),
    ))
}

#[derive(Clone, Copy)]
struct UploadLimit(u32);

struct Received {
    file: tempfile::NamedTempFile,
    name: String,
    size: i64,
    sha256: String,
    fields: HashMap<String, String>,
}
fn multipart_error(e: axum::extract::multipart::MultipartError) -> ApiError {
    if e.status() == StatusCode::PAYLOAD_TOO_LARGE {
        MaintError::PayloadTooLarge("Fichier trop volumineux".into()).into()
    } else {
        MaintError::BadRequest("Envoi multipart invalide".into()).into()
    }
}
async fn receive(
    s: &AppState,
    session: &Session,
    mut multipart: Multipart,
    version: bool,
    limit: UploadLimit,
) -> Result<Received> {
    session
        .actor
        .require_permission(Permission::DocumentsCreate)?;
    let max = u64::from(limit.0) * 1024 * 1024;
    let mut fields = HashMap::new();
    let mut received = None;
    while let Some(mut field) = multipart.next_field().await.map_err(multipart_error)? {
        let field_name = field.name().unwrap_or("").to_string();
        if field_name == "file" {
            if received.is_some() {
                return Err(MaintError::BadRequest("Un seul fichier est accepté".into()).into());
            }
            let name = field.file_name().unwrap_or("").to_string();
            let file = tempfile::NamedTempFile::new_in(&s.files.files_dir)
                .map_err(|e| MaintError::Internal(e.to_string()))?;
            let mut out = tokio::fs::File::from_std(
                file.reopen()
                    .map_err(|e| MaintError::Internal(e.to_string()))?,
            );
            let mut size = 0u64;
            let mut hash = Sha256::new();
            while let Some(chunk) = field.chunk().await.map_err(multipart_error)? {
                size += chunk.len() as u64;
                if size > max {
                    return Err(
                        MaintError::PayloadTooLarge("Fichier trop volumineux".into()).into(),
                    );
                }
                hash.update(&chunk);
                out.write_all(&chunk)
                    .await
                    .map_err(|e| MaintError::Internal(e.to_string()))?;
            }
            out.flush()
                .await
                .map_err(|e| MaintError::Internal(e.to_string()))?;
            drop(out);
            received = Some((file, name, size as i64, format!("{:x}", hash.finalize())));
        } else {
            let allowed = if version {
                &["comment"][..]
            } else {
                &[
                    "entity_type",
                    "entity_id",
                    "title",
                    "category",
                    "description",
                ][..]
            };
            if !allowed.contains(&field_name.as_str()) || fields.contains_key(&field_name) {
                return Err(
                    MaintError::BadRequest("Champ multipart inconnu ou répété".into()).into(),
                );
            }
            let mut data = Vec::new();
            while let Some(chunk) = field.chunk().await.map_err(multipart_error)? {
                if data.len() + chunk.len() > 16384 {
                    return Err(MaintError::BadRequest("Champ multipart trop long".into()).into());
                }
                data.extend_from_slice(&chunk);
            }
            fields.insert(
                field_name,
                String::from_utf8(data)
                    .map_err(|_| MaintError::BadRequest("Texte multipart invalide".into()))?,
            );
        }
    }
    let (file, name, size, sha256) =
        received.ok_or_else(|| MaintError::BadRequest("Fichier requis".into()))?;
    Ok(Received {
        file,
        name,
        size,
        sha256,
        fields,
    })
}
async fn create(
    State(s): State<AppState>,
    Extension(session): Extension<Session>,
    Extension(limit): Extension<UploadLimit>,
    multipart: Multipart,
) -> Result<(StatusCode, Json<documents::Document>)> {
    let mut r = receive(&s, &session, multipart, false, limit).await?;
    let mut required = |name: &str| {
        r.fields
            .remove(name)
            .ok_or_else(|| MaintError::BadRequest(format!("Champ {name} requis")))
    };
    let input = CreateDocument {
        entity_type: required("entity_type")?,
        entity_id: required("entity_id")?,
        title: required("title")?,
        category: r.fields.remove("category").filter(|s| !s.is_empty()),
        description: r.fields.remove("description"),
    };
    let document = documents::upload(
        &s.pool,
        &session.actor,
        &s.files,
        None,
        Some(input),
        Upload {
            path: r.file.path(),
            original_name: r.name,
            size_bytes: r.size,
            sha256: r.sha256,
            comment: None,
        },
    )
    .await?;
    Ok((StatusCode::CREATED, Json(document)))
}
async fn new_version(
    State(s): State<AppState>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
    Extension(limit): Extension<UploadLimit>,
    multipart: Multipart,
) -> Result<(StatusCode, Json<documents::Version>)> {
    // Vérifie le site avant de lire le moindre octet de fichier.
    documents::entity_for_upload(&s.pool, &session.actor, &id).await?;
    let mut r = receive(&s, &session, multipart, true, limit).await?;
    let document = documents::upload(
        &s.pool,
        &session.actor,
        &s.files,
        Some(&id),
        None,
        Upload {
            path: r.file.path(),
            original_name: r.name,
            size_bytes: r.size,
            sha256: r.sha256,
            comment: r.fields.remove("comment"),
        },
    )
    .await?;
    Ok((StatusCode::CREATED, Json(document.current_version)))
}
#[derive(Deserialize)]
struct DownloadQuery {
    version: Option<i64>,
    download: Option<u8>,
}
async fn download(
    State(s): State<AppState>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
    Query(q): Query<DownloadQuery>,
) -> Result<Response> {
    let (v, data) = documents::download(&s.pool, &session.actor, &s.files, &id, q.version).await?;
    let inline = q.download != Some(1)
        && types::CATALOG
            .iter()
            .any(|t| t.mime == v.mime_type && t.disposition == "inline");
    let encoded: String = v
        .original_name
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"!#$&+-.^_`|~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect();
    let disposition = format!(
        "{}; filename*=UTF-8''{}",
        if inline { "inline" } else { "attachment" },
        encoded
    );
    let mut response = Body::from_stream(data.into_stream()).into_response();
    let h = response.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&v.mime_type)
            .map_err(|_| MaintError::Internal("Type MIME invalide".into()))?,
    );
    h.insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&v.size_bytes.to_string())
            .map_err(|_| MaintError::Internal("Taille invalide".into()))?,
    );
    h.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&disposition)
            .map_err(|_| MaintError::Internal("Nom invalide".into()))?,
    );
    h.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    h.insert(
        "content-security-policy",
        HeaderValue::from_static("sandbox; default-src 'none'"),
    );
    Ok(response)
}
async fn qr(
    State(s): State<AppState>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
) -> Result<Response> {
    session
        .actor
        .require_permission(Permission::EquipmentsView)?;
    let entity = documents::entity(
        &mut *s.pool.acquire().await?,
        &session.actor,
        "equipment",
        &id,
        true,
    )
    .await?;
    Ok((
        [
            (header::CONTENT_TYPE, "image/svg+xml"),
            (header::CACHE_CONTROL, "private, max-age=3600"),
        ],
        types::qr_svg(&entity.code)?,
    )
        .into_response())
}
async fn settings(
    State(s): State<AppState>,
    Extension(session): Extension<Session>,
) -> Result<Json<Value>> {
    Ok(Json(documents::settings(&s.pool, &session.actor).await?))
}
async fn set_settings(
    State(s): State<AppState>,
    Extension(session): Extension<Session>,
    input: JsonBody<documents::DocumentSettings>,
) -> Result<Json<Value>> {
    Ok(Json(
        documents::set_settings(&s.pool, &session.actor, body(input)?).await?,
    ))
}
async fn labels(
    State(s): State<AppState>,
    Extension(session): Extension<Session>,
) -> Result<Json<types::LabelFormat>> {
    Ok(Json(documents::labels(&s.pool, &session.actor).await?))
}
async fn set_labels(
    State(s): State<AppState>,
    Extension(session): Extension<Session>,
    input: JsonBody<types::LabelFormat>,
) -> Result<Json<types::LabelFormat>> {
    Ok(Json(
        documents::set_labels(&s.pool, &session.actor, body(input)?).await?,
    ))
}
async fn list_storage(
    State(s): State<AppState>,
    Extension(session): Extension<Session>,
) -> Result<Json<Vec<storage::StorageBackend>>> {
    Ok(Json(storage::list_storage(&s.pool, &session.actor).await?))
}
async fn create_storage(
    State(s): State<AppState>,
    Extension(session): Extension<Session>,
    input: JsonBody<Value>,
) -> Result<(StatusCode, Json<storage::StorageBackend>)> {
    Ok((
        StatusCode::CREATED,
        Json(storage::save_storage(&s.pool, &session.actor, &s.files, None, body(input)?).await?),
    ))
}
async fn update_storage(
    State(s): State<AppState>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
    input: JsonBody<Value>,
) -> Result<Json<storage::StorageBackend>> {
    Ok(Json(
        storage::save_storage(&s.pool, &session.actor, &s.files, Some(&id), body(input)?).await?,
    ))
}
async fn delete_storage(
    State(s): State<AppState>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    storage::change_storage(&s.pool, &session.actor, &s.files, &id, true).await?;
    Ok(StatusCode::NO_CONTENT)
}
async fn activate_storage(
    State(s): State<AppState>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    storage::change_storage(&s.pool, &session.actor, &s.files, &id, false).await?;
    Ok(StatusCode::NO_CONTENT)
}
async fn test_storage(
    State(s): State<AppState>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    Ok(Json(
        storage::test_storage(&s.pool, &session.actor, &s.files, &id).await?,
    ))
}
