//! Authentification et administration des comptes : cookies de session, protection CSRF,
//! sites, utilisateurs et matrice des permissions.
use apex_core::{services::Session, store, MaintError, Permission, Role};
use axum::{
    extract::{rejection::JsonRejection, ConnectInfo, Path, Request, State},
    http::{header, HeaderMap, HeaderValue, Method, StatusCode, Uri},
    middleware::Next,
    response::{IntoResponse, Response},
    routing::{get, post, put},
    Extension, Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::net::SocketAddr;

use super::AppState;

pub struct ApiError(MaintError);
impl From<MaintError> for ApiError {
    fn from(e: MaintError) -> Self {
        Self(e)
    }
}
impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        Self(e.into())
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status =
            StatusCode::from_u16(self.0.http_status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let message = if status.is_server_error() {
            tracing::error!("API : {}", self.0);
            "Erreur interne du serveur".to_string()
        } else {
            match &self.0 {
                MaintError::Unauthorized(message) => message.clone(),
                _ => self.0.to_string(),
            }
        };
        (status, Json(json!({"error":true,"message":message}))).into_response()
    }
}
type Result<T> = std::result::Result<T, ApiError>;
pub(super) type Body<T> = std::result::Result<Json<T>, JsonRejection>;
pub(super) fn body<T>(input: Body<T>) -> Result<T> {
    input.map(|Json(v)| v).map_err(|e| {
        ApiError(if e.status() == StatusCode::PAYLOAD_TOO_LARGE {
            MaintError::PayloadTooLarge("Corps JSON trop volumineux (64 Kio maximum)".into())
        } else {
            MaintError::BadRequest("Corps JSON invalide ou champs inconnus".into())
        })
    })
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/auth/login", post(login))
        .route("/auth/logout", post(logout))
        .route("/auth/me", get(me))
        .route("/auth/password", post(change_password))
        .route("/sites", get(sites).post(create_site))
        .route(
            "/sites/:id",
            axum::routing::patch(update_site).delete(delete_site),
        )
        .route("/users", get(users).post(create_user))
        .route(
            "/users/:id",
            get(user).patch(update_user).delete(delete_user),
        )
        .route("/users/:id/password", post(reset_password))
        .route("/permissions", get(permissions))
        .route("/permissions/:role", put(set_permissions))
}

fn cookie_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .find_map(|p| {
            let (key, value) = p.trim().split_once('=')?;
            (key == "apex_maint_session").then_some(value)
        })
}
fn cookie(state: &AppState, token: &str, expired: bool, https: bool) -> HeaderValue {
    HeaderValue::from_str(&format!(
        "apex_maint_session={token}; HttpOnly; SameSite=Strict; Path=/{}{}",
        if state.secure_cookie || https {
            "; Secure"
        } else {
            ""
        },
        if expired {
            "; Max-Age=0; Expires=Thu, 01 Jan 1970 00:00:00 GMT"
        } else {
            ""
        }
    ))
    .expect("cookie opaque")
}

pub async fn guard(State(state): State<AppState>, mut req: Request, next: Next) -> Response {
    let path = req.uri().path().to_string();
    // Selon la couche de routage Axum, le préfixe imbriqué peut déjà être retiré.
    let path = path.strip_prefix("/api").unwrap_or(&path);
    if path.starts_with("/integrations/supervision/") {
        return super::integration::guard(&state, req, next).await;
    }
    if path.starts_with("/system/") {
        return next.run(req).await;
    }
    if matches!(
        *req.method(),
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    ) {
        let headers = req.headers();
        let fetch = headers.get("sec-fetch-site").and_then(|v| v.to_str().ok());
        // HTTP/2 ne transmet pas d'en-tête Host : l'hôte arrive dans la pseudo-en-tête
        // :authority, que hyper place dans l'URI de la requête.
        let host = headers
            .get(header::HOST)
            .and_then(|h| h.to_str().ok())
            .or_else(|| req.uri().authority().map(|a| a.as_str()));
        let origin_ok = match headers.get(header::ORIGIN) {
            None => true,
            Some(value) => value
                .to_str()
                .ok()
                .and_then(|s| s.parse::<Uri>().ok())
                .is_some_and(|uri| {
                    matches!(uri.scheme_str(), Some("http" | "https"))
                        && uri.authority().is_some_and(|a| {
                            host.is_some_and(|h| h.eq_ignore_ascii_case(a.as_str()))
                        })
                        && uri.path() == "/"
                        && uri.query().is_none()
                }),
        };
        if !origin_ok || matches!(fetch, Some("cross-site" | "same-site")) {
            return ApiError(MaintError::Forbidden(
                "Origine de la requête refusée".into(),
            ))
            .into_response();
        }
    }
    // Refus par défaut : toute route de l'API exige une session, sauf ces routes publiques.
    // Phase 7 : identité de l'écran de connexion et récupération du compte (§2.8).
    // Phase 9 : certificat de l'autorité locale, à installer avant toute connexion.
    // Ce passage reste après le contrôle d'Origin, également pour les deux POST publics.
    if matches!(
        path,
        "/health"
            | "/auth/login"
            | "/tls/ca.pem"
            | "/public/branding"
            | "/auth/password-reset/request"
            | "/auth/password-reset/confirm"
    ) {
        return next.run(req).await;
    }
    let session = match cookie_token(req.headers()) {
        Some(token) => state.auth.session(token).await,
        None => Err(MaintError::Unauthorized("Session requise".into())),
    };
    match session {
        Err(e) => ApiError(e).into_response(),
        Ok(session) => {
            let allowed = (path == "/auth/me" && req.method() == Method::GET)
                || (matches!(path, "/auth/password" | "/auth/logout")
                    && req.method() == Method::POST);
            if session.user.must_change_password && !allowed {
                return (StatusCode::FORBIDDEN,Json(json!({"error":true,"message":"Changez votre mot de passe pour continuer","code":"password_change_required"}))).into_response();
            }
            req.extensions_mut().insert(session);
            next.run(req).await
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Login {
    username: String,
    password: String,
}
async fn login(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Extension(super::RequestHttps(https)): Extension<super::RequestHttps>,
    input: Body<Login>,
) -> Result<Response> {
    let input = body(input)?;
    let ip = peer.ip().to_string();
    let agent = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok());
    let (user, token) = state
        .auth
        .login(&input.username, input.password, Some(&ip), agent)
        .await?;
    Ok((
        [(header::SET_COOKIE, cookie(&state, &token, false, https))],
        Json(json!({"user":user})),
    )
        .into_response())
}
async fn logout(
    State(state): State<AppState>,
    Extension(session): Extension<Session>,
    Extension(super::RequestHttps(https)): Extension<super::RequestHttps>,
) -> Result<Response> {
    state.auth.logout(&session).await?;
    Ok((
        StatusCode::NO_CONTENT,
        [(header::SET_COOKIE, cookie(&state, "", true, https))],
    )
        .into_response())
}
async fn me(
    State(state): State<AppState>,
    Extension(session): Extension<Session>,
) -> Result<Json<Value>> {
    let sites = store::sites(&state.pool, &session.actor).await?;
    let site = sites
        .into_iter()
        .find(|s| Some(&s.id) == session.user.site_id.as_ref());
    let mut permissions: Vec<_> = session.actor.permissions.iter().copied().collect();
    permissions.sort();
    Ok(Json(
        json!({"user":session.user,"permissions":permissions,"site":site,"must_change_password":session.user.must_change_password}),
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PasswordChange {
    current_password: String,
    new_password: String,
}
async fn change_password(
    State(state): State<AppState>,
    Extension(session): Extension<Session>,
    input: Body<PasswordChange>,
) -> Result<StatusCode> {
    let input = body(input)?;
    state
        .auth
        .change_password(&session, input.current_password, input.new_password)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
async fn sites(
    State(state): State<AppState>,
    Extension(session): Extension<Session>,
) -> Result<Json<Value>> {
    Ok(Json(json!(
        store::sites(&state.pool, &session.actor).await?
    )))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SiteInput {
    name: String,
}
async fn create_site(
    State(state): State<AppState>,
    Extension(session): Extension<Session>,
    input: Body<SiteInput>,
) -> Result<(StatusCode, Json<Value>)> {
    let input = body(input)?;
    Ok((
        StatusCode::CREATED,
        Json(store::save_site(&state.pool, &session.actor, None, Some(&input.name), false).await?),
    ))
}
async fn update_site(
    State(state): State<AppState>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
    input: Body<SiteInput>,
) -> Result<Json<Value>> {
    let input = body(input)?;
    Ok(Json(
        store::save_site(
            &state.pool,
            &session.actor,
            Some(&id),
            Some(&input.name),
            false,
        )
        .await?,
    ))
}
async fn delete_site(
    State(state): State<AppState>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    store::save_site(&state.pool, &session.actor, Some(&id), None, true).await?;
    Ok(StatusCode::NO_CONTENT)
}
async fn users(
    State(state): State<AppState>,
    Extension(session): Extension<Session>,
) -> Result<Json<Value>> {
    Ok(Json(json!(
        store::users(&state.pool, &session.actor).await?
    )))
}
async fn user(
    State(state): State<AppState>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    session.actor.require_permission(Permission::UsersView)?;
    Ok(Json(json!(
        store::user(&mut *state.pool.acquire().await?, &session.actor, &id).await?
    )))
}
async fn create_user(
    State(state): State<AppState>,
    Extension(session): Extension<Session>,
    input: Body<store::CreateUser>,
) -> Result<(StatusCode, Json<Value>)> {
    Ok((
        StatusCode::CREATED,
        Json(json!(
            store::create_user(&state.pool, &session.actor, body(input)?).await?
        )),
    ))
}
async fn update_user(
    State(state): State<AppState>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
    input: Body<Value>,
) -> Result<Json<Value>> {
    Ok(Json(json!(
        store::update_user(&state.pool, &session.actor, &id, body(input)?, false).await?
    )))
}
async fn delete_user(
    State(state): State<AppState>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    store::update_user(&state.pool, &session.actor, &id, json!({}), true).await?;
    Ok(StatusCode::NO_CONTENT)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResetPassword {
    #[serde(alias = "new_password")]
    password: String,
}
async fn reset_password(
    State(state): State<AppState>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
    input: Body<ResetPassword>,
) -> Result<StatusCode> {
    store::reset_password(&state.pool, &session.actor, &id, body(input)?.password).await?;
    Ok(StatusCode::NO_CONTENT)
}
async fn permissions(
    State(state): State<AppState>,
    Extension(session): Extension<Session>,
) -> Result<Json<Value>> {
    Ok(Json(store::permissions(&state.pool, &session.actor).await?))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Permissions {
    permissions: Vec<Permission>,
}
async fn set_permissions(
    State(state): State<AppState>,
    Extension(session): Extension<Session>,
    Path(role): Path<String>,
    input: Body<Permissions>,
) -> Result<StatusCode> {
    let role = Role::parse(&role).map_err(|e| MaintError::BadRequest(e.to_string()))?;
    store::set_permissions(&state.pool, &session.actor, role, body(input)?.permissions).await?;
    Ok(StatusCode::NO_CONTENT)
}
