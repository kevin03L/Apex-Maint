//! Tests boîte noire de la phase 6, exclusivement sur des instances temporaires.
#[allow(dead_code)]
mod common;
use common::{Response, Server, Settings};
use serde_json::{json, Value};

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
    let mut settings = Settings::new();
    settings.demo = true;
    let s = Server::start_with(name, settings);
    let admin = login(&s, "admin", "admin123");
    let r = s.session_call(
        "POST",
        "/api/auth/password",
        Some(&admin),
        Some(&json!({"current_password":"admin123","new_password":"Admin-documents1"})),
    );
    assert_eq!(r.status, 204, "{}", r.text());
    let resp = login(&s, "responsable", "demo1234");
    (s, admin, resp)
}
fn equipment(s: &Server, cookie: &str, code: &str) -> String {
    let r = s.session_call(
        "POST",
        "/api/equipments",
        Some(cookie),
        Some(&json!({"code":code,"name":"Presse"})),
    );
    assert_eq!(r.status, 201, "{}", r.text());
    r.json()["id"].as_str().unwrap().into()
}
fn send(
    s: &Server,
    cookie: &str,
    path: &str,
    fields: &[(&str, &str)],
    name: &str,
    data: &[u8],
) -> Response {
    let mut payload = Vec::new();
    for (name, value) in fields {
        payload.extend_from_slice(format!("--apex-boundary\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n").as_bytes());
    }
    payload.extend_from_slice(format!("--apex-boundary\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{name}\"\r\nContent-Type: application/octet-stream\r\n\r\n").as_bytes());
    payload.extend_from_slice(data);
    payload.extend_from_slice(b"\r\n--apex-boundary--\r\n");
    common::request_bytes(
        s.port(),
        "POST",
        path,
        None,
        &payload,
        &[
            ("Cookie", cookie),
            (
                "Content-Type",
                "multipart/form-data; boundary=apex-boundary",
            ),
        ],
    )
    .unwrap()
}
fn attach(s: &Server, cookie: &str, kind: &str, id: &str, name: &str, data: &[u8]) -> Response {
    send(
        s,
        cookie,
        "/api/documents",
        &[
            ("entity_type", kind),
            ("entity_id", id),
            ("title", "Notice"),
            ("category", "notice"),
        ],
        name,
        data,
    )
}
fn call(
    s: &Server,
    cookie: &str,
    method: &str,
    path: &str,
    data: Option<Value>,
    status: u16,
) -> Value {
    let r = s.session_call(method, path, Some(cookie), data.as_ref());
    assert_eq!(r.status, status, "{method} {path} : {}", r.text());
    if status == 204 {
        Value::Null
    } else {
        r.json()
    }
}
const PDF1: &[u8] = b"%PDF-1.7\nnotice version 1\n%%EOF";
const PDF2: &[u8] = b"%PDF-1.7\nnotice version 2\n%%EOF";

#[test]
fn plafonds_envoi_equivalents_pour_creation_et_version() {
    let (mut s, admin, resp) = setup("documents-plafonds");
    let eq = equipment(&s, &resp, "DOC-LIMIT");
    let mut cfg = apex_config::store::load(&s.config).unwrap().config;
    cfg.files.max_size_mb = 1;
    std::fs::write(&s.config, apex_config::store::render(&cfg)).unwrap();
    s.restart();
    // Même plafond effectif : tantôt le TOML, tantôt le réglage documentaire le borne.
    for maximum_documentaire in [2, 1] {
        call(
            &s,
            &admin,
            "PUT",
            "/api/settings/documents",
            Some(json!({"allowed_types":["pdf"],"max_size_mb":maximum_documentaire})),
            200,
        );
        let initial = attach(&s, &resp, "equipment", &eq, "notice.pdf", PDF1);
        assert_eq!(initial.status, 201);
        let id = initial.json()["id"].as_str().unwrap().to_string();
        for size in [1024 * 1024, 1024 * 1024 + 1] {
            let mut content = vec![b' '; size];
            content[..PDF1.len()].copy_from_slice(PDF1);
            let expected = if size > 1024 * 1024 { 413 } else { 201 };
            let creation = attach(&s, &resp, "equipment", &eq, "notice.pdf", &content);
            let version = send(
                &s,
                &resp,
                &format!("/api/documents/{id}/versions"),
                &[],
                "notice.pdf",
                &content,
            );
            assert_eq!(creation.status, expected, "{}", creation.text());
            assert_eq!(version.status, expected, "{}", version.text());
        }
        let document = call(&s, &resp, "GET", &format!("/api/documents/{id}"), None, 200);
        assert_eq!(document["versions_count"], 2);
    }
}

#[test]
fn adaptateurs_json_documentaires_conservent_413() {
    let (s, admin, _) = setup("documents-json-413");
    for path in ["/api/settings/documents", "/api/settings/labels"] {
        let response = common::request_bytes(
            s.port(),
            "PUT",
            path,
            None,
            &vec![b' '; 65537],
            &[("Cookie", &admin), ("Content-Type", "application/json")],
        )
        .unwrap();
        assert_eq!(response.status, 413, "{}", response.text());
    }
}

#[test]
fn restauration_stockages_personnalises_apres_redemarrage() {
    let (mut s, admin, resp) = setup("restauration-stockages");
    let eq = equipment(&s, &resp, "REST-DOC");
    let mut documents = Vec::new();
    for (index, contenu) in [PDF1, PDF2].into_iter().enumerate() {
        let folder = s.dir().join(format!("source-{index}"));
        std::fs::create_dir(&folder).unwrap();
        let storage = call(
            &s,
            &admin,
            "POST",
            "/api/settings/storage",
            Some(json!({"kind":"folder","label":"Source","config":{"path":folder}})),
            201,
        );
        call(
            &s,
            &admin,
            "POST",
            &format!(
                "/api/settings/storage/{}/activate",
                storage["id"].as_str().unwrap()
            ),
            None,
            204,
        );
        let response = attach(&s, &resp, "equipment", &eq, "notice.pdf", contenu);
        assert_eq!(response.status, 201, "{}", response.text());
        documents.push((
            response.json()["id"].as_str().unwrap().to_string(),
            contenu,
            folder,
        ));
    }
    let output = std::process::Command::new(common::EXE)
        .args([
            "backup",
            "--include-files",
            "folder_only",
            "--with-secret-key",
            "--config",
        ])
        .arg(&s.config)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let archive: Value = serde_json::from_slice(&output.stdout).unwrap();
    let token = s.control_token();
    assert_eq!(
        s.call("POST", "/api/system/shutdown", Some(&token), None)
            .status,
        200
    );
    assert!(s.wait_exit(common::EXIT_TIMEOUT).unwrap().success());
    for (_, _, folder) in &documents {
        std::fs::rename(folder, folder.with_extension("ancienne")).unwrap();
    }
    let output = std::process::Command::new(common::EXE)
        .args([
            "restore",
            archive["archive"].as_str().unwrap(),
            "--force",
            "--config",
        ])
        .arg(&s.config)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    s.restart();
    for (id, contenu, _) in documents {
        let response = s.session_call(
            "GET",
            &format!("/api/documents/{id}/download"),
            Some(&resp),
            None,
        );
        assert_eq!(response.status, 200, "{}", response.text());
        assert_eq!(response.body, contenu);
    }
}

#[test]
fn documents_versions_telechargement_et_audit() {
    let (s, admin, resp) = setup("documents-versions");
    let eq = equipment(&s, &resp, "DOC-01");
    let r = attach(&s, &resp, "equipment", &eq, "notice.pdf", PDF1);
    assert_eq!(r.status, 201, "{}", r.text());
    let d = r.json();
    let id = d["id"].as_str().unwrap();
    assert_eq!(d["versions_count"], 1);
    assert_eq!(d["current_version"]["version_no"], 1);
    assert!(d["current_version"]["uploaded_by"]["display_name"].is_string());
    let count = call(
        &s,
        &resp,
        "GET",
        &format!("/api/equipments/{eq}"),
        None,
        200,
    );
    assert_eq!(count["documents_count"], 1);
    let list = call(
        &s,
        &resp,
        "GET",
        &format!("/api/documents?entity_type=equipment&entity_id={eq}"),
        None,
        200,
    );
    assert_eq!(list.as_array().unwrap().len(), 1);
    let download = s.session_call(
        "GET",
        &format!("/api/documents/{id}/download"),
        Some(&resp),
        None,
    );
    assert_eq!(download.status, 200, "{}", download.text());
    assert_eq!(download.body, PDF1);
    assert_eq!(download.header("content-type"), Some("application/pdf"));
    assert!(download
        .header("content-disposition")
        .unwrap()
        .starts_with("inline; filename*=UTF-8''"));
    assert_eq!(download.header("x-content-type-options"), Some("nosniff"));
    assert_eq!(
        download.header("content-security-policy"),
        Some("sandbox; default-src 'none'")
    );
    assert_eq!(download.header("cache-control"), Some("private, no-store"));
    let forced = s.session_call(
        "GET",
        &format!("/api/documents/{id}/download?download=1"),
        Some(&resp),
        None,
    );
    assert!(forced
        .header("content-disposition")
        .unwrap()
        .starts_with("attachment;"));
    let r = send(
        &s,
        &resp,
        &format!("/api/documents/{id}/versions"),
        &[("comment", "Révision C")],
        "notice.pdf",
        PDF2,
    );
    assert_eq!(r.status, 201, "{}", r.text());
    assert_eq!(r.json()["version_no"], 2);
    assert_eq!(
        s.session_call(
            "GET",
            &format!("/api/documents/{id}/download"),
            Some(&resp),
            None
        )
        .body,
        PDF2
    );
    assert_eq!(
        s.session_call(
            "GET",
            &format!("/api/documents/{id}/download?version=1"),
            Some(&resp),
            None
        )
        .body,
        PDF1
    );
    assert_eq!(
        send(
            &s,
            &resp,
            &format!("/api/documents/{id}/versions"),
            &[],
            "notice.pdf",
            PDF2
        )
        .status,
        409
    );
    let v = call(
        &s,
        &resp,
        "POST",
        &format!("/api/documents/{id}/versions/1/restore"),
        None,
        201,
    );
    assert_eq!(v["version_no"], 3);
    assert_eq!(
        s.session_call(
            "GET",
            &format!("/api/documents/{id}/download"),
            Some(&resp),
            None
        )
        .body,
        PDF1
    );
    let versions = call(
        &s,
        &resp,
        "GET",
        &format!("/api/documents/{id}/versions"),
        None,
        200,
    );
    assert_eq!(versions.as_array().unwrap().len(), 3);
    assert_eq!(versions[0]["version_no"], 3);
    call(
        &s,
        &resp,
        "PATCH",
        &format!("/api/documents/{id}"),
        Some(json!({"title":"Notice corrigée","description":"Révision","category":"rapport"})),
        200,
    );
    let search = call(
        &s,
        &resp,
        "GET",
        "/api/documents/search?q=Notice&limit=1",
        None,
        200,
    );
    assert_eq!(
        search["items"][0]["entity"],
        json!({"type":"equipment","id":eq,"code":"DOC-01","label":"Presse"})
    );
    assert!(search["next_cursor"].is_null());
    assert_eq!(
        s.session_call(
            "GET",
            &format!("/api/equipments/{eq}/qr.svg"),
            Some(&resp),
            None
        )
        .status,
        200
    );
    call(
        &s,
        &resp,
        "DELETE",
        &format!("/api/documents/{id}"),
        None,
        204,
    );
    assert_eq!(
        s.session_call(
            "GET",
            &format!("/api/documents/{id}/download"),
            Some(&resp),
            None
        )
        .status,
        404
    );
    let pool = tokio::runtime::Runtime::new().unwrap();
    pool.block_on(async {
        let db = apex_core::db::open(Some(&s.dir().join("data/apex-maint.db")))
            .await
            .unwrap();
        let actions: Vec<String> = sqlx::query_scalar(
            "SELECT action FROM audit_log WHERE entity='document' AND entity_id=? ORDER BY id",
        )
        .bind(id)
        .fetch_all(&db)
        .await
        .unwrap();
        assert_eq!(
            actions,
            ["create", "version", "restore", "update", "delete"]
        );
        let snapshots: Vec<String> = sqlx::query_scalar(
            "SELECT after_json FROM audit_log WHERE entity='document' AND entity_id=?",
        )
        .bind(id)
        .fetch_all(&db)
        .await
        .unwrap();
        for row in snapshots {
            let v: Value = serde_json::from_str(&row).unwrap();
            assert!(v["current_version"]["sha256"].is_string());
            assert!(v["current_version"]["size_bytes"].is_number());
            assert!(v["current_version"]["original_name"].is_string());
        }
        db.close().await;
    });
    // Toutes les nouvelles routes exigent une session, même avec le jeton de contrôle.
    for path in [
        "/api/documents/search",
        "/api/settings/storage",
        "/api/settings/documents",
        "/api/settings/labels",
    ] {
        assert_eq!(
            s.call("GET", path, Some(&s.control_token()), None).status,
            401
        );
    }
    assert!(!admin.is_empty());
}

#[test]
fn types_taille_et_formats() {
    let (s, admin, resp) = setup("documents-types");
    let eq = equipment(&s, &resp, "DOC-TYPE");
    assert_eq!(
        attach(
            &s,
            &resp,
            "equipment",
            &eq,
            "faux.pdf",
            b"<html>faux</html>"
        )
        .status,
        415
    );
    assert_eq!(
        attach(
            &s,
            &resp,
            "equipment",
            &eq,
            "texte.txt",
            b"Notice de maintenance"
        )
        .status,
        415
    );
    call(
        &s,
        &admin,
        "PUT",
        "/api/settings/documents",
        Some(json!({"allowed_types":["pdf","txt"],"max_size_mb":1})),
        200,
    );
    let r = attach(
        &s,
        &resp,
        "equipment",
        &eq,
        "texte.txt",
        b"Notice de maintenance",
    );
    assert_eq!(r.status, 201, "{}", r.text());
    assert_eq!(
        attach(
            &s,
            &resp,
            "equipment",
            &eq,
            "texte.txt",
            b"Notice\0invalide"
        )
        .status,
        415
    );
    assert_eq!(
        attach(
            &s,
            &resp,
            "equipment",
            &eq,
            "notice.pdf",
            &vec![b'a'; 1024 * 1024 + 1]
        )
        .status,
        413
    );
    call(
        &s,
        &admin,
        "PUT",
        "/api/settings/documents",
        Some(json!({"allowed_types":["svg"],"max_size_mb":1})),
        400,
    );
    let mut labels = call(&s, &admin, "GET", "/api/settings/labels", None, 200);
    labels["sheet"]["columns"] = json!(1000);
    call(&s, &admin, "PUT", "/api/settings/labels", Some(labels), 400);
}

#[test]
fn stockage_historique_secret_et_integrite() {
    let (s, admin, resp) = setup("documents-stockages");
    let eq = equipment(&s, &resp, "DOC-STORAGE");
    let d = attach(&s, &resp, "equipment", &eq, "notice.pdf", PDF1).json();
    let id = d["id"].as_str().unwrap();
    let initial = call(&s, &admin, "GET", "/api/settings/storage", None, 200);
    let initial_id = initial[0]["id"].as_str().unwrap();
    let folder = s.dir().join("autre-stockage");
    std::fs::create_dir(&folder).unwrap();
    let new = call(
        &s,
        &admin,
        "POST",
        "/api/settings/storage",
        Some(json!({"kind":"folder","label":"Nouveau dossier","config":{"path":folder}})),
        201,
    );
    let storage_id = new["id"].as_str().unwrap();
    let probe = call(
        &s,
        &admin,
        "POST",
        &format!("/api/settings/storage/{storage_id}/test"),
        None,
        200,
    );
    assert_eq!(probe["ok"], true);
    call(
        &s,
        &admin,
        "POST",
        &format!("/api/settings/storage/{storage_id}/activate"),
        None,
        204,
    );
    let r = send(
        &s,
        &resp,
        &format!("/api/documents/{id}/versions"),
        &[],
        "notice.pdf",
        PDF2,
    );
    assert_eq!(r.status, 201, "{}", r.text());
    assert!(folder.read_dir().unwrap().next().is_some());
    assert_eq!(
        s.session_call(
            "GET",
            &format!("/api/documents/{id}/download?version=1"),
            Some(&resp),
            None
        )
        .body,
        PDF1
    );
    call(
        &s,
        &admin,
        "DELETE",
        &format!("/api/settings/storage/{initial_id}"),
        None,
        409,
    );
    let s3 = call(
        &s,
        &admin,
        "POST",
        "/api/settings/storage",
        Some(
            json!({"kind":"s3","label":"MinIO","config":{"endpoint":"http://127.0.0.1:1","region":"us-east-1","bucket":"documents","access_key_id":"compte","path_style":true,"prefix":"maint"},"secret":"secret-jamais-retourne"}),
        ),
        201,
    );
    assert_eq!(s3["has_secret"], true);
    assert!(s3.get("secret").is_none());
    assert!(s3.get("secret_enc").is_none());
    assert!(!s3.to_string().contains("secret-jamais-retourne"));
    let invalid = s.dir().join("stockage-disparu");
    std::fs::create_dir(&invalid).unwrap();
    let bad = call(
        &s,
        &admin,
        "POST",
        "/api/settings/storage",
        Some(json!({"kind":"folder","label":"Disparu","config":{"path":invalid}})),
        201,
    );
    std::fs::remove_dir(&invalid).unwrap();
    assert_eq!(
        call(
            &s,
            &admin,
            "POST",
            &format!("/api/settings/storage/{}/test", bad["id"].as_str().unwrap()),
            None,
            200
        )["ok"],
        false
    );
    // Supprimer uniquement le fichier temporaire de ce test pour vérifier le 410.
    fn files(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
        let mut result = Vec::new();
        for entry in dir.read_dir().unwrap() {
            let p = entry.unwrap().path();
            if p.is_dir() {
                result.extend(files(&p));
            } else {
                result.push(p);
            }
        }
        result
    }
    for p in files(&folder) {
        std::fs::remove_file(p).unwrap();
    }
    let missing = s.session_call(
        "GET",
        &format!("/api/documents/{id}/download"),
        Some(&resp),
        None,
    );
    assert_eq!(missing.status, 410, "{}", missing.text());
    assert_eq!(
        missing.json()["message"],
        "Fichier introuvable dans le stockage"
    );
}

#[test]
fn isolation_permissions_entites_supprimees_et_recherche() {
    let (s, admin, resp) = setup("documents-sites");
    let eq = equipment(&s, &resp, "DOC-SITE");
    let d = attach(&s, &resp, "equipment", &eq, "notice.pdf", PDF1).json();
    let id = d["id"].as_str().unwrap();
    let site = call(
        &s,
        &admin,
        "POST",
        "/api/sites",
        Some(json!({"name":"Autre site"})),
        201,
    );
    call(
        &s,
        &admin,
        "POST",
        "/api/users",
        Some(
            json!({"username":"autre-site","display_name":"Autre site","role":"technicien","site_id":site["id"],"password":"Autre-site123","active":true}),
        ),
        201,
    );
    let other = login(&s, "autre-site", "Autre-site123");
    for path in [
        format!("/api/documents?entity_type=equipment&entity_id={eq}"),
        format!("/api/documents/{id}/download"),
        format!("/api/documents/{id}/versions"),
        format!("/api/equipments/{eq}/qr.svg"),
    ] {
        assert_eq!(
            s.session_call("GET", &path, Some(&other), None).status,
            404,
            "{path}"
        );
    }
    assert_eq!(
        send(
            &s,
            &other,
            &format!("/api/documents/{id}/versions"),
            &[],
            "notice.pdf",
            PDF2
        )
        .status,
        404
    );
    let prod = login(&s, "production", "demo1234");
    assert_eq!(
        attach(&s, &prod, "equipment", &eq, "notice.pdf", PDF1).status,
        403
    );
    assert_eq!(
        s.session_call("DELETE", &format!("/api/documents/{id}"), Some(&prod), None)
            .status,
        403
    );
    // La recherche applique les vues d'entités et ne saute pas les lignes après pagination.
    let second = attach(&s, &resp, "equipment", &eq, "autre.pdf", PDF2);
    assert_eq!(second.status, 201);
    let first = call(&s, &resp, "GET", "/api/documents/search?limit=1", None, 200);
    let cursor = first["next_cursor"].as_str().unwrap();
    let next = call(
        &s,
        &resp,
        "GET",
        &format!("/api/documents/search?limit=1&cursor={cursor}"),
        None,
        200,
    );
    assert_ne!(first["items"][0]["id"], next["items"][0]["id"]);
    assert!(
        call(&s, &resp, "GET", "/api/documents/search?q=%25", None, 200)["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    call(
        &s,
        &admin,
        "PUT",
        "/api/permissions/technicien",
        Some(json!({"permissions":["documents.view","documents.create"]})),
        204,
    );
    assert!(
        call(&s, &other, "GET", "/api/documents/search", None, 200)["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    call(
        &s,
        &resp,
        "DELETE",
        &format!("/api/equipments/{eq}"),
        None,
        204,
    );
    assert_eq!(
        s.session_call(
            "GET",
            &format!("/api/documents/{id}/download"),
            Some(&resp),
            None
        )
        .status,
        404
    );
    assert!(
        call(&s, &resp, "GET", "/api/documents/search", None, 200)["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn quatre_entites_et_double_permission() {
    let (s, admin, resp) = setup("documents-entites");
    let eq = equipment(&s, &resp, "DOC-ENTITES");
    let intervention = call(
        &s,
        &resp,
        "POST",
        "/api/interventions",
        Some(
            json!({"title":"Contrôle documentaire","type":"hors_equipement","machine_state":"FONCTIONNEL"}),
        ),
        201,
    );
    let part = call(
        &s,
        &resp,
        "POST",
        "/api/parts",
        Some(json!({"reference":"DOC-PIECE","name":"Filtre"})),
        201,
    );
    let plan = call(
        &s,
        &resp,
        "POST",
        "/api/maintenance/plans",
        Some(
            json!({"code":"DOC-PLAN","title":"Inspection","equipment_id":eq,"frequency":"monthly","anchor_date":"2099-01-01","auto_generate":false}),
        ),
        201,
    );
    for (kind, entity_id, path) in [
        ("equipment", eq.as_str(), format!("/api/equipments/{eq}")),
        (
            "intervention",
            intervention["id"].as_str().unwrap(),
            format!(
                "/api/interventions/{}",
                intervention["id"].as_str().unwrap()
            ),
        ),
        (
            "part",
            part["id"].as_str().unwrap(),
            format!("/api/parts/{}", part["id"].as_str().unwrap()),
        ),
        (
            "maintenance_plan",
            plan["id"].as_str().unwrap(),
            format!("/api/maintenance/plans/{}", plan["id"].as_str().unwrap()),
        ),
    ] {
        let r = attach(&s, &resp, kind, entity_id, "notice.pdf", PDF1);
        assert_eq!(r.status, 201, "{kind} : {}", r.text());
        assert_eq!(
            call(&s, &resp, "GET", &path, None, 200)["documents_count"],
            1
        );
    }
    let technicien = login(&s, "technicien", "demo1234");
    call(
        &s,
        &admin,
        "PUT",
        "/api/permissions/technicien",
        Some(json!({"permissions":["documents.view","documents.create","documents.delete"]})),
        204,
    );
    // Même site et documents.view, mais aucune vue des entités : aucune fuite par la recherche.
    assert!(
        call(&s, &technicien, "GET", "/api/documents/search", None, 200)["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let r = s.session_call(
        "GET",
        &format!("/api/documents?entity_type=equipment&entity_id={eq}"),
        Some(&technicien),
        None,
    );
    assert_eq!(r.status, 403, "{}", r.text());
    assert_eq!(
        attach(&s, &technicien, "equipment", &eq, "notice.pdf", PDF1).status,
        403
    );
}
