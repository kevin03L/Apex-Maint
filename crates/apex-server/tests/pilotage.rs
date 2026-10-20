//! Contrats HTTP de la phase 7 sur des instances portables temporaires.
#[allow(dead_code)]
mod common;
use common::{Server, Settings};
use serde_json::{json, Value};

#[test]
fn adaptateur_json_pilotage_conserve_413() {
    let (s, admin, _) = setup("pilotage-json-413");
    let response = common::request_bytes(
        s.port(),
        "PUT",
        "/api/dashboard",
        None,
        &vec![b' '; 65537],
        &[("Cookie", &admin), ("Content-Type", "application/json")],
    )
    .unwrap();
    assert_eq!(response.status, 413, "{}", response.text());
}
fn login(s: &Server, name: &str, password: &str) -> String {
    let r = s.session_call(
        "POST",
        "/api/auth/login",
        None,
        Some(&json!({"username":name,"password":password})),
    );
    assert_eq!(r.status, 200, "{}", r.text());
    r.header("set-cookie")
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .into()
}
fn setup(name: &str) -> (Server, String, String) {
    let mut cfg = Settings::new();
    cfg.demo = true;
    let s = Server::start_with(name, cfg);
    let a = login(&s, "admin", "admin123");
    call(
        &s,
        &a,
        "POST",
        "/api/auth/password",
        Some(json!({"current_password":"admin123","new_password":"Admin-pilotage123"})),
        204,
    );
    let r = login(&s, "responsable", "demo1234");
    (s, a, r)
}
fn call(
    s: &Server,
    cookie: &str,
    method: &str,
    path: &str,
    input: Option<Value>,
    status: u16,
) -> Value {
    let r = s.session_call(method, path, Some(cookie), input.as_ref());
    assert_eq!(r.status, status, "{method} {path} : {}", r.text());
    if status == 204 || r.body.is_empty() {
        Value::Null
    } else {
        r.json()
    }
}
#[test]
fn dispositions_catalogue_permissions_et_cache() {
    let (s, a, r) = setup("pilotage-dispositions");
    let tech = login(&s, "technicien", "demo1234");
    call(
        &s,
        &a,
        "PUT",
        "/api/permissions/technicien",
        Some(json!({"permissions":["dashboard.view","interventions.view","reporting.view"]})),
        204,
    );
    let layout = json!({"widgets":[{"id":"stock","widget":"parts_cost","x":0,"y":0,"w":6,"h":3,"params":{"period":"30d"}}]});
    call(
        &s,
        &a,
        "PUT",
        "/api/dashboard/defaults/technicien",
        Some(layout.clone()),
        200,
    );
    let d = call(&s, &tech, "GET", "/api/dashboard", None, 200);
    assert_eq!(d["source"], "role");
    assert_eq!(d["widgets"], json!([]));
    call(
        &s,
        &tech,
        "GET",
        "/api/dashboard/widgets/parts_cost",
        None,
        403,
    );
    call(
        &s,
        &tech,
        "PUT",
        "/api/dashboard",
        Some(layout.clone()),
        403,
    );
    let catalog = call(&s, &tech, "GET", "/api/dashboard/catalog", None, 200);
    assert!(!catalog
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v["widget"] == "stock_value"));
    let d = call(&s, &r, "PUT", "/api/dashboard", Some(layout), 200);
    assert_eq!(d["source"], "user");
    let d = call(&s, &r, "DELETE", "/api/dashboard", None, 200);
    assert_eq!(d["source"], "builtin");
    let d = call(
        &s,
        &r,
        "GET",
        "/api/dashboard/widgets/interventions_summary",
        None,
        200,
    );
    let e = call(
        &s,
        &r,
        "GET",
        "/api/dashboard/widgets/interventions_summary",
        None,
        200,
    );
    assert_eq!(d, e);
    for path in [
        "/api/dashboard",
        "/api/reports/activity",
        "/api/audit",
        "/api/exports/parts",
        "/api/settings/smtp",
    ] {
        assert_eq!(s.session_call("GET", path, None, None).status, 401);
    }
}
#[test]
fn parametres_exports_audit_et_routes_publiques() {
    let (s, a, r) = setup("pilotage-api");
    let public = s.session_call("GET", "/api/public/branding", None, None);
    assert_eq!(public.status, 200);
    assert_eq!(public.json()["password_reset_available"], false);
    assert_eq!(
        s.session_call(
            "POST",
            "/api/auth/password-reset/request",
            None,
            Some(&json!({"identifier":"inconnu"}))
        )
        .status,
        404
    );
    assert_eq!(
        s.session_call(
            "POST",
            "/api/auth/password-reset/confirm",
            None,
            Some(&json!({"token":"inconnu","new_password":"Nouveau123"}))
        )
        .status,
        404
    );
    call(
        &s,
        &a,
        "PUT",
        "/api/settings/security",
        Some(json!({"session_timeout_min":1})),
        400,
    );
    call(
        &s,
        &a,
        "PUT",
        "/api/settings/audit",
        Some(json!({"retention_days":100})),
        400,
    );
    call(
        &s,
        &a,
        "PUT",
        "/api/settings/smtp",
        Some(json!({"password":"secret-pilotage"})),
        200,
    );
    let smtp = call(&s, &a, "GET", "/api/settings/smtp", None, 200);
    assert_eq!(smtp["has_password"], true);
    assert!(smtp.get("password").is_none());
    call(
        &s,
        &a,
        "PUT",
        "/api/settings/general",
        Some(json!({"app_name":"Atelier","public_base_url":"https://maint.example.test"})),
        200,
    );
    assert_eq!(
        s.session_call("GET", "/api/public/branding", None, None)
            .json()["app_name"],
        "Atelier"
    );
    let site = call(&s, &r, "GET", "/api/auth/me", None, 200)["user"]["site_id"]
        .as_str()
        .unwrap()
        .to_string();
    call(
        &s,
        &r,
        "POST",
        "/api/equipments",
        Some(json!({"code":"EXP-A","name":"=FORMULE"})),
        201,
    );
    call(
        &s,
        &a,
        "POST",
        "/api/equipments",
        Some(json!({"site_id":site,"code":"EXP-B","name":"Autre"})),
        201,
    );
    for (format, magic) in [
        ("csv", &[239, 187, 191][..]),
        ("xlsx", &b"PK"[..]),
        ("pdf", &b"%PDF"[..]),
    ] {
        let response = s.session_call(
            "GET",
            &format!("/api/exports/equipments?format={format}&q=EXP-A"),
            Some(&r),
            None,
        );
        assert_eq!(response.status, 200, "{}", response.text());
        assert!(response.body.starts_with(magic));
        assert!(response
            .header("content-disposition")
            .unwrap()
            .starts_with("attachment;"));
        if format == "csv" {
            assert!(response.text().contains("'=FORMULE"));
            assert!(!response.text().contains("EXP-B"));
        }
    }
    let tech = login(&s, "technicien", "demo1234");
    call(
        &s,
        &tech,
        "GET",
        "/api/exports/equipments?format=csv",
        None,
        403,
    );
    let audit = call(&s, &r, "GET", "/api/audit?entity=export&limit=1", None, 200);
    assert_eq!(audit["items"].as_array().unwrap().len(), 1);
    let cursor = audit["next_cursor"].as_str().unwrap();
    let page = call(
        &s,
        &r,
        "GET",
        &format!("/api/audit?entity=export&limit=1&cursor={cursor}"),
        None,
        200,
    );
    assert_ne!(page["items"][0]["id"], audit["items"][0]["id"]);
    let id = audit["items"][0]["id"].as_str().unwrap();
    call(&s, &r, "GET", &format!("/api/audit/{id}"), None, 200);
    let settings_audit = call(&s, &a, "GET", "/api/audit?entity=settings", None, 200);
    assert!(!settings_audit.to_string().contains("secret-pilotage"));
    for name in ["activity", "reliability", "preventive", "costs", "stock"] {
        call(&s, &r, "GET", &format!("/api/reports/{name}"), None, 200);
        let response = s.session_call(
            "GET",
            &format!("/api/reports/{name}/export?format=pdf"),
            Some(&r),
            None,
        );
        assert_eq!(response.status, 200, "{}", response.text());
        assert!(response.body.starts_with(b"%PDF"));
    }
    let response = common::request_with_headers(
        s.port(),
        "POST",
        "/api/auth/password-reset/request",
        None,
        Some(&json!({"identifier":"inconnu"})),
        &[("Origin", "https://autre.test")],
    )
    .unwrap();
    assert_eq!(response.status, 403);
}

#[test]
fn reinitialisation_par_smtp_local_et_reponse_uniforme() {
    use std::io::{BufRead, Write};
    let (s, a, _) = setup("pilotage-reset-smtp");
    let me = call(&s, &a, "GET", "/api/auth/me", None, 200);
    let uid = me["user"]["id"].as_str().unwrap();
    call(
        &s,
        &a,
        "PATCH",
        &format!("/api/users/{uid}"),
        Some(json!({"email":"admin@example.test"})),
        200,
    );
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    assert!(![3000, 3100].contains(&port));
    listener.set_nonblocking(true).unwrap();
    let smtp = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(40);
        let mut socket = loop {
            match listener.accept() {
                Ok((s, _)) => break s,
                Err(e)
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        && std::time::Instant::now() < deadline =>
                {
                    std::thread::sleep(std::time::Duration::from_millis(50))
                }
                Err(e) => panic!("SMTP local : {e}"),
            }
        };
        // Sous Windows, la connexion acceptée hérite du mode non bloquant de l'écouteur.
        socket.set_nonblocking(false).unwrap();
        socket
            .set_read_timeout(Some(std::time::Duration::from_secs(10)))
            .unwrap();
        socket.write_all(b"220 local.test ESMTP\r\n").unwrap();
        let mut reader = std::io::BufReader::new(socket.try_clone().unwrap());
        let mut data = false;
        let mut message = String::new();
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap() == 0 {
                break;
            }
            if data {
                if line == ".\r\n" {
                    data = false;
                    socket.write_all(b"250 recu\r\n").unwrap();
                } else {
                    message.push_str(&line);
                }
            } else if line.starts_with("EHLO") {
                socket
                    .write_all(b"250-local.test\r\n250 8BITMIME\r\n")
                    .unwrap();
            } else if line.starts_with("DATA") {
                data = true;
                socket.write_all(b"354 contenu\r\n").unwrap();
            } else if line.starts_with("QUIT") {
                socket.write_all(b"221 termine\r\n").unwrap();
                break;
            } else {
                socket.write_all(b"250 ok\r\n").unwrap();
            }
        }
        message
    });
    call(
        &s,
        &a,
        "PUT",
        "/api/settings/general",
        Some(json!({"public_base_url":"https://maint.example.test"})),
        200,
    );
    call(
        &s,
        &a,
        "PUT",
        "/api/settings/smtp",
        Some(
            json!({"host":"127.0.0.1","port":port,"security":"none","from_address":"maint@example.test","enabled":true}),
        ),
        200,
    );
    let known = s.session_call(
        "POST",
        "/api/auth/password-reset/request",
        None,
        Some(&json!({"identifier":"admin"})),
    );
    let unknown = s.session_call(
        "POST",
        "/api/auth/password-reset/request",
        None,
        Some(&json!({"identifier":"inconnu"})),
    );
    assert_eq!(known.status, 202);
    assert_eq!(unknown.status, 202);
    assert_eq!(known.body, unknown.body);
    let message = smtp
        .join()
        .unwrap()
        .replace("=\r\n", "")
        .replace("=3D", "=");
    let token = message
        .split("jeton=")
        .nth(1)
        .unwrap()
        .chars()
        .take_while(char::is_ascii_hexdigit)
        .collect::<String>();
    assert_eq!(token.len(), 64);
    assert!(!call(&s, &a, "GET", "/api/settings/smtp/outbox", None, 200)
        .to_string()
        .contains(&token));
    assert_eq!(
        s.session_call(
            "POST",
            "/api/auth/password-reset/confirm",
            None,
            Some(&json!({"token":token,"new_password":"Admin-reset123"}))
        )
        .status,
        204
    );
    assert_eq!(
        s.session_call("GET", "/api/auth/me", Some(&a), None).status,
        401
    );
    assert_eq!(
        s.session_call(
            "POST",
            "/api/auth/password-reset/confirm",
            None,
            Some(&json!({"token":token,"new_password":"Admin-reset456"}))
        )
        .status,
        400
    );
    login(&s, "admin", "Admin-reset123");
}

#[test]
fn purge_audit_quotidienne_acceleree_sur_instance_temporaire() {
    use sqlx::Connection;
    let s = Server::start_with_env(
        "pilotage-purge",
        Settings::new(),
        &[("APEX_MAINT_TEST_AUDIT_PURGE_SECONDS", "1")],
    );
    let a = login(&s, "admin", "admin123");
    call(
        &s,
        &a,
        "POST",
        "/api/auth/password",
        Some(json!({"current_password":"admin123","new_password":"Admin-purge123"})),
        204,
    );
    let file = s.dir().join("data").join("apex-maint.db");
    tokio::runtime::Runtime::new().unwrap().block_on(async {
  let mut conn=sqlx::SqliteConnection::connect_with(&sqlx::sqlite::SqliteConnectOptions::new().filename(file)).await.unwrap();
  sqlx::query("INSERT INTO audit_log(id,action,entity,created_at) VALUES('ancien-test','create','essai','2000-01-01T00:00:00.000Z')").execute(&mut conn).await.unwrap();
 });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let entries = call(
            &s,
            &a,
            "GET",
            "/api/audit?entity=audit&action=purge",
            None,
            200,
        );
        if entries["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["after"]["deleted_count"].as_i64().unwrap_or(0) > 0)
        {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "Purge non exécutée");
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    call(&s, &a, "GET", "/api/audit/ancien-test", None, 404);
}
