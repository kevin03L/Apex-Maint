use super::*;
use tower::ServiceExt;

/// Appel du routeur réel sur base en mémoire, sans serveur ni port d'exploitation.
async fn appel_conditionne(
    app: &axum::Router,
    method: &str,
    path: &str,
    body: serde_json::Value,
    headers: &[(&str, &str)],
) -> (StatusCode, serde_json::Value, Option<String>) {
    let mut request = axum::http::Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json")
        .header("host", "localhost");
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let mut request = request
        .body(axum::body::Body::from(body.to_string()))
        .unwrap();
    request.extensions_mut().insert(ConnectInfo(
        "127.0.0.1:12345".parse::<SocketAddr>().unwrap(),
    ));
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let cookie = response
        .headers()
        .get("set-cookie")
        .map(|v| v.to_str().unwrap().split(';').next().unwrap().to_string());
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, body, cookie)
}

#[tokio::test]
async fn routes_intervention_preconditions_et_version_du_temps() {
    use serde_json::json;
    let dir = tempfile::tempdir().unwrap();
    let (state, pool) = etat(dir.path()).await;
    let app = router(state, dir.path());
    let (status, _, cookie) = appel_conditionne(
        &app,
        "POST",
        "/api/auth/login",
        json!({"username":"admin","password":"admin123"}),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let cookie = cookie.unwrap();
    let auth = [("Cookie", cookie.as_str())];
    let (status, _, _) = appel_conditionne(
        &app,
        "POST",
        "/api/auth/password",
        json!({"current_password":"admin123","new_password":"Terrain-admin123!"}),
        &auth,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, site, _) =
        appel_conditionne(&app, "POST", "/api/sites", json!({"name":"Terrain"}), &auth).await;
    let (status, initial, _) = appel_conditionne(
        &app,
        "POST",
        "/api/interventions",
        json!({"site_id":site["id"],"title":"Panne","type":"hors_equipement"}),
        &auth,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{initial}");
    let path = format!("/api/interventions/{}", initial["id"].as_str().unwrap());
    let user: String = sqlx::query_scalar("SELECT id FROM users WHERE username='admin'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let time_path = format!("{path}/technicians/{user}/time");
    // Simuler une modification tierce après l'instantané, avec une version distincte déterministe.
    sqlx::query("UPDATE interventions SET description='Distante', updated_at='2026-01-01T00:00:00.000Z' WHERE id=?")
        .bind(initial["id"].as_str().unwrap()).execute(&pool).await.unwrap();
    let stale = [
        ("Cookie", cookie.as_str()),
        (
            "If-Unmodified-Since",
            initial["updated_at"].as_str().unwrap(),
        ),
    ];
    for (method, route, body) in [
        ("POST", format!("{path}/take"), json!({})),
        ("PUT", time_path.clone(), json!({"duration_minutes":30})),
        (
            "POST",
            format!("{path}/complete"),
            json!({"solution_report":"Réparé"}),
        ),
        ("PATCH", path.clone(), json!({"description":"Locale"})),
    ] {
        let (status, error, _) = appel_conditionne(&app, method, &route, body, &stale).await;
        assert_eq!(status, StatusCode::PRECONDITION_FAILED, "{route}: {error}");
    }
    let fresh = [
        ("Cookie", cookie.as_str()),
        ("If-Unmodified-Since", "2026-01-01T00:00:00.000Z"),
    ];
    let (status, taken, _) =
        appel_conditionne(&app, "POST", &format!("{path}/take"), json!({}), &fresh).await;
    assert_eq!(status, StatusCode::OK, "{taken}");
    assert_eq!(taken["description"], "Distante");
    let key = uuid::Uuid::now_v7().to_string();
    let time_headers = [
        ("Cookie", cookie.as_str()),
        ("If-Unmodified-Since", taken["updated_at"].as_str().unwrap()),
        ("Idempotency-Key", key.as_str()),
    ];
    let (status, time, _) = appel_conditionne(
        &app,
        "PUT",
        &time_path,
        json!({"duration_minutes":30}),
        &time_headers,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{time}");
    assert_eq!(time["intervention_id"], initial["id"]);
    assert_eq!(time["user_id"], user);
    assert_eq!(time["duration_minutes"], 30);
    let version: String = sqlx::query_scalar("SELECT updated_at FROM interventions WHERE id=?")
        .bind(initial["id"].as_str().unwrap())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(time["updated_at"], version);
    let fresh = [
        ("Cookie", cookie.as_str()),
        ("If-Unmodified-Since", version.as_str()),
    ];
    let (status, patched, _) = appel_conditionne(
        &app,
        "PATCH",
        &path,
        json!({"description":"Locale"}),
        &fresh,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{patched}");
    let (status, replay, _) = appel_conditionne(
        &app,
        "PUT",
        &time_path,
        json!({"duration_minutes":30}),
        &time_headers,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(replay, time);
    let fresh = [
        ("Cookie", cookie.as_str()),
        (
            "If-Unmodified-Since",
            patched["updated_at"].as_str().unwrap(),
        ),
    ];
    let (status, completed, _) = appel_conditionne(
        &app,
        "POST",
        &format!("{path}/complete"),
        json!({"solution_report":"Réparé"}),
        &fresh,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{completed}");
    // La compatibilité sans précondition est conservée, y compris pour les nouvelles routes protégées.
    let (_, item, _) = appel_conditionne(
        &app,
        "POST",
        "/api/interventions",
        json!({"site_id":site["id"],"title":"En ligne","type":"hors_equipement"}),
        &auth,
    )
    .await;
    let path = format!("/api/interventions/{}", item["id"].as_str().unwrap());
    for (method, route, body) in [
        ("POST", format!("{path}/take"), json!({})),
        (
            "PUT",
            format!("{path}/technicians/{user}/time"),
            json!({"duration_minutes":10}),
        ),
        ("PATCH", path.clone(), json!({"description":"En ligne"})),
        (
            "POST",
            format!("{path}/complete"),
            json!({"solution_report":"Réparé"}),
        ),
    ] {
        let (status, body, _) = appel_conditionne(&app, method, &route, body, &auth).await;
        assert_eq!(status, StatusCode::OK, "{route}: {body}");
    }
    pool.close().await;
}

/// État minimal d'un routeur sans instance, sur une base en mémoire.
async fn etat(dir: &std::path::Path) -> (AppState, sqlx::SqlitePool) {
    let pool = apex_core::db::open(None).await.unwrap();
    let auth = apex_core::services::Auth::new(pool.clone(), 24);
    auth.initialize(false).await.unwrap();
    let runtime = RuntimeInfo {
        instance_name: "Essai".into(),
        config_fingerprint: String::new(),
        config_file: String::new(),
        data_dir: dir.display().to_string(),
        log_dir: String::new(),
        web_dir: String::new(),
        run_mode: "manual".into(),
        mode: "portable".into(),
        pid: 0,
        started_at: String::new(),
        full_version: String::new(),
    };
    let state = AppState {
        integration_rates: Default::default(),
        widget_cache: Default::default(),
        tls_mode: "off".into(),
        trusted_proxies: Vec::new(),
        hsts_max_age: 86400,
        files: apex_core::documents::storage::FileStore::new(dir.into(), [0; 32]),
        pool: pool.clone(),
        auth,
        secure_cookie: false,
        runtime: Arc::new(runtime),
        control_token: "local".into(),
        shutdown_notify: Arc::new(tokio::sync::Notify::new()),
        started_at: Utc::now(),
        terrain_lock: Arc::new(tokio::sync::Mutex::new(())),
        files_max_size_mb: 50,
    };
    (state, pool)
}

#[tokio::test]
async fn proxy_de_confiance_cookie_secure_et_hsts_sans_instance() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, pool) = etat(dir.path()).await;
    for (trusted, tls, expected) in [
        (false, false, false),
        (true, false, true),
        (false, true, true),
    ] {
        state.trusted_proxies = if trusted {
            vec!["127.0.0.1".parse().unwrap()]
        } else {
            Vec::new()
        };
        state.tls_mode = if tls { "files" } else { "off" }.into();
        let router = router(state.clone(), dir.path());
        let mut request = axum::http::Request::builder()
            .method("POST")
            .uri("/api/auth/login")
            .header("content-type", "application/json")
            .header("host", "localhost")
            .header("x-forwarded-proto", "https")
            .header("x-forwarded-for", "203.0.113.5")
            .body(axum::body::Body::from(
                r#"{"username":"admin","password":"admin123"}"#,
            ))
            .unwrap();
        request.extensions_mut().insert(ConnectInfo(
            "127.0.0.1:12345".parse::<SocketAddr>().unwrap(),
        ));
        let response = router.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let cookie = response
            .headers()
            .get("set-cookie")
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(cookie.contains("; Secure"), expected);
        assert!(cookie.contains("HttpOnly"));
        assert_eq!(
            response.headers().contains_key("strict-transport-security"),
            expected
        );
    }
    pool.close().await;
}

/// HTTP/2 : pas d'en-tête Host, l'hôte est dans l'URI (:authority). Une Origin du même hôte
/// est acceptée, une autre refusée.
#[tokio::test]
async fn origine_controlee_sans_en_tete_host_en_http2() {
    let dir = tempfile::tempdir().unwrap();
    let (state, pool) = etat(dir.path()).await;
    for (origin, expected) in [
        ("https://maint.example", StatusCode::OK),
        ("https://autre.example", StatusCode::FORBIDDEN),
    ] {
        let mut request = axum::http::Request::builder()
            .method("POST")
            .version(axum::http::Version::HTTP_2)
            .uri("https://maint.example/api/auth/login")
            .header("content-type", "application/json")
            .header("origin", origin)
            .body(axum::body::Body::from(
                r#"{"username":"admin","password":"admin123"}"#,
            ))
            .unwrap();
        request.extensions_mut().insert(ConnectInfo(
            "127.0.0.1:12345".parse::<SocketAddr>().unwrap(),
        ));
        let response = router(state.clone(), dir.path())
            .oneshot(request)
            .await
            .unwrap();
        assert_eq!(response.status(), expected, "{origin}");
    }
    pool.close().await;
}
