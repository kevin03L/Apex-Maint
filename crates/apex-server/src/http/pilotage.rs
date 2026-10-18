//! API de pilotage, exports, paramètres et récupération du compte.
use super::{auth::ApiError, AppState};
use apex_core::{
    pilotage::{audit, dashboard, exports, notifications, reports, reset, settings},
    services::Session,
    MaintError, Permission, Role,
};
use axum::{
    extract::{rejection::JsonRejection, ConnectInfo, Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Extension, Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{collections::BTreeMap, net::SocketAddr};
type Result<T> = std::result::Result<T, ApiError>;
pub type WidgetCache =
    std::sync::Arc<tokio::sync::Mutex<BTreeMap<String, (std::time::Instant, Value)>>>;
type Body<T> = std::result::Result<Json<T>, JsonRejection>;
fn body<T>(input: Body<T>) -> Result<T> {
    input
        .map(|Json(v)| v)
        .map_err(|_| MaintError::BadRequest("Corps JSON invalide ou champs inconnus".into()).into())
}
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/dashboard",
            get(dashboard_get)
                .put(dashboard_put)
                .delete(dashboard_delete),
        )
        .route("/dashboard/catalog", get(catalog))
        .route(
            "/dashboard/defaults/:role",
            get(defaults_get).put(defaults_put),
        )
        .route("/dashboard/widgets/:widget", get(widget))
        .route("/reports/:report", get(report))
        .route("/reports/:report/export", get(report_export))
        .route("/exports/:list", get(list_export))
        .route("/interventions/:id/pdf", get(intervention_pdf))
        .route("/audit", get(audit_list))
        .route("/audit/:id", get(audit_get))
        .route("/settings/general", get(general_get).put(general_put))
        .route("/settings/security", get(security_get).put(security_put))
        .route("/settings/stock", get(stock_get).put(stock_put))
        .route("/settings/smtp", get(smtp_get).put(smtp_put))
        .route("/settings/alerts", get(alerts_get).put(alerts_put))
        .route(
            "/settings/audit",
            get(audit_settings_get).put(audit_settings_put),
        )
        .route("/settings/smtp/test", post(smtp_test))
        .route("/settings/smtp/outbox", get(outbox))
        .route(
            "/profile/notifications",
            get(preferences_get).put(preferences_put),
        )
        .route("/public/branding", get(branding))
        .route("/auth/password-reset/request", post(reset_request))
        .route("/auth/password-reset/confirm", post(reset_confirm))
}
async fn dashboard_get(
    State(s): State<AppState>,
    Extension(session): Extension<Session>,
) -> Result<Json<Value>> {
    Ok(Json(dashboard::resolve(&s.pool, &session.actor).await?))
}
async fn dashboard_put(
    State(s): State<AppState>,
    Extension(session): Extension<Session>,
    input: Body<dashboard::Layout>,
) -> Result<Json<Value>> {
    dashboard::save(&s.pool, &session.actor, None, Some(body(input)?)).await?;
    dashboard_get(State(s), Extension(session)).await
}
async fn dashboard_delete(
    State(s): State<AppState>,
    Extension(session): Extension<Session>,
) -> Result<Json<Value>> {
    dashboard::save(&s.pool, &session.actor, None, None).await?;
    dashboard_get(State(s), Extension(session)).await
}
async fn catalog(Extension(session): Extension<Session>) -> Result<Json<Value>> {
    Ok(Json(dashboard::catalog(&session.actor)?))
}
fn role(s: &str) -> Result<Role> {
    Role::parse(s).map_err(|_| MaintError::BadRequest("Rôle inconnu".into()).into())
}
async fn defaults_get(
    State(s): State<AppState>,
    Extension(session): Extension<Session>,
    Path(r): Path<String>,
) -> Result<Json<Value>> {
    Ok(Json(json!(
        dashboard::defaults(&s.pool, &session.actor, role(&r)?).await?
    )))
}
async fn defaults_put(
    State(s): State<AppState>,
    Extension(session): Extension<Session>,
    Path(r): Path<String>,
    input: Body<dashboard::Layout>,
) -> Result<Json<Value>> {
    dashboard::save(&s.pool, &session.actor, Some(role(&r)?), Some(body(input)?)).await?;
    defaults_get(State(s), Extension(session), Path(r)).await
}
async fn widget(
    State(s): State<AppState>,
    Extension(session): Extension<Session>,
    Path(name): Path<String>,
    Query(f): Query<reports::Filter>,
) -> Result<Json<Value>> {
    session
        .actor
        .require_permission(Permission::DashboardView)?;
    for permission in dashboard::permissions(&name)? {
        session.actor.require_permission(*permission)?;
    }
    reports::check(&s.pool, &session.actor, &f).await?;
    let mut permissions: Vec<_> = session.actor.permissions.iter().copied().collect();
    permissions.sort();
    let key = json!([
        session.actor.user_id,
        session.actor.site_id,
        session.actor.role,
        permissions,
        name,
        f
    ])
    .to_string();
    {
        let mut cache = s.widget_cache.lock().await;
        cache.retain(|_, (at, _)| at.elapsed().as_secs() < 60);
        if let Some((_, value)) = cache.get(&key) {
            return Ok(Json(value.clone()));
        }
    }
    let value = reports::widget(&s.pool, &session.actor, &name, &f).await?;
    let mut cache = s.widget_cache.lock().await;
    if cache.len() >= 512 {
        cache.clear();
    }
    cache.insert(key, (std::time::Instant::now(), value.clone()));
    Ok(Json(value))
}
async fn report(
    State(s): State<AppState>,
    Extension(session): Extension<Session>,
    Path(name): Path<String>,
    Query(f): Query<reports::Filter>,
) -> Result<Json<Value>> {
    Ok(Json(
        reports::report(&s.pool, &session.actor, &name, &f).await?,
    ))
}
async fn audit_list(
    State(s): State<AppState>,
    Extension(session): Extension<Session>,
    Query(f): Query<audit::Filter>,
) -> Result<Json<Value>> {
    Ok(Json(audit::list(&s.pool, &session.actor, &f).await?))
}
async fn audit_get(
    State(s): State<AppState>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    Ok(Json(audit::get(&s.pool, &session.actor, &id).await?))
}
macro_rules! section {
    ($get:ident,$put:ident,$name:literal) => {
        async fn $get(
            State(s): State<AppState>,
            Extension(session): Extension<Session>,
        ) -> Result<Json<Value>> {
            Ok(Json(settings::get(&s.pool, &session.actor, $name).await?))
        }
        async fn $put(
            State(s): State<AppState>,
            Extension(session): Extension<Session>,
            input: Body<Value>,
        ) -> Result<Json<Value>> {
            Ok(Json(
                settings::save(&s.pool, &s.files, &session.actor, $name, body(input)?).await?,
            ))
        }
    };
}
section!(general_get, general_put, "general");
section!(security_get, security_put, "security");
section!(stock_get, stock_put, "stock");
section!(smtp_get, smtp_put, "smtp");
section!(alerts_get, alerts_put, "alerts");
section!(audit_settings_get, audit_settings_put, "audit");
async fn branding(State(s): State<AppState>) -> Result<Json<Value>> {
    Ok(Json(settings::branding(&s.pool).await?))
}
async fn preferences_get(
    State(s): State<AppState>,
    Extension(session): Extension<Session>,
) -> Result<Json<Value>> {
    Ok(Json(
        notifications::preferences(&s.pool, &session.actor, None).await?,
    ))
}
async fn preferences_put(
    State(s): State<AppState>,
    Extension(session): Extension<Session>,
    input: Body<Value>,
) -> Result<Json<Value>> {
    Ok(Json(
        notifications::preferences(&s.pool, &session.actor, Some(body(input)?)).await?,
    ))
}
async fn outbox(
    State(s): State<AppState>,
    Extension(session): Extension<Session>,
    Query(filter): Query<notifications::OutboxFilter>,
) -> Result<Json<Value>> {
    Ok(Json(
        notifications::outbox(&s.pool, &session.actor, &filter).await?,
    ))
}
async fn smtp_test(
    State(s): State<AppState>,
    Extension(session): Extension<Session>,
) -> Result<StatusCode> {
    notifications::test(&s.pool, &session.actor).await?;
    Ok(StatusCode::ACCEPTED)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestReset {
    identifier: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfirmReset {
    token: String,
    new_password: String,
}
async fn reset_request(
    State(s): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    input: Body<RequestReset>,
) -> Result<(StatusCode, Json<Value>)> {
    reset::request(&s.pool, &body(input)?.identifier, &peer.ip().to_string()).await?;
    Ok((
        StatusCode::ACCEPTED,
        Json(
            json!({"message":"Si le compte le permet, un lien de réinitialisation vous sera envoyé."}),
        ),
    ))
}
async fn reset_confirm(State(s): State<AppState>, input: Body<ConfirmReset>) -> Result<StatusCode> {
    let input = body(input)?;
    reset::confirm(&s.pool, &input.token, input.new_password).await?;
    Ok(StatusCode::NO_CONTENT)
}
async fn download(
    s: &AppState,
    session: &Session,
    name: &str,
    p: exports::Params,
    rows: Vec<Value>,
    landscape: bool,
) -> Result<Response> {
    session.actor.require_permission(Permission::ExportsRun)?;
    let format = p.get("format").map(String::as_str).unwrap_or("csv");
    let (headers, table) = exports::table(&rows);
    let (mime, bytes) = match format {
        "csv" => {
            let first = exports::csv(&headers, &[]);
            let chunks = std::iter::once(first).chain(
                table
                    .into_iter()
                    .map(move |row| exports::csv_line(&headers, &row)),
            );
            (
                "text/csv; charset=utf-8",
                axum::body::Body::from_stream(futures_util::stream::iter(
                    chunks.map(Ok::<_, std::convert::Infallible>),
                )),
            )
        }
        "xlsx" => (
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            axum::body::Body::from(exports::xlsx(&headers, &table)?),
        ),
        "pdf" => (
            "application/pdf",
            axum::body::Body::from(exports::pdf(name, &headers, &table, landscape)?),
        ),
        _ => return Err(MaintError::BadRequest("Format d'export inconnu".into()).into()),
    };
    exports::record(&s.pool, &session.actor, name, format, &p, rows.len()).await?;
    let filename = format!(
        "attachment; filename=\"{name}_{}.{}\"",
        chrono::Local::now().format("%Y-%m-%d"),
        format
    );
    Ok((
        [
            (header::CONTENT_TYPE, mime),
            (header::CONTENT_DISPOSITION, filename.as_str()),
        ],
        bytes,
    )
        .into_response())
}
async fn list_export(
    State(s): State<AppState>,
    Extension(session): Extension<Session>,
    Path(name): Path<String>,
    Query(p): Query<exports::Params>,
) -> Result<Response> {
    if name == "audit" && p.get("format").is_some_and(|s| s == "pdf") {
        return Err(MaintError::BadRequest("Le journal s'exporte en CSV ou Excel".into()).into());
    }
    let rows = exports::list(&s.pool, &session.actor, &name, &p).await?;
    download(&s, &session, &name, p, rows, true).await
}
async fn report_export(
    State(s): State<AppState>,
    Extension(session): Extension<Session>,
    Path(name): Path<String>,
    Query(mut p): Query<exports::Params>,
) -> Result<Response> {
    session.actor.require_permission(Permission::ExportsRun)?;
    let format = p.remove("format").unwrap_or("csv".into());
    let f = serde_json::from_value::<reports::Filter>(json!(p))
        .map_err(|_| MaintError::BadRequest("Filtres invalides".into()))?;
    let report = reports::report(&s.pool, &session.actor, &name, &f).await?;
    p.insert("format".into(), format);
    let mut rows = Vec::new();
    let mut summary = serde_json::Map::new();
    summary.insert("section".into(), json!("Synthèse"));
    for (key, value) in report["data"].as_object().expect("rapport objet") {
        if let Some(items) = value.as_array() {
            for item in items {
                let mut row = item.as_object().cloned().unwrap_or_default();
                row.insert("section".into(), json!(key));
                rows.push(Value::Object(row));
            }
        } else {
            summary.insert(key.clone(), value.clone());
        }
    }
    rows.insert(0, Value::Object(summary));
    download(&s, &session, &name, p, rows, false).await
}
async fn intervention_pdf(
    State(s): State<AppState>,
    Extension(session): Extension<Session>,
    Path(id): Path<String>,
) -> Result<Response> {
    session.actor.require_permission(Permission::ExportsRun)?;
    let intervention =
        apex_core::interventions::get_intervention(&s.pool, &session.actor, &id).await?;
    let mut information = json!(intervention);
    let mut rows = Vec::new();
    // Les pièces exigent leur permission de vue, y compris dans une fiche exportée.
    if session.actor.can(Permission::StockView) {
        let mut total = 0;
        for p in apex_core::stock::list_intervention_parts(&s.pool, &session.actor, &id).await? {
            let cost = p.quantity * p.unit_price_cents;
            total += cost;
            let mut row = json!(p);
            row["cost_cents"] = json!(cost);
            rows.push(row);
        }
        information["parts_cost_cents"] = json!(total);
    }
    if let Some(plan_id) = information["maintenance_plan"]["id"].as_str() {
        let base = settings::raw(&mut *s.pool.acquire().await?, "public_base_url").await?;
        information["maintenance_plan_url"] = json!(format!(
            "{}/#/maintenance/{plan_id}",
            base.trim_end_matches('/')
        ));
    }
    rows.insert(0, information);
    let mut p = BTreeMap::new();
    p.insert("format".into(), "pdf".into());
    p.insert("intervention_id".into(), id);
    download(&s, &session, "intervention", p, rows, false).await
}

#[cfg(test)]
mod tests {
    //! Les contrats HTTP restent vérifiables indépendamment des ACL du cycle de vie Windows.
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    struct HttpTest {
        port: u16,
        pool: sqlx::SqlitePool,
        task: tokio::task::JoinHandle<()>,
        _dir: tempfile::TempDir,
    }
    impl Drop for HttpTest {
        fn drop(&mut self) {
            self.task.abort();
        }
    }
    impl HttpTest {
        async fn start() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let pool = apex_core::db::open(None).await.unwrap();
            let auth = apex_core::services::Auth::new(pool.clone(), 8);
            auth.initialize(true).await.unwrap();
            let runtime = super::super::RuntimeInfo {
                instance_name: "Essai HTTP".into(),
                config_fingerprint: String::new(),
                config_file: String::new(),
                data_dir: String::new(),
                log_dir: String::new(),
                web_dir: String::new(),
                run_mode: "portable".into(),
                mode: "test".into(),
                pid: std::process::id(),
                started_at: chrono::Utc::now().to_rfc3339(),
                full_version: String::new(),
            };
            let test_pool = pool.clone();
            let state = AppState {
                widget_cache: Default::default(),
                files: apex_core::documents::storage::FileStore::new(dir.path().into(), [5; 32]),
                pool,
                auth,
                secure_cookie: false,
                runtime: std::sync::Arc::new(runtime),
                control_token: "controle-test".into(),
                shutdown_notify: Default::default(),
                started_at: chrono::Utc::now(),
            };
            let app = super::super::router(state, dir.path());
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = listener.local_addr().unwrap().port();
            assert!(![3000, 3100].contains(&port));
            let task = tokio::spawn(async move {
                axum::serve(
                    listener,
                    app.into_make_service_with_connect_info::<SocketAddr>(),
                )
                .await
                .unwrap();
            });
            Self {
                port,
                pool: test_pool,
                task,
                _dir: dir,
            }
        }
        async fn call(
            &self,
            method: &str,
            path: &str,
            cookie: &str,
            input: Option<Value>,
        ) -> (u16, String, Vec<u8>) {
            let mut socket = tokio::net::TcpStream::connect(("127.0.0.1", self.port))
                .await
                .unwrap();
            let payload = input.map(|v| v.to_string()).unwrap_or_default();
            let request=format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\nCookie: {cookie}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{payload}",self.port,payload.len());
            socket.write_all(request.as_bytes()).await.unwrap();
            let mut raw = Vec::new();
            socket.read_to_end(&mut raw).await.unwrap();
            let end = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
            let head = String::from_utf8(raw[..end].to_vec()).unwrap();
            let status = head.split_whitespace().nth(1).unwrap().parse().unwrap();
            let mut body = raw[end + 4..].to_vec();
            if head.to_lowercase().contains("transfer-encoding: chunked") {
                let mut pos = 0;
                let mut decoded = Vec::new();
                loop {
                    let end = body[pos..].windows(2).position(|w| w == b"\r\n").unwrap() + pos;
                    let len =
                        usize::from_str_radix(std::str::from_utf8(&body[pos..end]).unwrap(), 16)
                            .unwrap();
                    pos = end + 2;
                    if len == 0 {
                        break;
                    }
                    decoded.extend(&body[pos..pos + len]);
                    pos += len + 2;
                }
                body = decoded;
            }
            (status, head, body)
        }
        async fn json(
            &self,
            method: &str,
            path: &str,
            cookie: &str,
            input: Option<Value>,
            status: u16,
        ) -> Value {
            let r = self.call(method, path, cookie, input).await;
            assert_eq!(
                r.0,
                status,
                "{method} {path}: {}",
                String::from_utf8_lossy(&r.2)
            );
            if r.2.is_empty() {
                Value::Null
            } else {
                serde_json::from_slice(&r.2).unwrap()
            }
        }
        async fn login(&self, user: &str, password: &str) -> String {
            let r = self
                .call(
                    "POST",
                    "/api/auth/login",
                    "",
                    Some(json!({"username":user,"password":password})),
                )
                .await;
            assert_eq!(r.0, 200);
            r.1.lines()
                .find_map(|l| l.strip_prefix("set-cookie: "))
                .unwrap()
                .split(';')
                .next()
                .unwrap()
                .into()
        }
    }
    #[tokio::test]
    async fn contrats_http_pilotage_exports_et_isolement() {
        let h = HttpTest::start().await;
        let admin = h.login("admin", "admin123").await;
        h.json(
            "POST",
            "/api/auth/password",
            &admin,
            Some(json!({"current_password":"admin123","new_password":"Admin-http123"})),
            204,
        )
        .await;
        let resp = h.login("responsable", "demo1234").await;
        let tech = h.login("technicien", "demo1234").await;
        let layout = json!({"widgets":[{"id":"w","widget":"parts_cost","x":0,"y":0,"w":6,"h":3,"params":{"period":"30d"}}]});
        h.json(
            "PUT",
            "/api/permissions/technicien",
            &admin,
            Some(json!({"permissions":["dashboard.view","reporting.view","interventions.view"]})),
            204,
        )
        .await;
        h.json(
            "PUT",
            "/api/dashboard/defaults/technicien",
            &admin,
            Some(layout.clone()),
            200,
        )
        .await;
        let d = h.json("GET", "/api/dashboard", &tech, None, 200).await;
        assert_eq!(d["widgets"], json!([]));
        h.json("GET", "/api/dashboard/widgets/parts_cost", &tech, None, 403)
            .await;
        h.json("PUT", "/api/dashboard", &tech, Some(layout.clone()), 403)
            .await;
        assert_eq!(
            h.json("PUT", "/api/dashboard", &resp, Some(layout), 200)
                .await["source"],
            "user"
        );
        assert_eq!(
            h.json("DELETE", "/api/dashboard", &resp, None, 200).await["source"],
            "builtin"
        );
        let eq = h
            .json(
                "POST",
                "/api/equipments",
                &resp,
                Some(json!({"code":"HTTP-A","name":"=FORMULE"})),
                201,
            )
            .await;
        for format in ["csv", "xlsx", "pdf"] {
            let r = h
                .call(
                    "GET",
                    &format!("/api/exports/equipments?format={format}&q=HTTP-A"),
                    &resp,
                    None,
                )
                .await;
            assert_eq!(r.0, 200, "{}", String::from_utf8_lossy(&r.2));
            assert!(match format {
                "csv" => r.2.starts_with(&[239, 187, 191]),
                "xlsx" => r.2.starts_with(b"PK"),
                _ => r.2.starts_with(b"%PDF"),
            });
        }
        h.json("GET", "/api/exports/equipments", &tech, None, 403)
            .await;
        for report in ["activity", "reliability", "preventive", "costs", "stock"] {
            h.json("GET", &format!("/api/reports/{report}"), &resp, None, 200)
                .await;
        }
        let foreign = h
            .json(
                "POST",
                "/api/sites",
                &admin,
                Some(json!({"name":"Autre site"})),
                201,
            )
            .await;
        let foreign_eq = h
            .json(
                "POST",
                "/api/equipments",
                &admin,
                Some(json!({"site_id":foreign["id"],"code":"HTTP-B","name":"Autre"})),
                201,
            )
            .await;
        h.json(
            "GET",
            &format!(
                "/api/reports/activity?equipment_id={}",
                foreign_eq["id"].as_str().unwrap()
            ),
            &resp,
            None,
            404,
        )
        .await;
        let audit = h
            .json("GET", "/api/audit?entity=export&limit=1", &resp, None, 200)
            .await;
        assert_eq!(audit["items"].as_array().unwrap().len(), 1);
        assert!(audit["next_cursor"].is_string());
        h.json(
            "PUT",
            "/api/settings/smtp",
            &admin,
            Some(json!({"password":"secret-http"})),
            200,
        )
        .await;
        let audit = h
            .json("GET", "/api/audit?entity=settings", &admin, None, 200)
            .await;
        assert!(!audit.to_string().contains("secret-http"));
        h.json(
            "PUT",
            "/api/settings/security",
            &admin,
            Some(json!({"session_timeout_min":1})),
            400,
        )
        .await;
        assert_eq!(
            h.json("GET", "/api/public/branding", "", None, 200).await["password_reset_available"],
            false
        );
        h.json(
            "POST",
            "/api/auth/password-reset/request",
            "",
            Some(json!({"identifier":"inconnu"})),
            404,
        )
        .await;
        let i=h.json("POST","/api/interventions",&resp,Some(json!({"title":"Urgence","type":"corrective","priority":"critical","machine_state":"ARRET","equipment_id":eq["id"]})),201).await;
        let r = h
            .call(
                "GET",
                &format!("/api/interventions/{}/pdf", i["id"].as_str().unwrap()),
                &resp,
                None,
            )
            .await;
        assert_eq!(r.0, 200);
        assert!(r.2.starts_with(b"%PDF"));
    }

    #[tokio::test]
    async fn contrats_http_reset_uniforme_usage_unique_et_revocation() {
        let h = HttpTest::start().await;
        let admin = h.login("admin", "admin123").await;
        h.json(
            "POST",
            "/api/auth/password",
            &admin,
            Some(json!({"current_password":"admin123","new_password":"Admin-http123"})),
            204,
        )
        .await;
        let me = h.json("GET", "/api/auth/me", &admin, None, 200).await;
        let uid = me["user"]["id"].as_str().unwrap();
        h.json(
            "PATCH",
            &format!("/api/users/{uid}"),
            &admin,
            Some(json!({"email":"admin@example.test"})),
            200,
        )
        .await;
        h.json(
            "PUT",
            "/api/settings/general",
            &admin,
            Some(json!({"public_base_url":"https://maint.example.test"})),
            200,
        )
        .await;
        h.json("PUT","/api/settings/smtp",&admin,Some(json!({"host":"127.0.0.1","port":h.port,"security":"none","from_address":"maint@example.test","enabled":true})),200).await;
        let known = h
            .json(
                "POST",
                "/api/auth/password-reset/request",
                "",
                Some(json!({"identifier":"admin"})),
                202,
            )
            .await;
        let unknown = h
            .json(
                "POST",
                "/api/auth/password-reset/request",
                "",
                Some(json!({"identifier":"inconnu"})),
                202,
            )
            .await;
        assert_eq!(known, unknown);
        let outbox = h
            .json("GET", "/api/settings/smtp/outbox", &admin, None, 200)
            .await;
        assert!(!outbox.to_string().contains("jeton="));
        let text: String =
            sqlx::query_scalar("SELECT body_text FROM outbox_emails WHERE event='password_reset'")
                .fetch_one(&h.pool)
                .await
                .unwrap();
        let token = text.split("jeton=").nth(1).unwrap().trim();
        h.json(
            "POST",
            "/api/auth/password-reset/confirm",
            "",
            Some(json!({"token":token,"new_password":"Admin-reinitialise123"})),
            204,
        )
        .await;
        h.json("GET", "/api/auth/me", &admin, None, 401).await;
        h.json(
            "POST",
            "/api/auth/password-reset/confirm",
            "",
            Some(json!({"token":token,"new_password":"Admin-inutilise123"})),
            400,
        )
        .await;
        let new = h.login("admin", "Admin-reinitialise123").await;
        assert_eq!(
            h.json("GET", "/api/auth/me", &new, None, 200).await["must_change_password"],
            false
        );
    }
}
