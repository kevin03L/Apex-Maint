//! Contrats HTTP de l'authentification sur des instances portables temporaires.
#[allow(dead_code)]
mod common;

use common::{request_with_headers, Server, Settings};
use serde_json::{json, Value};

fn login(srv: &Server, name: &str, password: &str) -> (String, Value) {
    let r = srv.session_call(
        "POST",
        "/api/auth/login",
        None,
        Some(&json!({"username":name,"password":password})),
    );
    assert_eq!(r.status, 200, "{}", r.text());
    assert!(r.json().get("token").is_none());
    let cookie = r.header("set-cookie").expect("cookie de connexion");
    assert!(
        cookie.contains("HttpOnly")
            && cookie.contains("SameSite=Strict")
            && cookie.contains("Path=/")
    );
    assert!(!cookie.contains("Max-Age"));
    (
        cookie.split(';').next().unwrap().to_owned(),
        r.json()["user"].clone(),
    )
}
fn admin(srv: &Server) -> (String, Value) {
    let (cookie, user) = login(srv, "admin", "admin123");
    let r = srv.session_call(
        "POST",
        "/api/auth/password",
        Some(&cookie),
        Some(&json!({"current_password":"admin123","new_password":"Admin-renouvele1"})),
    );
    assert_eq!(r.status, 204, "{}", r.text());
    (cookie, user)
}
fn demo(name: &str) -> Server {
    let mut cfg = Settings::new();
    cfg.demo = true;
    Server::start_with(name, cfg)
}

#[test]
fn connexion_changement_impose_et_protection_des_routes() {
    let srv = Server::start("auth-connexion");
    for path in [
        "/api/auth/me",
        "/api/users",
        "/api/sites",
        "/api/permissions",
        "/api/users/inconnu",
    ] {
        assert_eq!(
            srv.session_call("GET", path, None, None).status,
            401,
            "{path}"
        );
    }
    for name in ["admin", "inconnu"] {
        let r = srv.session_call(
            "POST",
            "/api/auth/login",
            None,
            Some(&json!({"username":name,"password":"incorrect"})),
        );
        assert_eq!(r.status, 401);
        assert!(r.json()["message"]
            .as_str()
            .unwrap()
            .contains("Identifiant ou mot de passe incorrect"));
    }
    let (cookie, _) = login(&srv, "AdMiN", "admin123");
    let me = srv
        .session_call("GET", "/api/auth/me", Some(&cookie), None)
        .json();
    assert_eq!(me["must_change_password"], true);
    assert_eq!(me["permissions"].as_array().unwrap().len(), 34);
    assert!(me["user"].get("password_hash").is_none());
    assert!(me["user"].get("failed_attempts").is_none());
    assert!(me["user"].get("locked_until").is_none());
    let r = srv.session_call("GET", "/api/users", Some(&cookie), None);
    assert_eq!(r.status, 403);
    assert_eq!(r.json()["code"], "password_change_required");
    for password in ["court", "admin123"] {
        assert_eq!(
            srv.session_call(
                "POST",
                "/api/auth/password",
                Some(&cookie),
                Some(&json!({"current_password":"admin123","new_password":password}))
            )
            .status,
            400
        );
    }
    assert_eq!(
        srv.session_call(
            "POST",
            "/api/auth/password",
            Some(&cookie),
            Some(&json!({"current_password":"admin123","new_password":"Admin-nouveau1"}))
        )
        .status,
        204
    );
    assert_eq!(
        srv.session_call("GET", "/api/users", Some(&cookie), None)
            .status,
        200
    );
}

#[test]
fn verrouillage_concurrent_et_expiration() {
    let srv = demo("auth-verrou");
    let port = srv.port();
    let barrier = std::sync::Barrier::new(10);
    let statuses = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..10)
            .map(|_| {
                let barrier = &barrier;
                scope.spawn(move || {
                    barrier.wait();
                    common::request(
                        port,
                        "POST",
                        "/api/auth/login",
                        None,
                        Some(&json!({"username":"technicien","password":"incorrect"})),
                    )
                    .unwrap()
                    .status
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(statuses.iter().filter(|&&s| s == 401).count(), 5);
    assert_eq!(statuses.iter().filter(|&&s| s == 429).count(), 5);
    let locked = srv.session_call(
        "POST",
        "/api/auth/login",
        None,
        Some(&json!({"username":"technicien","password":"demo1234"})),
    );
    assert_eq!(locked.status, 429);
    assert!(locked.json()["message"].as_str().unwrap().contains("s"));
    std::thread::sleep(std::time::Duration::from_millis(3200));
    login(&srv, "technicien", "demo1234");
}

#[test]
fn revocation_deconnexion_changement_desactivation_et_reset() {
    let srv = demo("auth-revocation");
    let (admin, _) = admin(&srv);
    let (first, user) = login(&srv, "technicien", "demo1234");
    let (second, _) = login(&srv, "technicien", "demo1234");
    assert_eq!(
        srv.session_call(
            "POST",
            "/api/auth/password",
            Some(&first),
            Some(&json!({"current_password":"demo1234","new_password":"Technicien-neuf1"}))
        )
        .status,
        204
    );
    assert_eq!(
        srv.session_call("GET", "/api/auth/me", Some(&second), None)
            .status,
        401
    );
    assert_eq!(
        srv.session_call("GET", "/api/auth/me", Some(&first), None)
            .status,
        200
    );
    let r = srv.session_call("POST", "/api/auth/logout", Some(&first), None);
    assert_eq!(r.status, 204);
    assert!(r.header("set-cookie").unwrap().contains("Max-Age=0"));
    assert_eq!(
        srv.session_call("GET", "/api/auth/me", Some(&first), None)
            .status,
        401
    );
    let (cookie, _) = login(&srv, "technicien", "Technicien-neuf1");
    let path = format!("/api/users/{}", user["id"].as_str().unwrap());
    assert_eq!(
        srv.session_call("PATCH", &path, Some(&admin), Some(&json!({"active":false})))
            .status,
        200
    );
    assert_eq!(
        srv.session_call("GET", "/api/auth/me", Some(&cookie), None)
            .status,
        401
    );
    assert_eq!(
        srv.session_call(
            "POST",
            "/api/auth/login",
            None,
            Some(&json!({"username":"technicien","password":"Technicien-neuf1"}))
        )
        .status,
        401
    );
    assert_eq!(
        srv.session_call("PATCH", &path, Some(&admin), Some(&json!({"active":true})))
            .status,
        200
    );
    let (cookie, _) = login(&srv, "technicien", "Technicien-neuf1");
    assert_eq!(
        srv.session_call(
            "POST",
            &format!("{path}/password"),
            Some(&admin),
            Some(&json!({"new_password":"Reinitialise-123"}))
        )
        .status,
        204
    );
    assert_eq!(
        srv.session_call("GET", "/api/auth/me", Some(&cookie), None)
            .status,
        401
    );
    let (cookie, _) = login(&srv, "technicien", "Reinitialise-123");
    assert_eq!(
        srv.session_call("GET", "/api/auth/me", Some(&cookie), None)
            .json()["must_change_password"],
        true
    );
}

#[test]
fn isolation_sites_permissions_et_garde_fous() {
    let srv = demo("auth-sites");
    let (admin, admin_user) = admin(&srv);
    let (responsable, _) = login(&srv, "responsable", "demo1234");
    let (technicien, _) = login(&srv, "technicien", "demo1234");
    assert_eq!(
        srv.session_call("GET", "/api/users", Some(&technicien), None)
            .status,
        403
    );
    let site = srv.session_call(
        "POST",
        "/api/sites",
        Some(&admin),
        Some(&json!({"name":"Site B"})),
    );
    assert_eq!(site.status, 201);
    let sid = site.json()["id"].as_str().unwrap().to_owned();
    let created=srv.session_call("POST","/api/users",Some(&admin),Some(&json!({"username":"autre-site","display_name":"Autre site","role":"production","site_id":sid,"password":"Compte-site-b1"})));
    assert_eq!(created.status, 201, "{}", created.text());
    let uid = created.json()["id"].as_str().unwrap().to_owned();
    let list = srv
        .session_call("GET", "/api/users", Some(&responsable), None)
        .json();
    assert!(list.as_array().unwrap().iter().all(|u| u["id"] != uid));
    for method in ["GET", "PATCH", "DELETE"] {
        let payload = json!({"display_name":"Tentative"});
        let r = srv.session_call(
            method,
            &format!("/api/users/{uid}"),
            Some(&responsable),
            if method == "PATCH" {
                Some(&payload)
            } else {
                None
            },
        );
        assert_eq!(r.status, 404, "{method}: {}", r.text());
    }
    let site_list = srv
        .session_call("GET", "/api/sites", Some(&responsable), None)
        .json();
    assert_eq!(site_list.as_array().unwrap().len(), 1);
    assert_eq!(
        srv.session_call("DELETE", &format!("/api/sites/{sid}"), Some(&admin), None)
            .status,
        409
    );
    for payload in [json!({"role":"production"}), json!({"active":false})] {
        assert_eq!(
            srv.session_call(
                "PATCH",
                &format!("/api/users/{}", admin_user["id"].as_str().unwrap()),
                Some(&admin),
                Some(&payload)
            )
            .status,
            409
        );
    }
    assert_eq!(
        srv.session_call(
            "DELETE",
            &format!("/api/users/{}", admin_user["id"].as_str().unwrap()),
            Some(&admin),
            None
        )
        .status,
        409
    );
    assert_eq!(
        srv.session_call("GET", "/api/permissions", Some(&responsable), None)
            .status,
        403
    );
    let r = srv.session_call("GET", "/api/permissions", Some(&admin), None);
    assert_eq!(
        r.json()["matrix"]["responsable"].as_array().unwrap().len(),
        33
    );
    assert_eq!(
        srv.session_call(
            "PUT",
            "/api/permissions/admin",
            Some(&admin),
            Some(&json!({"permissions":[]}))
        )
        .status,
        400
    );
    assert_eq!(
        srv.session_call(
            "PUT",
            "/api/permissions/production",
            Some(&admin),
            Some(&json!({"permissions":["inconnue.view"]}))
        )
        .status,
        400
    );
    assert_eq!(
        srv.session_call(
            "PUT",
            "/api/permissions/technicien",
            Some(&admin),
            Some(&json!({"permissions":["users.manage"]}))
        )
        .status,
        204
    );
    let me = srv
        .session_call("GET", "/api/auth/me", Some(&technicien), None)
        .json();
    assert_eq!(me["permissions"], json!(["users.manage"]));
    let own_site = site_list[0]["id"].clone();
    let created=srv.session_call("POST","/api/users",Some(&technicien),Some(&json!({"username":"gestion-seule","display_name":"Gestion","role":"production","site_id":own_site,"password":"Gestion-seule1"})));
    assert_eq!(created.status, 201, "{}", created.text());
    assert_eq!(srv.session_call("POST","/api/users",Some(&responsable),Some(&json!({"username":"admin-interdit","display_name":"Interdit","role":"admin","site_id":own_site,"password":"Interdit-123"}))).status,403);
}

#[test]
fn csrf_tailles_json_et_cookie_secure() {
    let srv = Server::start("auth-csrf");
    let (cookie, _) = admin(&srv);
    for headers in [
        vec![("Origin", "http://evil.example")],
        vec![("Sec-Fetch-Site", "cross-site")],
        vec![("Sec-Fetch-Site", "same-site")],
        vec![("Origin", "null")],
    ] {
        let mut headers = headers;
        headers.push(("Cookie", &cookie));
        let r = request_with_headers(srv.port(), "POST", "/api/auth/logout", None, None, &headers)
            .unwrap();
        assert_eq!(r.status, 403);
    }
    assert_eq!(
        srv.session_call(
            "POST",
            "/api/sites",
            Some(&cookie),
            Some(&json!({"name":"x","inconnu":true}))
        )
        .status,
        400
    );
    let too_big = json!({"name":"x".repeat(65*1024)});
    assert_eq!(
        srv.session_call("POST", "/api/sites", Some(&cookie), Some(&too_big))
            .status,
        413
    );
    let origin = format!("http://127.0.0.1:{}", srv.port());
    let headers = [
        ("Origin", origin.as_str()),
        ("Sec-Fetch-Site", "same-origin"),
        ("Cookie", cookie.as_str()),
    ];
    assert_eq!(
        request_with_headers(srv.port(), "POST", "/api/auth/logout", None, None, &headers)
            .unwrap()
            .status,
        204
    );
    let cfg = Settings::new();
    let dir = common::TempDir::new("auth-secure");
    let config = common::write_config(
        &dir.path,
        &format!("{}\n[security]\nsecure_cookie = true\n", cfg.render()),
    );
    let mut child = common::ChildGuard(Some(common::spawn_engine(&dir.path, &config, "secure")));
    let deadline = std::time::Instant::now() + common::STARTUP_TIMEOUT;
    loop {
        if let Ok(r) = common::request(
            cfg.port,
            "POST",
            "/api/auth/login",
            None,
            Some(&json!({"username":"admin","password":"admin123"})),
        ) {
            assert_eq!(r.status, 200);
            assert!(r.header("set-cookie").unwrap().contains("; Secure"));
            break;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    child.kill();
}

#[test]
fn session_persistante_et_demo_non_recreee() {
    let mut srv = demo("auth-persistance");
    let (admin, _) = admin(&srv);
    let (cookie, user) = login(&srv, "production", "demo1234");
    srv.restart();
    assert_eq!(
        srv.session_call("GET", "/api/auth/me", Some(&cookie), None)
            .status,
        200
    );
    assert_eq!(
        srv.session_call("GET", "/api/auth/me", Some(&admin), None)
            .status,
        200
    );
    assert_eq!(
        srv.session_call(
            "DELETE",
            &format!("/api/users/{}", user["id"].as_str().unwrap()),
            Some(&admin),
            None
        )
        .status,
        204
    );
    srv.restart();
    assert_eq!(
        srv.session_call(
            "POST",
            "/api/auth/login",
            None,
            Some(&json!({"username":"production","password":"demo1234"}))
        )
        .status,
        401
    );
    assert_eq!(
        srv.session_call("GET", "/api/sites", Some(&admin), None)
            .json()
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn changements_de_role_de_site_et_suppression_revoquent() {
    let srv = demo("auth-droits");
    let (admin, _) = admin(&srv);
    let (cookie, user) = login(&srv, "production", "demo1234");
    let path = format!("/api/users/{}", user["id"].as_str().unwrap());
    assert_eq!(
        srv.session_call(
            "PATCH",
            &path,
            Some(&admin),
            Some(&json!({"role":"technicien"}))
        )
        .status,
        200
    );
    assert_eq!(
        srv.session_call("GET", "/api/auth/me", Some(&cookie), None)
            .status,
        401
    );
    let (cookie, _) = login(&srv, "production", "demo1234");
    let site = srv
        .session_call(
            "POST",
            "/api/sites",
            Some(&admin),
            Some(&json!({"name":"Site de transfert"})),
        )
        .json();
    assert_eq!(
        srv.session_call(
            "PATCH",
            &path,
            Some(&admin),
            Some(&json!({"site_id":site["id"]}))
        )
        .status,
        200
    );
    assert_eq!(
        srv.session_call("GET", "/api/auth/me", Some(&cookie), None)
            .status,
        401
    );
    let (cookie, _) = login(&srv, "production", "demo1234");
    assert_eq!(
        srv.session_call("DELETE", &path, Some(&admin), None).status,
        204
    );
    assert_eq!(
        srv.session_call("GET", "/api/auth/me", Some(&cookie), None)
            .status,
        401
    );
    assert_eq!(
        srv.session_call("GET", &path, Some(&admin), None).status,
        404
    );
    assert_eq!(
        srv.session_call(
            "DELETE",
            &format!("/api/sites/{}", site["id"].as_str().unwrap()),
            Some(&admin),
            None
        )
        .status,
        204
    );
    let sites = srv
        .session_call("GET", "/api/sites", Some(&admin), None)
        .json();
    assert!(sites
        .as_array()
        .unwrap()
        .iter()
        .all(|s| s["id"] != site["id"]));
}
