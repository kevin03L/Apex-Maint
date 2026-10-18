//! Tests boîte noire complets de la phase 10 : liaison avec Apex Supervision.
#[allow(dead_code)]
mod common;
use common::{request_with_headers, Server};
use serde_json::json;

fn admin_login(server: &Server) -> String {
    let login = server.session_call(
        "POST",
        "/api/auth/login",
        None,
        Some(&json!({"username": "admin", "password": "admin123"})),
    );
    assert_eq!(login.status, 200, "Connexion admin réussie");
    let cookie = login
        .header("set-cookie")
        .expect("cookie de session")
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
                Some(&json!({
                    "current_password": "admin123",
                    "new_password": "Integration-Admin123!"
                }))
            )
            .status,
        204,
        "Changement de mot de passe admin"
    );
    cookie
}

#[test]
fn boite_noire_integration_supervision_cycle_complet() {
    let server = Server::start("it-supervision");
    let cookie = admin_login(&server);

    // 1. Liaison désactivée par défaut : toutes les routes d'intégration répondent 404
    let dummy_headers = [(
        "Authorization",
        "Bearer apx_1234567890123456789012345678901234567890123",
    )];
    let r_off = request_with_headers(
        server.port(),
        "POST",
        "/api/integrations/supervision/counters",
        None,
        Some(&json!({"readings": []})),
        &dummy_headers,
    )
    .unwrap();
    assert_eq!(r_off.status, 404, "Liaison désactivée -> 404");

    // 2. Création de deux sites distincts : Site A et Site B
    let r_site_a = server.session_call(
        "POST",
        "/api/sites",
        Some(&cookie),
        Some(&json!({"name": "Atelier Nord"})),
    );
    assert_eq!(r_site_a.status, 201);
    let site_a = r_site_a.json()["id"].as_str().unwrap().to_string();

    let r_site_b = server.session_call(
        "POST",
        "/api/sites",
        Some(&cookie),
        Some(&json!({"name": "Atelier Sud"})),
    );
    assert_eq!(r_site_b.status, 201);
    let site_b = r_site_b.json()["id"].as_str().unwrap().to_string();

    // 3. Activation de la liaison dans les paramètres
    let r_settings = server.session_call(
        "PUT",
        "/api/integration/settings",
        Some(&cookie),
        Some(&json!({
            "integration.supervision.enabled": true,
            "integration.supervision.counters_enabled": true,
            "integration.supervision.status_enabled": true,
            "integration.supervision.counter_min_interval_min": 15,
            "integration.supervision.status_stale_after_min": 30
        })),
    );
    assert_eq!(
        r_settings.status,
        200,
        "Activation liaison : {}",
        r_settings.text()
    );

    // 4. Création des clés de service
    // Clé Site A avec portée ["counters"]
    let r_key_c = server.session_call(
        "POST",
        "/api/integration/keys",
        Some(&cookie),
        Some(&json!({
            "site_id": site_a,
            "label": "Supervision Compteurs A",
            "scopes": ["counters"]
        })),
    );
    assert_eq!(r_key_c.status, 201);
    let token_c = r_key_c.json()["key"].as_str().unwrap().to_string();
    let key_c_id = r_key_c.json()["item"]["id"].as_str().unwrap().to_string();

    // Clé Site A avec portée ["status"]
    let r_key_s = server.session_call(
        "POST",
        "/api/integration/keys",
        Some(&cookie),
        Some(&json!({
            "site_id": site_a,
            "label": "Supervision Statut A",
            "scopes": ["status"]
        })),
    );
    assert_eq!(r_key_s.status, 201);
    let token_s = r_key_s.json()["key"].as_str().unwrap().to_string();
    let key_s_id = r_key_s.json()["item"]["id"].as_str().unwrap().to_string();

    // Clé révoquée
    let r_key_rev = server.session_call(
        "POST",
        "/api/integration/keys",
        Some(&cookie),
        Some(&json!({
            "site_id": site_a,
            "label": "Clé Temporaire",
            "scopes": ["counters", "status"]
        })),
    );
    let token_rev = r_key_rev.json()["key"].as_str().unwrap().to_string();
    let key_rev_id = r_key_rev.json()["item"]["id"].as_str().unwrap().to_string();
    let r_revoke = server.session_call(
        "POST",
        &format!("/api/integration/keys/{key_rev_id}/revoke"),
        Some(&cookie),
        None,
    );
    assert_eq!(r_revoke.status, 200);

    // 5. Tests de sécurité et des gardes
    // a) Clé status reçoit 401 sur /counters
    let auth_s = format!("Bearer {token_s}");
    let h_s = [("Authorization", auth_s.as_str())];
    let r_scope_err = request_with_headers(
        server.port(),
        "POST",
        "/api/integrations/supervision/counters",
        None,
        Some(&json!({"readings": []})),
        &h_s,
    )
    .unwrap();
    assert_eq!(
        r_scope_err.status, 401,
        "Clé status sur counters -> 401 sans détail"
    );

    // b) Clé counters reçoit 401 sur /status
    let auth_c = format!("Bearer {token_c}");
    let h_c = [("Authorization", auth_c.as_str())];
    let r_scope_err2 = request_with_headers(
        server.port(),
        "POST",
        "/api/integrations/supervision/status",
        None,
        Some(&json!({"statuses": []})),
        &h_c,
    )
    .unwrap();
    assert_eq!(r_scope_err2.status, 401, "Clé counters sur status -> 401");

    // c) Clé révoquée reçoit 401
    let auth_rev = format!("Bearer {token_rev}");
    let h_rev = [("Authorization", auth_rev.as_str())];
    let r_rev_err = request_with_headers(
        server.port(),
        "POST",
        "/api/integrations/supervision/counters",
        None,
        Some(&json!({"readings": []})),
        &h_rev,
    )
    .unwrap();
    assert_eq!(r_rev_err.status, 401, "Clé révoquée -> 401");

    // d) Cookie de session seul sans clé sur /counters -> 401
    let h_cookie_only = [("Cookie", cookie.as_str())];
    let r_cookie_err = request_with_headers(
        server.port(),
        "POST",
        "/api/integrations/supervision/counters",
        None,
        Some(&json!({"readings": []})),
        &h_cookie_only,
    )
    .unwrap();
    assert_eq!(
        r_cookie_err.status, 401,
        "Cookie de session sans clé -> 401"
    );

    // e) Une clé d'intégration n'ouvre aucune autre route (/api/equipments -> 401)
    let r_other_route =
        request_with_headers(server.port(), "GET", "/api/equipments", None, None, &h_c).unwrap();
    assert_eq!(
        r_other_route.status, 401,
        "Clé d'intégration sur /api/equipments -> 401"
    );

    // 6. Préparation des équipements et compteurs
    // Équipement A1 sur Site A
    let r_eq_a = server.session_call(
        "POST",
        "/api/equipments",
        Some(&cookie),
        Some(&json!({
            "site_id": site_a,
            "code": "PRS-01",
            "name": "Presse hydraulique A1",
            "status": "operational"
        })),
    );
    assert_eq!(r_eq_a.status, 201);
    let eq_a_id = r_eq_a.json()["id"].as_str().unwrap().to_string();

    // Référence externe sur Équipement A1
    let r_ref_eq = server.session_call(
        "PATCH",
        &format!("/api/equipments/{eq_a_id}/external-ref"),
        Some(&cookie),
        Some(&json!({"external_ref": "SUP-EQ-PRS01"})),
    );
    assert_eq!(r_ref_eq.status, 200);

    // Compteur A1 sur Eq A1
    let r_cnt_a = server.session_call(
        "POST",
        &format!("/api/equipments/{eq_a_id}/counters"),
        Some(&cookie),
        Some(&json!({
            "name": "Heures de fonctionnement",
            "unit": "h",
            "current_value": 0
        })),
    );
    assert_eq!(r_cnt_a.status, 201);
    let cnt_a_id = r_cnt_a.json()["id"].as_str().unwrap().to_string();

    // Référence externe sur Compteur A1
    let r_ref_cnt = server.session_call(
        "PATCH",
        &format!("/api/counters/{cnt_a_id}/external-ref"),
        Some(&cookie),
        Some(&json!({"external_ref": "SUP-VAR-HOURS-01"})),
    );
    assert_eq!(r_ref_cnt.status, 200);

    // Équipement B1 et Compteur B1 sur Site B
    let r_eq_b = server.session_call(
        "POST",
        "/api/equipments",
        Some(&cookie),
        Some(&json!({
            "site_id": site_b,
            "code": "PRS-02",
            "name": "Presse hydraulique B1",
            "status": "operational"
        })),
    );
    assert_eq!(r_eq_b.status, 201);
    let eq_b_id = r_eq_b.json()["id"].as_str().unwrap().to_string();

    let r_cnt_b = server.session_call(
        "POST",
        &format!("/api/equipments/{eq_b_id}/counters"),
        Some(&cookie),
        Some(&json!({
            "name": "Heures Site B",
            "unit": "h",
            "current_value": 0
        })),
    );
    assert_eq!(r_cnt_b.status, 201);
    let cnt_b_id = r_cnt_b.json()["id"].as_str().unwrap().to_string();
    server.session_call(
        "PATCH",
        &format!("/api/counters/{cnt_b_id}/external-ref"),
        Some(&cookie),
        Some(&json!({"external_ref": "SUP-VAR-SITE-B"})),
    );

    // Planning préventif compteur sur Eq A1 : seuil 100, lead 10 (déclenchement à 90)
    let r_plan = server.session_call(
        "POST",
        "/api/maintenance/plans",
        Some(&cookie),
        Some(&json!({
            "site_id": site_a,
            "code": "PLN-PRS-100",
            "title": "Maintenance 100h Presse",
            "equipment_id": eq_a_id,
            "frequency": "counter",
            "counter_id": cnt_a_id,
            "counter_interval": 100,
            "anchor_counter": 0,
            "next_due_counter": 100,
            "lead_counter": 10,
            "auto_generate": true
        })),
    );
    assert_eq!(r_plan.status, 201, "{}", r_plan.text());
    let plan_id = r_plan.json()["id"].as_str().unwrap().to_string();

    // 7. Isolation de site :
    // La clé Site A tente d'envoyer un relevé pour le compteur du Site B -> rejeté comme inconnu
    let now = chrono::Utc::now() - chrono::Duration::hours(2);
    let r_cross_site = request_with_headers(
        server.port(),
        "POST",
        "/api/integrations/supervision/counters",
        None,
        Some(&json!({
            "readings": [
                {
                    "counter_ref": "SUP-VAR-SITE-B",
                    "value": 10,
                    "read_at": now.to_rfc3339()
                }
            ]
        })),
        &h_c,
    )
    .unwrap();
    assert_eq!(r_cross_site.status, 200);
    assert_eq!(r_cross_site.json()["rejected"], 1);
    assert_eq!(r_cross_site.json()["accepted"], 0);

    // 8. Ingestion de compteurs
    // a) Lot mixte : 1 inconnu + 1 valide (50h)
    let r_batch1 = request_with_headers(
        server.port(),
        "POST",
        "/api/integrations/supervision/counters",
        None,
        Some(&json!({
            "readings": [
                {
                    "counter_ref": "REF-INEXISTANTE",
                    "value": 10,
                    "read_at": now.to_rfc3339()
                },
                {
                    "counter_ref": "SUP-VAR-HOURS-01",
                    "value": 50,
                    "read_at": now.to_rfc3339()
                }
            ]
        })),
        &h_c,
    )
    .unwrap();
    assert_eq!(r_batch1.status, 200);
    assert_eq!(r_batch1.json()["accepted"], 1);
    assert_eq!(r_batch1.json()["rejected"], 1);
    assert_eq!(r_batch1.json()["errors"].as_array().unwrap().len(), 1);

    // b) Idempotence : renvoyer la même valeur est accepté sans effet
    let r_dup = request_with_headers(
        server.port(),
        "POST",
        "/api/integrations/supervision/counters",
        None,
        Some(&json!({
            "readings": [
                {
                    "counter_ref": "SUP-VAR-HOURS-01",
                    "value": 50,
                    "read_at": now.to_rfc3339()
                }
            ]
        })),
        &h_c,
    )
    .unwrap();
    assert_eq!(r_dup.status, 200);
    assert_eq!(r_dup.json()["accepted"], 1);
    assert_eq!(r_dup.json()["rejected"], 0);

    // c) Valeur en baisse sans reset -> rejet
    let r_down = request_with_headers(
        server.port(),
        "POST",
        "/api/integrations/supervision/counters",
        None,
        Some(&json!({
            "readings": [
                {
                    "counter_ref": "SUP-VAR-HOURS-01",
                    "value": 30,
                    "read_at": (now + chrono::Duration::minutes(1)).to_rfc3339()
                }
            ]
        })),
        &h_c,
    )
    .unwrap();
    assert_eq!(r_down.status, 200);
    assert_eq!(r_down.json()["rejected"], 1);
    assert!(r_down.json()["errors"][0]["message"]
        .as_str()
        .unwrap()
        .contains("valeur en baisse"));

    // d) Espacement : relevé rapproché (5 min) sans franchissement de seuil
    // Met à jour current_value mais n'écrit pas d'historique
    let t_5min = (now + chrono::Duration::minutes(5)).to_rfc3339();
    let r_spaced = request_with_headers(
        server.port(),
        "POST",
        "/api/integrations/supervision/counters",
        None,
        Some(&json!({
            "readings": [
                {
                    "counter_ref": "SUP-VAR-HOURS-01",
                    "value": 60,
                    "read_at": t_5min
                }
            ]
        })),
        &h_c,
    )
    .unwrap();
    assert_eq!(r_spaced.status, 200);
    assert_eq!(r_spaced.json()["accepted"], 1);

    // Vérifier l'historique des relevés : 1 seule ligne (celle à 50)
    let r_history = server.session_call(
        "GET",
        &format!("/api/counters/{cnt_a_id}/readings"),
        Some(&cookie),
        None,
    );
    assert_eq!(r_history.status, 200);
    assert_eq!(r_history.json()["items"].as_array().unwrap().len(), 1);
    // Mais current_value est à 60
    let r_cnt_check = server.session_call(
        "GET",
        &format!("/api/equipments/{eq_a_id}/counters"),
        Some(&cookie),
        None,
    );
    assert_eq!(r_cnt_check.json()[0]["current_value"], 60);

    // e) Franchissement de seuil (valeur 95 >= 90) : enregistre la ligne et génère le préventif !
    let t_10min = (now + chrono::Duration::minutes(10)).to_rfc3339();
    let r_threshold = request_with_headers(
        server.port(),
        "POST",
        "/api/integrations/supervision/counters",
        None,
        Some(&json!({
            "readings": [
                {
                    "counter_ref": "SUP-VAR-HOURS-01",
                    "value": 95,
                    "read_at": t_10min
                }
            ]
        })),
        &h_c,
    )
    .unwrap();
    assert_eq!(r_threshold.status, 200);
    assert_eq!(r_threshold.json()["accepted"], 1);

    // Vérifier le déclenchement de l'intervention préventive
    let r_plan_check = server.session_call(
        "GET",
        &format!("/api/maintenance/plans/{plan_id}"),
        Some(&cookie),
        None,
    );
    assert_eq!(r_plan_check.status, 200);
    assert!(
        !r_plan_check.json()["open_occurrence"].is_null(),
        "Occurrence préventive générée"
    );

    // f) Reset avec conservation de la distance restante
    let r_reset = request_with_headers(
        server.port(),
        "POST",
        "/api/integrations/supervision/counters",
        None,
        Some(&json!({
            "readings": [
                {
                    "counter_ref": "SUP-VAR-HOURS-01",
                    "value": 0,
                    "read_at": (now + chrono::Duration::minutes(20)).to_rfc3339(),
                    "reset": true
                }
            ]
        })),
        &h_c,
    )
    .unwrap();
    assert_eq!(r_reset.status, 200);
    assert_eq!(r_reset.json()["accepted"], 1);

    // 9. Ingestion d'états
    // Rapprochement par external_ref (Eq A1) et message contenant du HTML brut
    let html_payload = "Alerte thermique : <b>surchauffe > 90°C</b> & arret d'urgence";
    let r_st1 = request_with_headers(
        server.port(),
        "POST",
        "/api/integrations/supervision/status",
        None,
        Some(&json!({
            "statuses": [
                {
                    "equipment_ref": "SUP-EQ-PRS01",
                    "state": "alarm",
                    "since": now.to_rfc3339(),
                    "message": html_payload
                }
            ]
        })),
        &h_s,
    )
    .unwrap();
    assert_eq!(r_st1.status, 200);
    assert_eq!(r_st1.json()["accepted"], 1);

    // a) Statut Maint de l'équipement reste INCHANGÉ
    let r_eq_check = server.session_call(
        "GET",
        &format!("/api/equipments/{eq_a_id}"),
        Some(&cookie),
        None,
    );
    assert_eq!(r_eq_check.status, 200);
    assert_eq!(r_eq_check.json()["status"], "operational");

    // b) Consultation de l'état Supervision : le HTML est restitué tel quel (brut)
    let r_ext_status = server.session_call(
        "GET",
        &format!("/api/equipments/{eq_a_id}/supervision-status"),
        Some(&cookie),
        None,
    );
    assert_eq!(r_ext_status.status, 200);
    assert_eq!(r_ext_status.json()["state"], "alarm");
    assert_eq!(r_ext_status.json()["message"], html_payload);

    // c) L'historique ne garde que les changements : renvoyer le même état alarm
    let r_st_dup = request_with_headers(
        server.port(),
        "POST",
        "/api/integrations/supervision/status",
        None,
        Some(&json!({
            "statuses": [
                {
                    "equipment_ref": "SUP-EQ-PRS01",
                    "state": "alarm",
                    "since": (now + chrono::Duration::minutes(5)).to_rfc3339(),
                    "message": "Toujours alarm"
                }
            ]
        })),
        &h_s,
    )
    .unwrap();
    assert_eq!(r_st_dup.status, 200);

    let r_hist1 = server.session_call(
        "GET",
        &format!("/api/equipments/{eq_a_id}/supervision-history"),
        Some(&cookie),
        None,
    );
    assert_eq!(r_hist1.json()["items"].as_array().unwrap().len(), 1);

    // Changement d'état vers running -> 2ème ligne dans l'historique
    let r_st_run = request_with_headers(
        server.port(),
        "POST",
        "/api/integrations/supervision/status",
        None,
        Some(&json!({
            "statuses": [
                {
                    "equipment_ref": "SUP-EQ-PRS01",
                    "state": "running",
                    "since": (now + chrono::Duration::minutes(10)).to_rfc3339(),
                    "message": "Reprise normale"
                }
            ]
        })),
        &h_s,
    )
    .unwrap();
    assert_eq!(r_st_run.status, 200);

    let r_hist2 = server.session_call(
        "GET",
        &format!("/api/equipments/{eq_a_id}/supervision-history"),
        Some(&cookie),
        None,
    );
    assert_eq!(r_hist2.json()["items"].as_array().unwrap().len(), 2);

    // 10. Données du widget supervision_status
    // Actuellement eq A1 est running. Mettons Eq B1 à l'arrêt ("stopped") via son code "PRS-02"
    // Créons une clé sur Site B pour envoyer le statut
    let r_key_b = server.session_call(
        "POST",
        "/api/integration/keys",
        Some(&cookie),
        Some(&json!({
            "site_id": site_b,
            "label": "Supervision B",
            "scopes": ["status"]
        })),
    );
    let token_b = r_key_b.json()["key"].as_str().unwrap().to_string();
    let auth_b = format!("Bearer {token_b}");
    let h_b = [("Authorization", auth_b.as_str())];
    let r_st_b = request_with_headers(
        server.port(),
        "POST",
        "/api/integrations/supervision/status",
        None,
        Some(&json!({
            "statuses": [
                {
                    "equipment_ref": "PRS-02", // par code
                    "state": "stopped",
                    "since": now.to_rfc3339(),
                    "message": "Arrêt maintenance"
                }
            ]
        })),
        &h_b,
    )
    .unwrap();
    assert_eq!(r_st_b.status, 200);
    assert_eq!(r_st_b.json()["accepted"], 1);

    // Widget supervision-status : doit retourner Eq B1 (stopped)
    let r_widget = server.session_call(
        "GET",
        "/api/integration/supervision-status",
        Some(&cookie),
        None,
    );
    assert_eq!(r_widget.status, 200);
    let widget_items = r_widget.json()["items"].as_array().unwrap().clone();
    assert_eq!(widget_items.len(), 1);
    assert_eq!(widget_items[0]["equipment_id"], eq_b_id);

    // 11. Journal des échanges
    let r_events = server.session_call("GET", "/api/integration/events", Some(&cookie), None);
    assert_eq!(r_events.status, 200);
    assert!(
        !r_events.json()["items"].as_array().unwrap().is_empty(),
        "Événements d'échange présents"
    );

    // 12. Limitation de débit : 120 requêtes par minute par clé
    // 1 requête a déjà été faite avec token_b, en faire 119 autres pour atteindre 120, puis la 121ème répond 429
    for i in 1..=119 {
        let res = request_with_headers(
            server.port(),
            "POST",
            "/api/integrations/supervision/status",
            None,
            Some(&json!({"statuses": []})),
            &h_b,
        )
        .unwrap();
        assert_eq!(res.status, 200, "Requête rate limit {i}");
    }
    let r_rate_limited = request_with_headers(
        server.port(),
        "POST",
        "/api/integrations/supervision/status",
        None,
        Some(&json!({"statuses": []})),
        &h_b,
    )
    .unwrap();
    assert_eq!(
        r_rate_limited.status, 429,
        "121ème requête -> 429 Too Many Requests"
    );
    assert!(
        r_rate_limited.header("retry-after").is_some(),
        "En-tête Retry-After présent"
    );

    // 13. Audit de la gestion des clés
    // Modifier la portée de key_c_id
    server.session_call(
        "PATCH",
        &format!("/api/integration/keys/{key_c_id}"),
        Some(&cookie),
        Some(&json!({"scopes": ["counters", "status"]})),
    );
    // Révocation de key_s_id
    server.session_call(
        "POST",
        &format!("/api/integration/keys/{key_s_id}/revoke"),
        Some(&cookie),
        None,
    );
    // Liste des clés
    let r_keys = server.session_call("GET", "/api/integration/keys", Some(&cookie), None);
    assert_eq!(r_keys.status, 200);
    assert!(r_keys.json()["items"].as_array().unwrap().len() >= 3);
}

#[test]
fn tailles_corps_limite_lot_et_peremption() {
    let server = Server::start("it-limits");
    let cookie = admin_login(&server);

    let r_site = server.session_call(
        "POST",
        "/api/sites",
        Some(&cookie),
        Some(&json!({"name": "Atelier Limites"})),
    );
    assert_eq!(r_site.status, 201);
    let site_id = r_site.json()["id"].as_str().unwrap().to_string();

    server.session_call(
        "PUT",
        "/api/integration/settings",
        Some(&cookie),
        Some(&json!({
            "integration.supervision.enabled": true,
            "integration.supervision.counters_enabled": true,
            "integration.supervision.status_enabled": true,
            "integration.supervision.status_stale_after_min": 30
        })),
    );

    let r_key = server.session_call(
        "POST",
        "/api/integration/keys",
        Some(&cookie),
        Some(&json!({
            "site_id": site_id,
            "label": "Clé Limites",
            "scopes": ["counters", "status"]
        })),
    );
    assert_eq!(r_key.status, 201);
    let token = r_key.json()["key"].as_str().unwrap().to_string();
    let auth_str = format!("Bearer {token}");
    let headers = [("Authorization", auth_str.as_str())];

    // 1. Lot de plus de 1000 éléments -> 400
    let now = (chrono::Utc::now() - chrono::Duration::hours(1)).to_rfc3339();
    let too_many_readings: Vec<_> = (0..1001)
        .map(|i| {
            json!({
                "counter_ref": format!("REF-{i}"),
                "value": i,
                "read_at": now
            })
        })
        .collect();
    let r_too_many = request_with_headers(
        server.port(),
        "POST",
        "/api/integrations/supervision/counters",
        None,
        Some(&json!({"readings": too_many_readings})),
        &headers,
    )
    .unwrap();
    assert_eq!(r_too_many.status, 400, "Plus de 1000 éléments -> 400");

    // 2. Corps dépassant 64 Ko mais inférieur à 1 Mo (environ 80 Ko avec 800 éléments) -> accepté 200
    let large_batch: Vec<_> = (0..800)
        .map(|i| {
            json!({
                "counter_ref": format!("REF-VERY-LONG-NAME-TESTING-LIMITS-EXPANDING-PAYLOAD-{i}"),
                "value": i,
                "read_at": now
            })
        })
        .collect();
    let r_large = request_with_headers(
        server.port(),
        "POST",
        "/api/integrations/supervision/counters",
        None,
        Some(&json!({"readings": large_batch})),
        &headers,
    )
    .unwrap();
    assert_eq!(
        r_large.status,
        200,
        "Corps > 64 Ko et <= 1 Mo accepté : {}",
        r_large.text()
    );

    // 3. Corps dépassant 1 Mo (> 1 048 576 octets) -> rejeté 413
    let huge_string = "x".repeat(1024 * 1024 + 1024);
    let r_huge = request_with_headers(
        server.port(),
        "POST",
        "/api/integrations/supervision/status",
        None,
        Some(&json!({"statuses": [{"equipment_ref": "PRS", "state": "running", "since": now, "message": huge_string}]})),
        &headers,
    )
    .unwrap();
    assert_eq!(r_huge.status, 413, "Corps > 1 Mo -> 413 Payload Too Large");
}
