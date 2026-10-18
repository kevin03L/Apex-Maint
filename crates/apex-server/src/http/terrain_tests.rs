use super::*;
use tower::ServiceExt;

#[tokio::test]
async fn proxy_de_confiance_cookie_secure_et_hsts_sans_instance() {
    let pool = apex_core::db::open(None).await.unwrap();
    let auth = apex_core::services::Auth::new(pool.clone(), 24);
    auth.initialize(false).await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let runtime = RuntimeInfo {
        instance_name: "Essai".into(),
        config_fingerprint: String::new(),
        config_file: String::new(),
        data_dir: dir.path().display().to_string(),
        log_dir: String::new(),
        web_dir: String::new(),
        run_mode: "manual".into(),
        mode: "portable".into(),
        pid: 0,
        started_at: String::new(),
        full_version: String::new(),
    };
    let mut state = AppState {
        integration_rates: Default::default(),
        widget_cache: Default::default(),
        tls_mode: "off".into(),
        trusted_proxies: Vec::new(),
        hsts_max_age: 86400,
        files: apex_core::documents::storage::FileStore::new(dir.path().into(), [0; 32]),
        pool: pool.clone(),
        auth,
        secure_cookie: false,
        runtime: Arc::new(runtime),
        control_token: "local".into(),
        shutdown_notify: Arc::new(tokio::sync::Notify::new()),
        started_at: Utc::now(),
        terrain_lock: Arc::new(tokio::sync::Mutex::new(())),
    };
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
