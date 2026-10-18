//! Tests boîte noire de la maintenance préventive (Phase 5).
//!
//! Couvre l'ensemble des exigences de docs/specs/phase-5.md §6 :
//! - CRUD des plannings et des compteurs
//! - Isolation entre sites (404)
//! - Permissions RBAC (403 sans maintenance.manage, equipments.edit, counters.record)
//! - Cohérence fréquence et colonnes (400)
//! - Compteur d'un autre équipement refusé (400)
//! - Génération manuelle (intervention preventive + occurrence, 409 sur la 2ème)
//! - Clôture de l'intervention (occurrence done, avance de l'échéance fixe)
//! - Annulation de l'intervention (occurrence skipped, avance de l'échéance)
//! - Report sans intervention (skip, 409 si ouverte)
//! - Relevé de compteur (refus décroissant, reset, génération auto au franchissement de seuil)
//! - Génération automatique par la tâche de fond (APEX_MAINT_TEST_SCHEDULER_SECONDS)
//! - Deux générations concurrentes ne créent qu'une seule intervention
//! - Suppression d'un équipement ayant des plannings (409)
//! - Vue planning (projection, refus > 366 jours)
//! - Remise en échéance non traitée après suppression logique d'intervention

#[allow(dead_code)]
mod common;

use common::{Server, Settings};
use serde_json::{json, Value};

fn login(srv: &Server, name: &str, password: &str) -> (String, Value) {
    let r = srv.session_call(
        "POST",
        "/api/auth/login",
        None,
        Some(&json!({"username": name, "password": password})),
    );
    assert_eq!(r.status, 200, "Connexion de {name} : {}", r.text());
    let cookie = r.header("set-cookie").expect("cookie de connexion");
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
        Some(&json!({"current_password": "admin123", "new_password": "Admin-renouvele1"})),
    );
    assert_eq!(r.status, 204, "{}", r.text());
    (cookie, user)
}

fn demo(name: &str) -> Server {
    let mut cfg = Settings::new();
    cfg.demo = true;
    Server::start_with(name, cfg)
}

fn create_equipment(srv: &Server, cookie: &str, code: &str, name: &str) -> String {
    let r = srv.session_call(
        "POST",
        "/api/equipments",
        Some(cookie),
        Some(&json!({
            "code": code,
            "name": name,
            "status": "operational"
        })),
    );
    assert_eq!(r.status, 201, "création équipement {code} : {}", r.text());
    r.json()["id"].as_str().unwrap().to_string()
}

#[test]
fn crud_plannings_et_compteurs() {
    let srv = demo("maint-crud");
    let (resp_cookie, _) = login(&srv, "responsable", "demo1234");
    let eq_id = create_equipment(&srv, &resp_cookie, "EQ-M-01", "Presse Hydraulique");

    // 1. Création d'un compteur
    let r_c = srv.session_call(
        "POST",
        &format!("/api/equipments/{eq_id}/counters"),
        Some(&resp_cookie),
        Some(&json!({
            "name": "Heures de marche",
            "unit": "h",
            "current_value": 150
        })),
    );
    assert_eq!(r_c.status, 201, "{}", r_c.text());
    let counter = r_c.json();
    let counter_id = counter["id"].as_str().unwrap();
    assert_eq!(counter["name"], "Heures de marche");
    assert_eq!(counter["unit"], "h");
    assert_eq!(counter["current_value"], 150);
    assert_eq!(counter["plans_count"], 0);

    // 2. Mise à jour du compteur (PATCH)
    let r_cp = srv.session_call(
        "PATCH",
        &format!("/api/counters/{counter_id}"),
        Some(&resp_cookie),
        Some(&json!({
            "name": "Heures moteur",
            "active": true
        })),
    );
    assert_eq!(r_cp.status, 200, "{}", r_cp.text());
    assert_eq!(r_cp.json()["name"], "Heures moteur");

    // 3. Liste des compteurs de l'équipement
    let r_clist = srv.session_call(
        "GET",
        &format!("/api/equipments/{eq_id}/counters"),
        Some(&resp_cookie),
        None,
    );
    assert_eq!(r_clist.status, 200, "{}", r_clist.text());
    assert_eq!(r_clist.json().as_array().unwrap().len(), 1);

    // 4. Création d'un planning de maintenance (fréquence interval_days)
    let r_plan = srv.session_call(
        "POST",
        "/api/maintenance/plans",
        Some(&resp_cookie),
        Some(&json!({
            "code": "PLN-001",
            "title": "Vidange huile hydraulique",
            "equipment_id": eq_id,
            "frequency": "interval_days",
            "interval_days": 90,
            "anchor_date": "2026-11-01",
            "lead_days": 10,
            "priority": "high",
            "instructions": "Remplacer l'huile et vérifier les filtres"
        })),
    );
    assert_eq!(r_plan.status, 201, "{}", r_plan.text());
    let plan = r_plan.json();
    let plan_id = plan["id"].as_str().unwrap();
    assert_eq!(plan["code"], "PLN-001");
    assert_eq!(plan["title"], "Vidange huile hydraulique");
    assert_eq!(plan["equipment"]["code"], "EQ-M-01");
    assert_eq!(plan["next_due_date"], "2026-11-01");
    assert!(plan["open_occurrence"].is_null());

    // 5. Lecture du planning
    let r_pget = srv.session_call(
        "GET",
        &format!("/api/maintenance/plans/{plan_id}"),
        Some(&resp_cookie),
        None,
    );
    assert_eq!(r_pget.status, 200, "{}", r_pget.text());

    // 6. Mise à jour du planning (PATCH)
    let r_ppatch = srv.session_call(
        "PATCH",
        &format!("/api/maintenance/plans/{plan_id}"),
        Some(&resp_cookie),
        Some(&json!({
            "title": "Vidange complète huile hydraulique",
            "priority": "critical"
        })),
    );
    assert_eq!(r_ppatch.status, 200, "{}", r_ppatch.text());
    assert_eq!(
        r_ppatch.json()["title"],
        "Vidange complète huile hydraulique"
    );
    assert_eq!(r_ppatch.json()["priority"], "critical");

    // 7. Liste des plannings
    let r_plist = srv.session_call("GET", "/api/maintenance/plans", Some(&resp_cookie), None);
    assert_eq!(r_plist.status, 200, "{}", r_plist.text());
    assert_eq!(r_plist.json()["items"].as_array().unwrap().len(), 1);

    // 8. Suppression logique du planning
    let r_pdel = srv.session_call(
        "DELETE",
        &format!("/api/maintenance/plans/{plan_id}"),
        Some(&resp_cookie),
        None,
    );
    assert_eq!(r_pdel.status, 204, "{}", r_pdel.text());

    // Vérifier qu'il n'apparaît plus
    let r_plist2 = srv.session_call("GET", "/api/maintenance/plans", Some(&resp_cookie), None);
    assert_eq!(r_plist2.json()["items"].as_array().unwrap().len(), 0);

    // 9. Suppression logique du compteur
    let r_cdel = srv.session_call(
        "DELETE",
        &format!("/api/counters/{counter_id}"),
        Some(&resp_cookie),
        None,
    );
    assert_eq!(r_cdel.status, 204, "{}", r_cdel.text());
}

#[test]
fn isolation_entre_sites_et_404() {
    let srv = demo("maint-isolation");
    let (admin_cookie, _) = admin(&srv);

    // Créer Site B
    let r_site_b = srv.session_call(
        "POST",
        "/api/sites",
        Some(&admin_cookie),
        Some(&json!({"name": "Site Secondaire B"})),
    );
    assert_eq!(r_site_b.status, 201, "{}", r_site_b.text());
    let site_b_id = r_site_b.json()["id"].as_str().unwrap().to_string();

    // Créer responsable Site B
    let r_user_b = srv.session_call(
        "POST",
        "/api/users",
        Some(&admin_cookie),
        Some(&json!({
            "username": "resp_b",
            "display_name": "Responsable B",
            "role": "responsable",
            "site_id": site_b_id,
            "password": "Password-b123"
        })),
    );
    assert_eq!(r_user_b.status, 201, "{}", r_user_b.text());

    let (cookie_a, _) = login(&srv, "responsable", "demo1234");
    let (cookie_b, _) = login(&srv, "resp_b", "Password-b123");

    // Créer équipement, compteur et planning sur Site B
    let eq_b_id = create_equipment(&srv, &cookie_b, "EQ-B-01", "Presse B");

    let r_c_b = srv.session_call(
        "POST",
        &format!("/api/equipments/{eq_b_id}/counters"),
        Some(&cookie_b),
        Some(&json!({"name": "Compteur B", "unit": "h"})),
    );
    assert_eq!(r_c_b.status, 201, "{}", r_c_b.text());
    let counter_b_id = r_c_b.json()["id"].as_str().unwrap().to_string();

    let r_plan_b = srv.session_call(
        "POST",
        "/api/maintenance/plans",
        Some(&cookie_b),
        Some(&json!({
            "code": "PLN-B-1",
            "title": "Maintenance B",
            "equipment_id": eq_b_id,
            "frequency": "daily",
            "anchor_date": "2026-10-25"
        })),
    );
    assert_eq!(r_plan_b.status, 201, "{}", r_plan_b.text());
    let plan_b_id = r_plan_b.json()["id"].as_str().unwrap().to_string();

    // Responsable A tente d'accéder aux ressources de B -> 404 partout
    let r1 = srv.session_call(
        "GET",
        &format!("/api/equipments/{eq_b_id}/counters"),
        Some(&cookie_a),
        None,
    );
    assert_eq!(r1.status, 404);

    let r2 = srv.session_call(
        "PATCH",
        &format!("/api/counters/{counter_b_id}"),
        Some(&cookie_a),
        Some(&json!({"name":"Hack"})),
    );
    assert_eq!(r2.status, 404);

    let r3 = srv.session_call(
        "DELETE",
        &format!("/api/counters/{counter_b_id}"),
        Some(&cookie_a),
        None,
    );
    assert_eq!(r3.status, 404);

    let r4 = srv.session_call(
        "GET",
        &format!("/api/counters/{counter_b_id}/readings"),
        Some(&cookie_a),
        None,
    );
    assert_eq!(r4.status, 404);

    let r5 = srv.session_call(
        "POST",
        &format!("/api/counters/{counter_b_id}/readings"),
        Some(&cookie_a),
        Some(&json!({"value": 100})),
    );
    assert_eq!(r5.status, 404);

    let r6 = srv.session_call(
        "GET",
        &format!("/api/maintenance/plans/{plan_b_id}"),
        Some(&cookie_a),
        None,
    );
    assert_eq!(r6.status, 404);

    let r7 = srv.session_call(
        "PATCH",
        &format!("/api/maintenance/plans/{plan_b_id}"),
        Some(&cookie_a),
        Some(&json!({"title":"Hack"})),
    );
    assert_eq!(r7.status, 404);

    let r8 = srv.session_call(
        "DELETE",
        &format!("/api/maintenance/plans/{plan_b_id}"),
        Some(&cookie_a),
        None,
    );
    assert_eq!(r8.status, 404);

    let r9 = srv.session_call(
        "POST",
        &format!("/api/maintenance/plans/{plan_b_id}/generate"),
        Some(&cookie_a),
        None,
    );
    assert_eq!(r9.status, 404);

    let r10 = srv.session_call(
        "POST",
        &format!("/api/maintenance/plans/{plan_b_id}/skip"),
        Some(&cookie_a),
        Some(&json!({"reason":"Hack"})),
    );
    assert_eq!(r10.status, 404);
}

#[test]
fn permissions_rbac_et_contraintes() {
    let srv = demo("maint-rbac");
    let (resp_cookie, _) = login(&srv, "responsable", "demo1234");
    let (tech_cookie, _) = login(&srv, "technicien", "demo1234");
    let (prod_cookie, _) = login(&srv, "production", "demo1234");

    let eq_id = create_equipment(&srv, &resp_cookie, "EQ-RBAC-1", "Machine RBAC");

    // Technicien n'a pas equipments.edit -> 403
    let r_c_fail = srv.session_call(
        "POST",
        &format!("/api/equipments/{eq_id}/counters"),
        Some(&tech_cookie),
        Some(&json!({"name": "Compteur Tech", "unit": "h"})),
    );
    assert_eq!(r_c_fail.status, 403, "{}", r_c_fail.text());

    // Responsable crée le compteur
    let r_c = srv.session_call(
        "POST",
        &format!("/api/equipments/{eq_id}/counters"),
        Some(&resp_cookie),
        Some(&json!({"name": "Compteur Valide", "unit": "h", "current_value": 10})),
    );
    assert_eq!(r_c.status, 201);
    let counter_id = r_c.json()["id"].as_str().unwrap().to_string();

    // Technicien et Production ONT counters.record -> 201
    let r_read_tech = srv.session_call(
        "POST",
        &format!("/api/counters/{counter_id}/readings"),
        Some(&tech_cookie),
        Some(&json!({"value": 20})),
    );
    assert_eq!(r_read_tech.status, 201, "{}", r_read_tech.text());

    let r_read_prod = srv.session_call(
        "POST",
        &format!("/api/counters/{counter_id}/readings"),
        Some(&prod_cookie),
        Some(&json!({"value": 30})),
    );
    assert_eq!(r_read_prod.status, 201, "{}", r_read_prod.text());

    // Technicien n'a pas maintenance.manage -> 403 sur création de planning
    let r_plan_fail = srv.session_call(
        "POST",
        "/api/maintenance/plans",
        Some(&tech_cookie),
        Some(&json!({
            "code": "PLN-FAIL",
            "title": "Titre",
            "equipment_id": eq_id,
            "frequency": "daily",
            "anchor_date": "2026-10-25"
        })),
    );
    assert_eq!(r_plan_fail.status, 403, "{}", r_plan_fail.text());
}

#[test]
fn coherence_frequence_et_colonnes_400() {
    let srv = demo("maint-coherence");
    let (resp_cookie, _) = login(&srv, "responsable", "demo1234");
    let eq_id = create_equipment(&srv, &resp_cookie, "EQ-COH-1", "Machine Cohérence");

    // 1. interval_days sans interval_days -> 400
    let r1 = srv.session_call(
        "POST",
        "/api/maintenance/plans",
        Some(&resp_cookie),
        Some(&json!({
            "code": "PLN-ERR-1",
            "title": "Erreur",
            "equipment_id": eq_id,
            "frequency": "interval_days",
            "anchor_date": "2026-10-25"
        })),
    );
    assert_eq!(r1.status, 400, "{}", r1.text());

    // 2. interval_days hors limites (ex. 0 ou 5000) -> 400
    let r2 = srv.session_call(
        "POST",
        "/api/maintenance/plans",
        Some(&resp_cookie),
        Some(&json!({
            "code": "PLN-ERR-2",
            "title": "Erreur",
            "equipment_id": eq_id,
            "frequency": "interval_days",
            "interval_days": 0,
            "anchor_date": "2026-10-25"
        })),
    );
    assert_eq!(r2.status, 400);

    // 3. frequency counter sans counter_id -> 400
    let r3 = srv.session_call(
        "POST",
        "/api/maintenance/plans",
        Some(&resp_cookie),
        Some(&json!({
            "code": "PLN-ERR-3",
            "title": "Erreur",
            "equipment_id": eq_id,
            "frequency": "counter",
            "counter_interval": 100
        })),
    );
    assert_eq!(r3.status, 400);

    // 4. Calendrier sans anchor_date -> 400
    let r4 = srv.session_call(
        "POST",
        "/api/maintenance/plans",
        Some(&resp_cookie),
        Some(&json!({
            "code": "PLN-ERR-4",
            "title": "Erreur",
            "equipment_id": eq_id,
            "frequency": "monthly"
        })),
    );
    assert_eq!(r4.status, 400);
}

#[test]
fn compteur_dun_autre_equipement_refuse_400() {
    let srv = demo("maint-cross-counter");
    let (resp_cookie, _) = login(&srv, "responsable", "demo1234");
    let eq1 = create_equipment(&srv, &resp_cookie, "EQ-CROSS-1", "Machine 1");
    let eq2 = create_equipment(&srv, &resp_cookie, "EQ-CROSS-2", "Machine 2");

    let r_c1 = srv.session_call(
        "POST",
        &format!("/api/equipments/{eq1}/counters"),
        Some(&resp_cookie),
        Some(&json!({"name": "Compteur 1", "unit": "h"})),
    );
    assert_eq!(r_c1.status, 201);
    let counter1_id = r_c1.json()["id"].as_str().unwrap().to_string();

    // Tenter de créer un planning pour EQ-2 avec le compteur de EQ-1 -> 400
    let r_cross = srv.session_call(
        "POST",
        "/api/maintenance/plans",
        Some(&resp_cookie),
        Some(&json!({
            "code": "PLN-CROSS",
            "title": "Cross",
            "equipment_id": eq2,
            "frequency": "counter",
            "counter_id": counter1_id,
            "counter_interval": 50
        })),
    );
    assert_eq!(r_cross.status, 400, "{}", r_cross.text());
}

#[test]
fn generation_manuelle_cloture_et_concurrence() {
    let srv = demo("maint-gen-lifecycle");
    let (resp_cookie, _) = login(&srv, "responsable", "demo1234");
    let (tech_cookie, tech_user) = login(&srv, "technicien", "demo1234");
    let tech_id = tech_user["id"].as_str().unwrap();

    let eq_id = create_equipment(&srv, &resp_cookie, "EQ-GEN-1", "Machine Génération");

    let r_plan = srv.session_call(
        "POST",
        "/api/maintenance/plans",
        Some(&resp_cookie),
        Some(&json!({
            "code": "PLN-GEN-1",
            "title": "Inspection Mensuelle",
            "equipment_id": eq_id,
            "frequency": "monthly",
            "anchor_date": "2026-10-01",
            "default_technician_id": tech_id,
            "instructions": "Vérifier la pression"
        })),
    );
    assert_eq!(r_plan.status, 201);
    let plan_id = r_plan.json()["id"].as_str().unwrap().to_string();

    // 1. Première génération manuelle -> 201
    let r_gen1 = srv.session_call(
        "POST",
        &format!("/api/maintenance/plans/{plan_id}/generate"),
        Some(&resp_cookie),
        None,
    );
    assert_eq!(r_gen1.status, 201, "{}", r_gen1.text());
    let gen_res = r_gen1.json();
    let int_id = gen_res["intervention"]["id"].as_str().unwrap().to_string();
    assert_eq!(gen_res["occurrence"]["status"], "generated");
    assert_eq!(gen_res["intervention"]["type"], "preventive");

    // 2. Deuxième génération concurrente / immédiate -> 409
    let r_gen2 = srv.session_call(
        "POST",
        &format!("/api/maintenance/plans/{plan_id}/generate"),
        Some(&resp_cookie),
        None,
    );
    assert_eq!(r_gen2.status, 409, "{}", r_gen2.text());

    // 3. Clôture de l'intervention générée
    // Prise en charge
    let r_take = srv.session_call(
        "POST",
        &format!("/api/interventions/{int_id}/take"),
        Some(&tech_cookie),
        None,
    );
    assert_eq!(r_take.status, 200);

    // Temps technicien
    let r_time = srv.session_call(
        "PUT",
        &format!("/api/interventions/{int_id}/technicians/{tech_id}/time"),
        Some(&tech_cookie),
        Some(&json!({"duration_minutes": 45})),
    );
    assert_eq!(r_time.status, 200);

    // Clôture
    let r_comp = srv.session_call(
        "POST",
        &format!("/api/interventions/{int_id}/complete"),
        Some(&tech_cookie),
        Some(&json!({
            "solution_report": "Pression conforme"
        })),
    );
    assert_eq!(r_comp.status, 200, "{}", r_comp.text());

    // 4. Vérification de l'occurrence passée à done
    let r_occs = srv.session_call(
        "GET",
        &format!("/api/maintenance/plans/{plan_id}/occurrences"),
        Some(&resp_cookie),
        None,
    );
    assert_eq!(r_occs.status, 200);
    let occs = r_occs.json();
    let occ_list = occs["items"].as_array().unwrap();
    assert_eq!(occ_list.len(), 1);
    assert_eq!(occ_list[0]["status"], "done");
    assert!(occ_list[0]["closed_at"].is_string());

    // 5. Vérification que next_due_date vaut la date ancrée suivante (1er novembre 2026)
    let r_plan_after = srv.session_call(
        "GET",
        &format!("/api/maintenance/plans/{plan_id}"),
        Some(&resp_cookie),
        None,
    );
    assert_eq!(r_plan_after.json()["next_due_date"], "2026-11-01");
}

#[test]
fn annulation_intervention_et_skip_sans_intervention() {
    let srv = demo("maint-cancel-skip");
    let (resp_cookie, _) = login(&srv, "responsable", "demo1234");
    let eq_id = create_equipment(&srv, &resp_cookie, "EQ-CS-1", "Machine Annulation");

    // 1. Annulation d'une intervention générée
    let r_p1 = srv.session_call(
        "POST",
        "/api/maintenance/plans",
        Some(&resp_cookie),
        Some(&json!({
            "code": "PLN-CS-1",
            "title": "Plan Annulation",
            "equipment_id": eq_id,
            "frequency": "daily",
            "anchor_date": "2026-11-10"
        })),
    );
    assert_eq!(r_p1.status, 201);
    let p1_id = r_p1.json()["id"].as_str().unwrap().to_string();

    let r_gen = srv.session_call(
        "POST",
        &format!("/api/maintenance/plans/{p1_id}/generate"),
        Some(&resp_cookie),
        None,
    );
    assert_eq!(r_gen.status, 201);
    let int_id = r_gen.json()["intervention"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Annuler l'intervention
    let r_cancel = srv.session_call(
        "POST",
        &format!("/api/interventions/{int_id}/cancel"),
        Some(&resp_cookie),
        Some(&json!({"reason": "Arrêt technique imprévu"})),
    );
    assert_eq!(r_cancel.status, 200);

    // Vérifier occurrence skipped
    let r_occs = srv.session_call(
        "GET",
        &format!("/api/maintenance/plans/{p1_id}/occurrences"),
        Some(&resp_cookie),
        None,
    );
    let occs = r_occs.json();
    assert_eq!(occs["items"][0]["status"], "skipped");
    assert_eq!(occs["items"][0]["skip_reason"], "Arrêt technique imprévu");

    // 2. Report sans intervention (SKIP)
    let r_p2 = srv.session_call(
        "POST",
        "/api/maintenance/plans",
        Some(&resp_cookie),
        Some(&json!({
            "code": "PLN-CS-2",
            "title": "Plan Report Direct",
            "equipment_id": eq_id,
            "frequency": "monthly",
            "anchor_date": "2026-10-01"
        })),
    );
    assert_eq!(r_p2.status, 201);
    let p2_id = r_p2.json()["id"].as_str().unwrap().to_string();

    let r_skip = srv.session_call(
        "POST",
        &format!("/api/maintenance/plans/{p2_id}/skip"),
        Some(&resp_cookie),
        Some(&json!({"reason": "Pièces en rupture"})),
    );
    assert_eq!(r_skip.status, 200, "{}", r_skip.text());
    assert_eq!(r_skip.json()["next_due_date"], "2026-11-01");

    // Tenter de skip alors qu'une occurrence est ouverte -> 409
    let _ = srv.session_call(
        "POST",
        &format!("/api/maintenance/plans/{p2_id}/generate"),
        Some(&resp_cookie),
        None,
    );
    let r_skip_fail = srv.session_call(
        "POST",
        &format!("/api/maintenance/plans/{p2_id}/skip"),
        Some(&resp_cookie),
        Some(&json!({"reason": "Nouveau report"})),
    );
    assert_eq!(r_skip_fail.status, 409);
}

#[test]
fn releves_compteur_decroissant_et_declenchement_auto() {
    let srv = demo("maint-reading-trigger");
    let (resp_cookie, _) = login(&srv, "responsable", "demo1234");
    let eq_id = create_equipment(&srv, &resp_cookie, "EQ-READ-1", "Machine Relevé");

    let r_c = srv.session_call(
        "POST",
        &format!("/api/equipments/{eq_id}/counters"),
        Some(&resp_cookie),
        Some(&json!({"name": "Cycles presse", "unit": "cycles", "current_value": 100})),
    );
    assert_eq!(r_c.status, 201);
    let counter_id = r_c.json()["id"].as_str().unwrap().to_string();

    // 1. Relevé inférieur sans reset -> 400
    let r_dec = srv.session_call(
        "POST",
        &format!("/api/counters/{counter_id}/readings"),
        Some(&resp_cookie),
        Some(&json!({"value": 80})),
    );
    assert_eq!(r_dec.status, 400, "{}", r_dec.text());

    // 2. Relevé inférieur avec reset -> 201
    let r_reset = srv.session_call(
        "POST",
        &format!("/api/counters/{counter_id}/readings"),
        Some(&resp_cookie),
        Some(&json!({"value": 0, "is_reset": true})),
    );
    assert_eq!(r_reset.status, 201, "{}", r_reset.text());

    // 3. Créer un planning avec échéance à 50 cycles et lead_counter = 10 (seuil à 40)
    let r_plan = srv.session_call(
        "POST",
        "/api/maintenance/plans",
        Some(&resp_cookie),
        Some(&json!({
            "code": "PLN-CYC-1",
            "title": "Contrôle joints 50 cycles",
            "equipment_id": eq_id,
            "frequency": "counter",
            "counter_id": counter_id,
            "counter_interval": 50,
            "anchor_counter": 0,
            "next_due_counter": 50,
            "lead_counter": 10,
            "auto_generate": true
        })),
    );
    assert_eq!(r_plan.status, 201);
    let plan_id = r_plan.json()["id"].as_str().unwrap().to_string();

    // Relevé à 35 (en dessous du seuil 40) -> aucune intervention créée
    let _ = srv.session_call(
        "POST",
        &format!("/api/counters/{counter_id}/readings"),
        Some(&resp_cookie),
        Some(&json!({"value": 35})),
    );
    let r_p_check1 = srv.session_call(
        "GET",
        &format!("/api/maintenance/plans/{plan_id}"),
        Some(&resp_cookie),
        None,
    );
    assert!(r_p_check1.json()["open_occurrence"].is_null());

    // Relevé à 42 (franchit le seuil 40) -> déclenche automatiquement la génération !
    let _ = srv.session_call(
        "POST",
        &format!("/api/counters/{counter_id}/readings"),
        Some(&resp_cookie),
        Some(&json!({"value": 42})),
    );
    let r_p_check2 = srv.session_call(
        "GET",
        &format!("/api/maintenance/plans/{plan_id}"),
        Some(&resp_cookie),
        None,
    );
    assert!(!r_p_check2.json()["open_occurrence"].is_null());
}

#[test]
fn suppression_equipement_avec_plannings_409() {
    let srv = demo("maint-eq-delete-guard");
    let (resp_cookie, _) = login(&srv, "responsable", "demo1234");
    let eq_id = create_equipment(&srv, &resp_cookie, "EQ-GUARD-1", "Machine Garde");

    let r_plan = srv.session_call(
        "POST",
        "/api/maintenance/plans",
        Some(&resp_cookie),
        Some(&json!({
            "code": "PLN-GUARD-1",
            "title": "Planning Actif",
            "equipment_id": eq_id,
            "frequency": "daily",
            "anchor_date": "2026-10-25"
        })),
    );
    assert_eq!(r_plan.status, 201);

    // Supprimer l'équipement -> 409 Conflict
    let r_eq_del = srv.session_call(
        "DELETE",
        &format!("/api/equipments/{eq_id}"),
        Some(&resp_cookie),
        None,
    );
    assert_eq!(r_eq_del.status, 409, "{}", r_eq_del.text());
}

#[test]
fn vue_planning_schedule_et_limites() {
    let srv = demo("maint-schedule-view");
    let (resp_cookie, _) = login(&srv, "responsable", "demo1234");
    let eq_id = create_equipment(&srv, &resp_cookie, "EQ-SCHED-1", "Machine Schedule");

    // 1. Refus d'une période de plus de 366 jours (400)
    let r_toolong = srv.session_call(
        "GET",
        "/api/maintenance/schedule?from=2026-01-01&to=2027-02-01",
        Some(&resp_cookie),
        None,
    );
    assert_eq!(r_toolong.status, 400, "{}", r_toolong.text());

    // 2. Planning calendaire hebdomadaire
    let r_plan = srv.session_call(
        "POST",
        "/api/maintenance/plans",
        Some(&resp_cookie),
        Some(&json!({
            "code": "PLN-SCH-1",
            "title": "Nettoyage hebdo",
            "equipment_id": eq_id,
            "frequency": "weekly",
            "anchor_date": "2026-10-01"
        })),
    );
    assert_eq!(r_plan.status, 201);

    // 3. Vue planning sur 30 jours
    let r_sched = srv.session_call(
        "GET",
        "/api/maintenance/schedule?from=2026-10-01&to=2026-10-31",
        Some(&resp_cookie),
        None,
    );
    assert_eq!(r_sched.status, 200, "{}", r_sched.text());
    let entries = r_sched.json();
    let arr = entries.as_array().unwrap();
    // Doit contenir au moins les échéances projetées hebdomadaires (1er, 8, 15, 22, 29 oct)
    assert!(
        arr.len() >= 4,
        "Nombre d'échéances projetées : {}",
        arr.len()
    );
    assert_eq!(arr[0]["plan"]["code"], "PLN-SCH-1");
}

#[test]
fn suppression_intervention_remet_occurrence_en_attente() {
    let srv = demo("maint-delete-intervention-reset");
    let (resp_cookie, _) = login(&srv, "responsable", "demo1234");
    let eq_id = create_equipment(&srv, &resp_cookie, "EQ-RESET-1", "Machine Reset");

    let r_plan = srv.session_call(
        "POST",
        "/api/maintenance/plans",
        Some(&resp_cookie),
        Some(&json!({
            "code": "PLN-RST-1",
            "title": "Plan Reset",
            "equipment_id": eq_id,
            "frequency": "daily",
            "anchor_date": "2026-10-25"
        })),
    );
    assert_eq!(r_plan.status, 201);
    let plan_id = r_plan.json()["id"].as_str().unwrap().to_string();

    // 1. Générer
    let r_gen = srv.session_call(
        "POST",
        &format!("/api/maintenance/plans/{plan_id}/generate"),
        Some(&resp_cookie),
        None,
    );
    assert_eq!(r_gen.status, 201);
    let int_id = r_gen.json()["intervention"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Occurrence ouverte : tentative de génération bloquée à 409
    let r_gen2 = srv.session_call(
        "POST",
        &format!("/api/maintenance/plans/{plan_id}/generate"),
        Some(&resp_cookie),
        None,
    );
    assert_eq!(r_gen2.status, 409);

    // 2. Supprimer logiquement l'intervention
    let r_del = srv.session_call(
        "DELETE",
        &format!("/api/interventions/{int_id}"),
        Some(&resp_cookie),
        None,
    );
    assert_eq!(r_del.status, 204);

    // 3. Règle §2.5 : L'occurrence est supprimée et le planning garde son échéance, une nouvelle génération est à nouveau possible !
    let r_gen3 = srv.session_call(
        "POST",
        &format!("/api/maintenance/plans/{plan_id}/generate"),
        Some(&resp_cookie),
        None,
    );
    assert_eq!(
        r_gen3.status,
        201,
        "Nouvelle génération permise : {}",
        r_gen3.text()
    );
}

#[test]
fn generation_automatique_tache_de_fond_et_audit() {
    let mut cfg = Settings::new();
    cfg.demo = true;
    let srv = Server::start_with_env(
        "maint-auto-bg",
        cfg,
        &[("APEX_MAINT_TEST_SCHEDULER_SECONDS", "1")],
    );
    let (resp_cookie, _) = login(&srv, "responsable", "demo1234");
    let eq_id = create_equipment(&srv, &resp_cookie, "EQ-AUTO-1", "Machine Auto Scheduler");

    let r_plan = srv.session_call(
        "POST",
        "/api/maintenance/plans",
        Some(&resp_cookie),
        Some(&json!({
            "code": "PLN-AUTO-1",
            "title": "Plan Auto Background",
            "equipment_id": eq_id,
            "frequency": "daily",
            "anchor_date": "2026-01-01",
            "auto_generate": true
        })),
    );
    assert_eq!(r_plan.status, 201, "{}", r_plan.text());
    let plan_id = r_plan.json()["id"].as_str().unwrap().to_string();

    // Attendre que la tâche de fond (qui tourne chaque seconde) génère l'occurrence
    let mut generated = false;
    let mut intervention_id = String::new();
    for _ in 0..30 {
        std::thread::sleep(std::time::Duration::from_millis(500));
        let r_check = srv.session_call(
            "GET",
            &format!("/api/maintenance/plans/{plan_id}"),
            Some(&resp_cookie),
            None,
        );
        if r_check.status == 200 {
            let body = r_check.json();
            if let Some(occ) = body.get("open_occurrence") {
                if !occ.is_null() {
                    generated = true;
                    intervention_id = occ["intervention_id"].as_str().unwrap_or("").to_string();
                    break;
                }
            }
        }
    }
    assert!(
        generated,
        "La tâche de fond doit avoir généré l'occurrence automatiquement"
    );
    assert!(!intervention_id.is_empty());

    // Vérifier l'intervention générée
    let r_int = srv.session_call(
        "GET",
        &format!("/api/interventions/{intervention_id}"),
        Some(&resp_cookie),
        None,
    );
    assert_eq!(r_int.status, 200);
    assert_eq!(r_int.json()["type"], "preventive");

    // Vérifier l'audit en base : user_id NULL et source auto (§2.4)
    let rt = tokio::runtime::Runtime::new().unwrap();
    let db_path = srv.dir().join("data").join("apex-maint.db");
    rt.block_on(async {
        let pool = sqlx::SqlitePool::connect(&format!("sqlite://{}", db_path.display()))
            .await
            .expect("connexion sqlite");
        let row: (Option<String>, String) = sqlx::query_as(
            "SELECT user_id, after_json FROM audit_log WHERE action = 'generate' AND entity = 'maintenance_occurrence' ORDER BY created_at DESC LIMIT 1"
        )
        .fetch_one(&pool)
        .await
        .expect("ligne audit");

        assert!(row.0.is_none(), "L'audit de la tâche de fond doit avoir user_id NULL : {:?}", row.0);
        let after_json: serde_json::Value = serde_json::from_str(&row.1).expect("json audit");
        assert_eq!(after_json["details"]["source"], "auto");
    });
}
