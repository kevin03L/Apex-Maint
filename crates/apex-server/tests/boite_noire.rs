//! Tests d'intégration « boîte noire » du serveur : le binaire `apex-maint` compilé est lancé
//! dans un dossier temporaire neuf (mode portable, ports libres) puis interrogé
//! uniquement par HTTP, comme le feraient l'IHM et l'utilitaire desktop.

mod common;

use std::net::TcpListener;

use common::{request, run_to_exit, write_config, Server, Settings, TempDir, EXIT_TIMEOUT};

// ============================================================================
// SANTÉ & DÉTAILS D'EXÉCUTION
// ============================================================================

#[test]
fn health_public_sans_details_et_avec_jeton_local() {
    let mut srv = Server::start("health");

    // 1. Appel public sans jeton
    let r = srv.call("GET", "/api/health", None, None);
    assert_eq!(r.status, 200, "{}", r.text());
    assert_eq!(r.header("cache-control"), Some("no-store"));
    assert_eq!(r.header("x-content-type-options"), Some("nosniff"));
    assert_eq!(r.header("x-frame-options"), Some("DENY"));
    assert_eq!(r.header("referrer-policy"), Some("no-referrer"));

    let body = r.json();
    assert_eq!(body["status"], "ok");
    assert_eq!(body["product"], "Apex Maint");
    assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
    assert!(
        body.get("runtime").is_none(),
        "runtime présent sans jeton : {body}"
    );
    assert!(
        body.get("started_at").is_none(),
        "started_at présent sans jeton : {body}"
    );

    // 2. Appel avec jeton inconnu : détails toujours masqués
    let bad = srv.call("GET", "/api/health", Some("mauvais-jeton"), None);
    assert_eq!(bad.status, 200);
    assert!(bad.json().get("runtime").is_none());

    // 3. Appel avec le jeton de contrôle local (en boucle locale directe)
    let token = srv.control_token();
    let r_auth = srv.call("GET", "/api/health", Some(&token), None);
    assert_eq!(r_auth.status, 200);
    let body_auth = r_auth.json();
    let rt = &body_auth["runtime"];
    assert!(
        rt.is_object(),
        "runtime absent avec jeton valide : {body_auth}"
    );
    assert_eq!(rt["pid"], srv.pid());
    assert_eq!(rt["mode"], "manual");
    assert_eq!(rt["instance_name"], "Essai d'intégration");
    assert!(rt["config_file"]
        .as_str()
        .is_some_and(|f| f.contains("apex-maint.toml")));
    assert!(body_auth["started_at"].is_string());

    // 4. Derrière un proxy inverse : les en-têtes masquent les détails même avec le jeton
    for header in [
        ("X-Forwarded-For", "203.0.113.7"),
        ("Forwarded", "for=203.0.113.7;proto=https"),
        ("X-Real-IP", "203.0.113.7"),
    ] {
        let r_proxy = srv.call_with_headers("GET", "/api/health", Some(&token), &[header]);
        assert_eq!(r_proxy.status, 200);
        let body_proxy = r_proxy.json();
        assert!(
            body_proxy.get("runtime").is_none(),
            "détails publiés via proxy {} : {body_proxy}",
            header.0
        );
    }
}

// ============================================================================
// EN-TÊTES DE SÉCURITÉ & FICHIERS STATIQUES
// ============================================================================

#[test]
fn en_tetes_de_securite_et_fichiers_statiques() {
    let srv = Server::start("security-headers");

    // IHM statique
    for path in ["/", "/app.js", "/styles.css"] {
        let r = srv.call("GET", path, None, None);
        assert_eq!(r.status, 200, "{path}");
        assert_eq!(r.header("cache-control"), Some("no-cache"), "{path}");
        assert_eq!(
            r.header("x-content-type-options"),
            Some("nosniff"),
            "{path}"
        );
        assert_eq!(r.header("x-frame-options"), Some("DENY"), "{path}");
        assert_eq!(r.header("referrer-policy"), Some("no-referrer"), "{path}");
        let csp = r.header("content-security-policy").unwrap_or_default();
        assert!(csp.contains("default-src 'self'"), "{path}");
    }

    // Route inconnue sous /api -> 404 JSON avec Cache-Control: no-store
    let r_404 = srv.call("GET", "/api/inexistant", None, None);
    assert_eq!(r_404.status, 404);
    assert_eq!(r_404.header("cache-control"), Some("no-store"));
    assert_eq!(r_404.json()["error"], true);
}

// ============================================================================
// CODES DE SORTIE
// ============================================================================

#[test]
fn code_3_configuration_invalide() {
    let base = Settings::new();
    let port_nul = base
        .render()
        .replace(&format!("port = {}\n", base.port), "port = 0\n");
    assert!(port_nul.contains("port = 0\n"));

    let cases = [
        ("port-nul", port_nul),
        (
            "syntaxe",
            "config_version = 1\n[server\nport = 1\n".to_string(),
        ),
        (
            "champ-inconnu",
            "config_version = 1\n[server]\nportt = 4000\n".to_string(),
        ),
        ("version-future", "config_version = 99\n".to_string()),
        ("version-negative", "config_version = -1\n".to_string()),
        ("version-nulle", "config_version = 0\n".to_string()),
    ];

    for (name, content) in cases {
        let dir = TempDir::new(&format!("code3-{name}"));
        let config = write_config(&dir.path, &content);
        let out = run_to_exit(&dir.path, &config, "engine");
        assert_eq!(
            out.code,
            Some(3),
            "{name} : code attendu 3, obtenu {:?}\n{}\n{}",
            out.code,
            out.stdout,
            out.stderr
        );
        assert!(
            out.stderr.contains("Démarrage impossible"),
            "{name} : {}",
            out.stderr
        );
        assert!(
            !dir.path.join("data").join("apex-maint.db").exists(),
            "{name} : base créée malgré configuration invalide"
        );
        assert!(
            dir.path
                .join("logs")
                .join("apex-maint-startup.log")
                .is_file(),
            "{name} : journal d'erreur de démarrage manquant"
        );
    }
}

#[test]
fn code_4_deuxieme_instance_meme_dossier() {
    let srv = Server::start("code4");
    let out = run_to_exit(srv.dir(), &srv.config, "second");
    assert_eq!(out.code, Some(4), "{}\n{}", out.stdout, out.stderr);
    assert!(
        out.stderr.contains("autre instance"),
        "message attendu dans stderr : {}",
        out.stderr
    );
    // Première instance toujours active
    let r = srv.call("GET", "/api/health", None, None);
    assert_eq!(r.status, 200);
}

#[test]
fn code_5_port_occupe() {
    let holder = TcpListener::bind("127.0.0.1:0").expect("réservation d'un port libre");
    let port = holder.local_addr().expect("adresse locale").port();

    let mut settings = Settings::new();
    settings.port = port;

    let dir = TempDir::new("code5");
    let config = write_config(&dir.path, &settings.render());
    let out = run_to_exit(&dir.path, &config, "engine");
    drop(holder);

    assert_eq!(out.code, Some(5), "{}\n{}", out.stdout, out.stderr);
    assert!(
        out.stderr.contains("impossible d'écouter"),
        "message attendu dans stderr : {}",
        out.stderr
    );
    assert!(!dir.path.join("data").join("apex-maint.db").exists());
}

// ============================================================================
// ARRÊT PROPRE
// ============================================================================

#[test]
fn arret_propre_par_jeton_de_controle_local() {
    let mut srv = Server::start("arret-propre");
    let token_file = srv.dir().join("run").join("control.token");
    assert!(
        token_file.is_file(),
        "fichier du jeton de contrôle manquant"
    );
    let token = srv.control_token();

    // 1. Sans jeton -> 401
    let r_no_token = srv.call("POST", "/api/system/shutdown", None, None);
    assert_eq!(r_no_token.status, 401);
    assert_eq!(srv.call("GET", "/api/health", None, None).status, 200);

    // 2. Avec jeton mais via proxy -> 401
    for header in [
        ("X-Forwarded-For", "203.0.113.7"),
        ("Forwarded", "for=203.0.113.7"),
        ("X-Real-IP", "203.0.113.7"),
    ] {
        let r_proxy =
            srv.call_with_headers("POST", "/api/system/shutdown", Some(&token), &[header]);
        assert_eq!(r_proxy.status, 401);
    }

    // 3. Status autorisé avec le jeton
    let r_status = srv.call("GET", "/api/system/status", Some(&token), None);
    assert_eq!(r_status.status, 200);

    // 4. Arrêt demandé avec le jeton direct
    let r_shutdown = srv.call("POST", "/api/system/shutdown", Some(&token), None);
    assert_eq!(r_shutdown.status, 200);
    assert_eq!(r_shutdown.json()["status"], "shutting_down");

    // Le processus doit s'arrêter proprement
    let status = srv
        .wait_exit(EXIT_TIMEOUT)
        .unwrap_or_else(|| panic!("le moteur ne s'est pas arrêté à temps\n{}", srv.logs()));
    assert_eq!(
        status.code(),
        Some(0),
        "code de sortie 0 attendu : {}",
        srv.logs()
    );

    // Le jeton doit être supprimé et le port libéré
    assert!(
        !token_file.exists(),
        "le fichier du jeton doit être supprimé"
    );
    assert!(request(srv.port(), "GET", "/api/health", None, None).is_err());
}
