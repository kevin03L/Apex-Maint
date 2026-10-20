//! Recette terrain avec binaire réel, destinée à être exécutée hors du bac à sable.
#[allow(dead_code)]
mod common;
use common::{request_with_headers, Server};
use serde_json::json;

#[test]
fn synchronisation_idempotence_dates_et_preconditions() {
    let mut server = Server::start("terrain");
    let login = server.session_call(
        "POST",
        "/api/auth/login",
        None,
        Some(&json!({"username":"admin","password":"admin123"})),
    );
    let cookie = login
        .header("set-cookie")
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    assert_eq!(
        server
            .session_call(
                "POST",
                "/api/auth/password",
                Some(&cookie),
                Some(&json!({"current_password":"admin123","new_password":"Terrain-admin123!"}))
            )
            .status,
        204
    );
    let site = server
        .session_call(
            "POST",
            "/api/sites",
            Some(&cookie),
            Some(&json!({"name":"Terrain"})),
        )
        .json();
    let site = site["id"].as_str().unwrap();
    let input = json!({"site_id":site,"title":"Déclaration hors ligne","type":"hors_equipement"});
    let key = uuid::Uuid::now_v7().to_string();
    let date = (chrono::Utc::now() - chrono::Duration::hours(1))
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let headers = [
        ("Cookie", cookie.as_str()),
        ("Idempotency-Key", key.as_str()),
        ("X-Occurred-At", date.as_str()),
    ];
    let first = request_with_headers(
        server.port(),
        "POST",
        "/api/interventions",
        None,
        Some(&input),
        &headers,
    )
    .unwrap();
    assert_eq!(first.status, 201, "{}", first.json());
    assert_eq!(first.json()["reported_at"], date);
    let id = first.json()["id"].as_str().unwrap().to_string();
    server.restart();
    let replay = request_with_headers(
        server.port(),
        "POST",
        "/api/interventions",
        None,
        Some(&input),
        &headers,
    )
    .unwrap();
    assert_eq!(replay.status, 201);
    assert_eq!(first.body, replay.body);
    let child_path = format!("/api/interventions/{id}/sub-interventions");
    let child_key = uuid::Uuid::now_v7().to_string();
    let child_headers = [
        ("Cookie", cookie.as_str()),
        ("Idempotency-Key", child_key.as_str()),
    ];
    let child_input = json!({"site_id":site,"title":"Sous-intervention","type":"hors_equipement"});
    let child = request_with_headers(
        server.port(),
        "POST",
        &child_path,
        None,
        Some(&child_input),
        &child_headers,
    )
    .unwrap();
    assert_eq!(child.status, 201, "{}", child.text());
    server.restart();
    let child_replay = request_with_headers(
        server.port(),
        "POST",
        &child_path,
        None,
        Some(&child_input),
        &child_headers,
    )
    .unwrap();
    assert_eq!(child_replay.status, 201);
    assert_eq!(child.body, child_replay.body);
    let path = format!("/api/interventions/{id}/take");
    assert_eq!(
        request_with_headers(
            server.port(),
            "POST",
            &path,
            None,
            Some(&json!({})),
            &headers
        )
        .unwrap()
        .status,
        422
    );
    let patch = format!("/api/interventions/{id}");
    assert_eq!(
        request_with_headers(
            server.port(),
            "PATCH",
            &patch,
            None,
            Some(&json!({"description":"Compte rendu"})),
            &[
                ("Cookie", &cookie),
                ("If-Unmodified-Since", "2000-01-01T00:00:00Z")
            ]
        )
        .unwrap()
        .status,
        412
    );
    let snapshot = server.session_call("GET", "/api/offline/snapshot", Some(&cookie), None);
    assert_eq!(snapshot.status, 200);
    assert_eq!(
        snapshot.json()["interventions"].as_array().unwrap().len(),
        2
    );
    assert!(!String::from_utf8_lossy(&snapshot.body).contains("password_hash"));
    let login = server.call_with_headers(
        "POST",
        "/api/auth/login",
        None,
        &[("X-Forwarded-Proto", "https")],
    );
    assert_ne!(login.status, 200);
    assert_eq!(
        server.call("GET", "/api/tls/ca.pem", None, None).status,
        404
    );
}
