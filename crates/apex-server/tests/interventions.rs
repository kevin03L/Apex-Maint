//! Tests boîte noire des interventions de maintenance (Phase 3).
//!
//! Couvre l'ensemble des exigences de docs/specs/phase-3.md §6 et les décisions du 2026-10-18 (§7) :
//! - Cycle de vie standard (open -> in_progress -> completed)
//! - Clôture directe / a posteriori
//! - Refus des transitions illégales (409 Conflict)
//! - Contrainte CHECK sur equipment_id (défaut I1)
//! - Recalcul dynamique du statut opérationnel de l'équipement
//! - Sous-interventions, profondeur 1 et blocage de clôture/suppression (409)
//! - Gestion des techniciens assignés et saisie des temps par technicien (§7.3)
//! - Annulation d'intervention avec motif et recalcul équipement (§7.1)
//! - Isolation multi-sites (404) et permissions RBAC (403)
//! - Pagination keyset par curseur et filtres combinés

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
fn cycle_de_vie_standard_et_temps_technicien() {
    let srv = demo("int-std-lifecycle");
    let (resp_cookie, _resp_user) = login(&srv, "responsable", "demo1234");
    let (tech_cookie, tech_user) = login(&srv, "technicien", "demo1234");
    let tech_id = tech_user["id"].as_str().unwrap();

    let eq_id = create_equipment(&srv, &resp_cookie, "EQ-STD-1", "Machine Standard 1");

    // 1. Création open
    let r_create = srv.session_call(
        "POST",
        "/api/interventions",
        Some(&resp_cookie),
        Some(&json!({
            "title": "Intervention préventive semestrielle",
            "type": "preventive",
            "priority": "high",
            "machine_state": "FONCTIONNEL",
            "equipment_id": eq_id
        })),
    );
    assert_eq!(r_create.status, 201, "{}", r_create.text());
    let int_created = r_create.json();
    let int_id = int_created["id"].as_str().unwrap();
    assert_eq!(int_created["status"], "open");
    assert!(int_created["code"].as_str().unwrap().starts_with("INT-"));

    // 2. Prise en charge par le technicien (/take)
    let r_take = srv.session_call(
        "POST",
        &format!("/api/interventions/{int_id}/take"),
        Some(&tech_cookie),
        None,
    );
    assert_eq!(r_take.status, 200, "{}", r_take.text());
    let int_taken = r_take.json();
    assert_eq!(int_taken["status"], "in_progress");
    assert!(int_taken["started_at"].is_string());
    // Le technicien qui prend en charge est automatiquement assigné comme lead
    let techs = int_taken["technicians"].as_array().unwrap();
    assert_eq!(techs.len(), 1);
    assert_eq!(techs[0]["user_id"], tech_id);
    assert_eq!(techs[0]["is_lead"], true);

    // 3. Tentative de clôture sans temps -> refusée (400 Bad Request selon §7.3)
    let r_complete_fail = srv.session_call(
        "POST",
        &format!("/api/interventions/{int_id}/complete"),
        Some(&tech_cookie),
        Some(&json!({
            "solution_report": "Maintenance préventive effectuée conformément à la gamme."
        })),
    );
    assert_eq!(r_complete_fail.status, 400, "{}", r_complete_fail.text());

    // 4. Saisie du temps par le technicien (/technicians/{user_id}/time)
    let r_time = srv.session_call(
        "PUT",
        &format!("/api/interventions/{int_id}/technicians/{tech_id}/time"),
        Some(&tech_cookie),
        Some(&json!({ "duration_minutes": 45 })),
    );
    assert_eq!(r_time.status, 200, "{}", r_time.text());
    assert_eq!(r_time.json()["duration_minutes"], 45);

    // 5. Clôture avec compte-rendu
    let r_complete = srv.session_call(
        "POST",
        &format!("/api/interventions/{int_id}/complete"),
        Some(&tech_cookie),
        Some(&json!({
            "solution_report": "Maintenance préventive effectuée avec succès. Nettoyage et graissage."
        })),
    );
    assert_eq!(r_complete.status, 200, "{}", r_complete.text());
    let int_completed = r_complete.json();
    assert_eq!(int_completed["status"], "completed");
    assert_eq!(int_completed["duration_minutes"], 45);
    assert!(int_completed["completed_at"].is_string());
    assert!(int_completed["solution_report"].is_string());
}

#[test]
fn cloture_directe_et_a_posteriori() {
    let srv = demo("int-aposteriori");
    let (resp_cookie, _) = login(&srv, "responsable", "demo1234");
    let (tech_cookie, tech_user) = login(&srv, "technicien", "demo1234");
    let tech_id = tech_user["id"].as_str().unwrap();

    let eq_id = create_equipment(&srv, &resp_cookie, "EQ-POST-1", "Convoyeur A");

    // Création directe d'une intervention a_posteriori
    let r_create = srv.session_call(
        "POST",
        "/api/interventions",
        Some(&tech_cookie),
        Some(&json!({
            "title": "Dépannage d'urgence tapis convoyeur",
            "type": "a_posteriori",
            "priority": "critical",
            "machine_state": "ARRET",
            "equipment_id": eq_id,
            "solution_report": "Remplacement de la courroie rompue et recalage du tendeur.",
            "duration_minutes": 60,
            "technician_ids": [tech_id]
        })),
    );
    assert_eq!(r_create.status, 201, "{}", r_create.text());
    let int = r_create.json();
    assert_eq!(int["status"], "completed");
    assert_eq!(int["type"], "a_posteriori");
    assert_eq!(int["duration_minutes"], 60);
    assert!(int["started_at"].is_string());
    assert!(int["completed_at"].is_string());
    assert_eq!(
        int["solution_report"],
        "Remplacement de la courroie rompue et recalage du tendeur."
    );
    let techs = int["technicians"].as_array().unwrap();
    assert_eq!(techs.len(), 1);
    assert_eq!(techs[0]["duration_minutes"], 60);
}

#[test]
fn refus_des_transitions_illegales() {
    let srv = demo("int-transitions");
    let (resp_cookie, _) = login(&srv, "responsable", "demo1234");
    let (tech_cookie, tech_user) = login(&srv, "technicien", "demo1234");
    let tech_id = tech_user["id"].as_str().unwrap();

    let eq_id = create_equipment(&srv, &resp_cookie, "EQ-TR-1", "Tour CN");

    let r_create = srv.session_call(
        "POST",
        "/api/interventions",
        Some(&tech_cookie),
        Some(&json!({
            "title": "Changement de plaquette",
            "type": "corrective",
            "priority": "medium",
            "machine_state": "FONCTIONNEL",
            "equipment_id": eq_id
        })),
    );
    assert_eq!(r_create.status, 201);
    let int_id = r_create.json()["id"].as_str().unwrap().to_string();

    // 1. Passage à in_progress
    let r_take = srv.session_call(
        "POST",
        &format!("/api/interventions/{int_id}/take"),
        Some(&tech_cookie),
        None,
    );
    assert_eq!(r_take.status, 200);

    // 2. Tentative de reprendre en charge alors que déjà in_progress -> 409
    let r_take_again = srv.session_call(
        "POST",
        &format!("/api/interventions/{int_id}/take"),
        Some(&tech_cookie),
        None,
    );
    assert_eq!(r_take_again.status, 409);

    // Saisie du temps
    let _ = srv.session_call(
        "PUT",
        &format!("/api/interventions/{int_id}/technicians/{tech_id}/time"),
        Some(&tech_cookie),
        Some(&json!({ "duration_minutes": 20 })),
    );

    // 3. Clôture
    let r_comp = srv.session_call(
        "POST",
        &format!("/api/interventions/{int_id}/complete"),
        Some(&tech_cookie),
        Some(&json!({ "solution_report": "Plaquette changée." })),
    );
    assert_eq!(r_comp.status, 200);

    // 4. Tentative de réouverture depuis completed vers in_progress (via take) -> 409
    let r_take_completed = srv.session_call(
        "POST",
        &format!("/api/interventions/{int_id}/take"),
        Some(&tech_cookie),
        None,
    );
    assert_eq!(r_take_completed.status, 409);

    // 5. Tentative de reclôture sur une intervention déjà completed -> 409
    let r_comp_completed = srv.session_call(
        "POST",
        &format!("/api/interventions/{int_id}/complete"),
        Some(&tech_cookie),
        Some(&json!({ "solution_report": "Encore" })),
    );
    assert_eq!(r_comp_completed.status, 409);

    // 6. Tentative d'annulation sur une intervention completed -> 409
    let r_cancel_completed = srv.session_call(
        "POST",
        &format!("/api/interventions/{int_id}/cancel"),
        Some(&tech_cookie),
        Some(&json!({ "reason": "Erreur" })),
    );
    assert_eq!(r_cancel_completed.status, 409);

    // 7. Tentative de modification (PATCH) sur une intervention completed -> 409
    let r_patch_completed = srv.session_call(
        "PATCH",
        &format!("/api/interventions/{int_id}"),
        Some(&tech_cookie),
        Some(&json!({ "title": "Nouveau titre" })),
    );
    assert_eq!(r_patch_completed.status, 409);
}

#[test]
fn contraintes_check_sur_equipment_id() {
    let srv = demo("int-equip-check");
    let (resp_cookie, _) = login(&srv, "responsable", "demo1234");
    let eq_id = create_equipment(&srv, &resp_cookie, "EQ-CHK-1", "Machine Test");

    // 1. Corrective sans equipment_id -> 400 Bad Request (défaut I1)
    let r1 = srv.session_call(
        "POST",
        "/api/interventions",
        Some(&resp_cookie),
        Some(&json!({
            "title": "Panne sans équipement",
            "type": "corrective",
            "priority": "medium",
            "equipment_id": null
        })),
    );
    assert_eq!(r1.status, 400, "{}", r1.text());

    // 2. Hors équipement avec equipment_id -> 400 Bad Request
    let r2 = srv.session_call(
        "POST",
        "/api/interventions",
        Some(&resp_cookie),
        Some(&json!({
            "title": "Bâtiment avec équipement",
            "type": "hors_equipement",
            "priority": "low",
            "equipment_id": eq_id
        })),
    );
    assert_eq!(r2.status, 400, "{}", r2.text());

    // 3. Hors équipement sans equipment_id -> 201 Created
    let r3 = srv.session_call(
        "POST",
        "/api/interventions",
        Some(&resp_cookie),
        Some(&json!({
            "title": "Réparation fuite toiture atelier",
            "type": "hors_equipement",
            "priority": "low"
        })),
    );
    assert_eq!(r3.status, 201, "{}", r3.text());
    assert!(r3.json()["equipment"].is_null());
}

#[test]
fn effet_sur_le_statut_de_equipement() {
    let srv = demo("int-equip-status");
    let (resp_cookie, _) = login(&srv, "responsable", "demo1234");
    let (tech_cookie, tech_user) = login(&srv, "technicien", "demo1234");
    let tech_id = tech_user["id"].as_str().unwrap();

    let eq_id = create_equipment(&srv, &resp_cookie, "EQ-STAT-1", "Presse Hydraulique");

    // Vérifie statut initial = operational
    let r_eq0 = srv.session_call(
        "GET",
        &format!("/api/equipments/{eq_id}"),
        Some(&resp_cookie),
        None,
    );
    assert_eq!(r_eq0.json()["status"], "operational");

    // 1. Création intervention avec machine_state = ARRET -> équipement bascule out_of_service
    let r_int1 = srv.session_call(
        "POST",
        "/api/interventions",
        Some(&tech_cookie),
        Some(&json!({
            "title": "Fuite d'huile majeure",
            "type": "corrective",
            "priority": "critical",
            "machine_state": "ARRET",
            "equipment_id": eq_id
        })),
    );
    assert_eq!(r_int1.status, 201);
    let int1_id = r_int1.json()["id"].as_str().unwrap().to_string();

    let r_eq1 = srv.session_call(
        "GET",
        &format!("/api/equipments/{eq_id}"),
        Some(&resp_cookie),
        None,
    );
    assert_eq!(r_eq1.json()["status"], "out_of_service");

    // 2. Deuxième intervention concurrente sur le même équipement avec machine_state = ARRET
    let r_int2 = srv.session_call(
        "POST",
        "/api/interventions",
        Some(&tech_cookie),
        Some(&json!({
            "title": "Consigne électrique en cours",
            "type": "corrective",
            "priority": "high",
            "machine_state": "ARRET",
            "equipment_id": eq_id
        })),
    );
    assert_eq!(r_int2.status, 201);
    let int2_id = r_int2.json()["id"].as_str().unwrap().to_string();

    // Saisie du temps et prise en charge sur int1
    let _ = srv.session_call(
        "POST",
        &format!("/api/interventions/{int1_id}/take"),
        Some(&tech_cookie),
        None,
    );
    let _ = srv.session_call(
        "PUT",
        &format!("/api/interventions/{int1_id}/technicians/{tech_id}/time"),
        Some(&tech_cookie),
        Some(&json!({ "duration_minutes": 30 })),
    );

    // 3. Clôture de int1 : int2 est toujours active à l'ARRET -> l'équipement reste out_of_service
    let r_comp1 = srv.session_call(
        "POST",
        &format!("/api/interventions/{int1_id}/complete"),
        Some(&tech_cookie),
        Some(&json!({ "solution_report": "Joint remplacé" })),
    );
    assert_eq!(r_comp1.status, 200);

    let r_eq2 = srv.session_call(
        "GET",
        &format!("/api/equipments/{eq_id}"),
        Some(&resp_cookie),
        None,
    );
    assert_eq!(
        r_eq2.json()["status"],
        "out_of_service",
        "L'équipement doit rester out_of_service tant que int2 est active"
    );

    // Saisie du temps et clôture de int2
    let _ = srv.session_call(
        "POST",
        &format!("/api/interventions/{int2_id}/take"),
        Some(&tech_cookie),
        None,
    );
    let _ = srv.session_call(
        "PUT",
        &format!("/api/interventions/{int2_id}/technicians/{tech_id}/time"),
        Some(&tech_cookie),
        Some(&json!({ "duration_minutes": 15 })),
    );
    let r_comp2 = srv.session_call(
        "POST",
        &format!("/api/interventions/{int2_id}/complete"),
        Some(&tech_cookie),
        Some(&json!({ "solution_report": "Déconsignation effectuée" })),
    );
    assert_eq!(r_comp2.status, 200);

    // 4. Plus aucune intervention active -> repasse automatiquement à operational
    let r_eq3 = srv.session_call(
        "GET",
        &format!("/api/equipments/{eq_id}"),
        Some(&resp_cookie),
        None,
    );
    assert_eq!(r_eq3.json()["status"], "operational");
}

#[test]
fn sous_interventions_blocage_cloture_et_profondeur() {
    let srv = demo("int-subs");
    let (resp_cookie, _) = login(&srv, "responsable", "demo1234");
    let (tech_cookie, tech_user) = login(&srv, "technicien", "demo1234");
    let tech_id = tech_user["id"].as_str().unwrap();

    let eq_id = create_equipment(&srv, &resp_cookie, "EQ-SUB-1", "Ligne de conditionnement");

    // 1. Intervention parent
    let r_p = srv.session_call(
        "POST",
        "/api/interventions",
        Some(&resp_cookie),
        Some(&json!({
            "title": "Rénovation générale de la ligne",
            "type": "corrective",
            "priority": "high",
            "machine_state": "FONCTIONNEL",
            "equipment_id": eq_id
        })),
    );
    assert_eq!(r_p.status, 201);
    let parent_id = r_p.json()["id"].as_str().unwrap().to_string();

    // 2. Sous-intervention via POST /sub-interventions
    let r_sub = srv.session_call(
        "POST",
        &format!("/api/interventions/{parent_id}/sub-interventions"),
        Some(&resp_cookie),
        Some(&json!({
            "title": "Sous-tâche : alignement des capteurs optiques",
            "type": "corrective",
            "priority": "medium",
            "machine_state": "FONCTIONNEL",
            "equipment_id": eq_id
        })),
    );
    assert_eq!(r_sub.status, 201, "{}", r_sub.text());
    let sub_id = r_sub.json()["id"].as_str().unwrap().to_string();
    assert_eq!(r_sub.json()["parent_id"], parent_id);

    // 3. Tentative de créer une sous-intervention sur la sous-intervention (profondeur > 1) -> 400
    let r_sub_sub = srv.session_call(
        "POST",
        &format!("/api/interventions/{sub_id}/sub-interventions"),
        Some(&resp_cookie),
        Some(&json!({
            "title": "Sous-sous-tâche interdite",
            "type": "corrective",
            "priority": "low",
            "equipment_id": eq_id
        })),
    );
    assert_eq!(r_sub_sub.status, 400, "{}", r_sub_sub.text());

    // Prise en charge et temps sur le parent
    let _ = srv.session_call(
        "POST",
        &format!("/api/interventions/{parent_id}/take"),
        Some(&tech_cookie),
        None,
    );
    let _ = srv.session_call(
        "PUT",
        &format!("/api/interventions/{parent_id}/technicians/{tech_id}/time"),
        Some(&tech_cookie),
        Some(&json!({ "duration_minutes": 120 })),
    );

    // 4. Tentative de clôturer le parent alors que la sous-intervention est active -> 409 Conflict
    let r_close_parent_fail = srv.session_call(
        "POST",
        &format!("/api/interventions/{parent_id}/complete"),
        Some(&tech_cookie),
        Some(&json!({ "solution_report": "Terminé prématurément" })),
    );
    assert_eq!(
        r_close_parent_fail.status,
        409,
        "{}",
        r_close_parent_fail.text()
    );

    // 5. Tentative de suppression du parent ayant des sous-interventions -> 409 Conflict
    let r_del_parent_fail = srv.session_call(
        "DELETE",
        &format!("/api/interventions/{parent_id}"),
        Some(&resp_cookie),
        None,
    );
    assert_eq!(r_del_parent_fail.status, 409);

    // Clôture de la sous-intervention
    let _ = srv.session_call(
        "POST",
        &format!("/api/interventions/{sub_id}/take"),
        Some(&tech_cookie),
        None,
    );
    let _ = srv.session_call(
        "PUT",
        &format!("/api/interventions/{sub_id}/technicians/{tech_id}/time"),
        Some(&tech_cookie),
        Some(&json!({ "duration_minutes": 30 })),
    );
    let r_close_sub = srv.session_call(
        "POST",
        &format!("/api/interventions/{sub_id}/complete"),
        Some(&tech_cookie),
        Some(&json!({ "solution_report": "Capteurs alignés." })),
    );
    assert_eq!(r_close_sub.status, 200);

    // 6. Clôture réussie du parent maintenant que la sous-intervention est terminée
    let r_close_parent_ok = srv.session_call(
        "POST",
        &format!("/api/interventions/{parent_id}/complete"),
        Some(&tech_cookie),
        Some(&json!({ "solution_report": "Rénovation générale complétée." })),
    );
    assert_eq!(
        r_close_parent_ok.status,
        200,
        "{}",
        r_close_parent_ok.text()
    );
}

#[test]
fn gestion_des_techniciens_et_saisie_des_temps() {
    let srv = demo("int-techs-time");
    let (admin_cookie, _) = admin(&srv);
    let (resp_cookie, resp_user) = login(&srv, "responsable", "demo1234");
    let site_id = resp_user["site_id"].as_str().unwrap();
    let (tech1_cookie, tech1_user) = login(&srv, "technicien", "demo1234");
    let tech1_id = tech1_user["id"].as_str().unwrap();

    let eq_id = create_equipment(&srv, &resp_cookie, "EQ-TECH-1", "Ventilateur d'extraction");

    // Création d'un 2ème technicien sur le même site
    let r_user2 = srv.session_call(
        "POST",
        "/api/users",
        Some(&admin_cookie),
        Some(&json!({
            "username": "tech2",
            "display_name": "Deuxième Technicien",
            "role": "technicien",
            "site_id": site_id,
            "password": "Password123!"
        })),
    );
    assert_eq!(r_user2.status, 201, "{}", r_user2.text());
    let tech2_id = r_user2.json()["id"].as_str().unwrap().to_string();

    // Création d'une intervention
    let r_int = srv.session_call(
        "POST",
        "/api/interventions",
        Some(&resp_cookie),
        Some(&json!({
            "title": "Équilibrage des pales",
            "type": "preventive",
            "priority": "medium",
            "equipment_id": eq_id
        })),
    );
    assert_eq!(r_int.status, 201);
    let int_id = r_int.json()["id"].as_str().unwrap().to_string();

    // 1. Assignation de tech1 et tech2
    let r_as1 = srv.session_call(
        "POST",
        &format!("/api/interventions/{int_id}/technicians"),
        Some(&resp_cookie),
        Some(&json!({ "user_id": tech1_id, "is_lead": true })),
    );
    assert_eq!(r_as1.status, 200);

    let r_as2 = srv.session_call(
        "POST",
        &format!("/api/interventions/{int_id}/technicians"),
        Some(&resp_cookie),
        Some(&json!({ "user_id": tech2_id, "is_lead": false })),
    );
    assert_eq!(r_as2.status, 200);

    // 2. Tech1 saisit son propre temps (40 minutes) -> OK
    let r_time1 = srv.session_call(
        "PUT",
        &format!("/api/interventions/{int_id}/technicians/{tech1_id}/time"),
        Some(&tech1_cookie),
        Some(&json!({ "duration_minutes": 40 })),
    );
    assert_eq!(r_time1.status, 200);

    // 3. Tech1 tente de saisir le temps de Tech2 -> 403 Forbidden (§7.3)
    let r_time2_forbidden = srv.session_call(
        "PUT",
        &format!("/api/interventions/{int_id}/technicians/{tech2_id}/time"),
        Some(&tech1_cookie),
        Some(&json!({ "duration_minutes": 25 })),
    );
    assert_eq!(
        r_time2_forbidden.status,
        403,
        "{}",
        r_time2_forbidden.text()
    );

    // 4. Le responsable saisit le temps de Tech2 (25 minutes) -> OK
    let r_time2_ok = srv.session_call(
        "PUT",
        &format!("/api/interventions/{int_id}/technicians/{tech2_id}/time"),
        Some(&resp_cookie),
        Some(&json!({ "duration_minutes": 25 })),
    );
    assert_eq!(r_time2_ok.status, 200);

    // 5. Vérification : la durée de l'intervention est la somme automatique (40 + 25 = 65)
    let r_check = srv.session_call(
        "GET",
        &format!("/api/interventions/{int_id}"),
        Some(&resp_cookie),
        None,
    );
    assert_eq!(r_check.json()["duration_minutes"], 65);

    // 6. Retrait de Tech2 -> la durée globale est recalculée (redevient 40)
    let r_del_tech2 = srv.session_call(
        "DELETE",
        &format!("/api/interventions/{int_id}/technicians/{tech2_id}"),
        Some(&resp_cookie),
        None,
    );
    assert_eq!(r_del_tech2.status, 204);

    let r_check2 = srv.session_call(
        "GET",
        &format!("/api/interventions/{int_id}"),
        Some(&resp_cookie),
        None,
    );
    assert_eq!(r_check2.json()["duration_minutes"], 40);
}

#[test]
fn annulation_et_recalcul_equipement() {
    let srv = demo("int-cancel");
    let (resp_cookie, _) = login(&srv, "responsable", "demo1234");
    let (tech_cookie, _) = login(&srv, "technicien", "demo1234");

    let eq_id = create_equipment(&srv, &resp_cookie, "EQ-CAN-1", "Centrifugeuse");

    // 1. Création avec ARRET -> équipement bascule out_of_service
    let r_int = srv.session_call(
        "POST",
        "/api/interventions",
        Some(&tech_cookie),
        Some(&json!({
            "title": "Bruit de roulement aigu",
            "type": "corrective",
            "priority": "critical",
            "machine_state": "ARRET",
            "equipment_id": eq_id
        })),
    );
    assert_eq!(r_int.status, 201);
    let int_id = r_int.json()["id"].as_str().unwrap().to_string();

    let r_eq1 = srv.session_call(
        "GET",
        &format!("/api/equipments/{eq_id}"),
        Some(&resp_cookie),
        None,
    );
    assert_eq!(r_eq1.json()["status"], "out_of_service");

    // 2. Tentative d'annulation sans motif -> 400 Bad Request (§7.1)
    let r_cancel_empty = srv.session_call(
        "POST",
        &format!("/api/interventions/{int_id}/cancel"),
        Some(&tech_cookie),
        Some(&json!({ "reason": "   " })),
    );
    assert_eq!(r_cancel_empty.status, 400);

    // 3. Annulation réussie avec motif
    let r_cancel = srv.session_call(
        "POST",
        &format!("/api/interventions/{int_id}/cancel"),
        Some(&tech_cookie),
        Some(&json!({ "reason": "Fausse alerte constatée après vérification du capteur de vibration." })),
    );
    assert_eq!(r_cancel.status, 200, "{}", r_cancel.text());
    let int_cancelled = r_cancel.json();
    assert_eq!(int_cancelled["status"], "cancelled");
    assert!(int_cancelled["cancelled_at"].is_string());
    assert!(int_cancelled["cancel_reason"].is_string());

    // 4. L'équipement repasse automatiquement à operational
    let r_eq2 = srv.session_call(
        "GET",
        &format!("/api/equipments/{eq_id}"),
        Some(&resp_cookie),
        None,
    );
    assert_eq!(r_eq2.json()["status"], "operational");
}

#[test]
fn isolation_inter_sites_et_permissions_rbac() {
    let srv = demo("int-rbac");
    let (admin_cookie, _) = admin(&srv);
    let (resp_cookie, _) = login(&srv, "responsable", "demo1234");
    let (prod_cookie, _) = login(&srv, "production", "demo1234");

    // 1. Création d'un second site B et d'un utilisateur sur le site B
    let r_site_b = srv.session_call(
        "POST",
        "/api/sites",
        Some(&admin_cookie),
        Some(&json!({ "name": "Site Usine B" })),
    );
    assert_eq!(r_site_b.status, 201, "{}", r_site_b.text());
    let site_b_id = r_site_b.json()["id"].as_str().unwrap().to_string();

    let r_user_b = srv.session_call(
        "POST",
        "/api/users",
        Some(&admin_cookie),
        Some(&json!({
            "username": "tech_site_b",
            "display_name": "Tech Site B",
            "role": "technicien",
            "site_id": site_b_id,
            "password": "Password123!"
        })),
    );
    assert_eq!(r_user_b.status, 201);
    let tech_b_id = r_user_b.json()["id"].as_str().unwrap().to_string();
    let (tech_b_cookie, _) = login(&srv, "tech_site_b", "Password123!");

    // Équipement et intervention sur site A
    let eq_a_id = create_equipment(&srv, &resp_cookie, "EQ-ISO-A", "Machine Site A");
    let r_int_a = srv.session_call(
        "POST",
        "/api/interventions",
        Some(&resp_cookie),
        Some(&json!({
            "title": "Intervention Confidentielle Site A",
            "type": "preventive",
            "priority": "low",
            "equipment_id": eq_a_id
        })),
    );
    assert_eq!(r_int_a.status, 201);
    let int_a_id = r_int_a.json()["id"].as_str().unwrap().to_string();

    // 2. Isolation inter-sites : tech du site B tente d'accéder à l'intervention du site A -> 404
    let r_get_cross = srv.session_call(
        "GET",
        &format!("/api/interventions/{int_a_id}"),
        Some(&tech_b_cookie),
        None,
    );
    assert_eq!(
        r_get_cross.status, 404,
        "Une ressource hors-site doit répondre 404"
    );

    // Tentative d'assigner un utilisateur du site B à une intervention du site A -> 400
    let r_assign_cross = srv.session_call(
        "POST",
        &format!("/api/interventions/{int_a_id}/technicians"),
        Some(&resp_cookie),
        Some(&json!({ "user_id": tech_b_id, "is_lead": false })),
    );
    assert_eq!(
        r_assign_cross.status, 400,
        "L'assignation d'un utilisateur hors-site doit répondre 400"
    );

    // 3. Permissions RBAC : opérateur de Production
    // Production peut créer une intervention (201)
    let r_prod_create = srv.session_call(
        "POST",
        "/api/interventions",
        Some(&prod_cookie),
        Some(&json!({
            "title": "Signalement opérateur incident ligne",
            "type": "corrective",
            "priority": "medium",
            "machine_state": "FONCTIONNEL",
            "equipment_id": eq_a_id
        })),
    );
    assert_eq!(
        r_prod_create.status, 201,
        "Production doit pouvoir créer une demande"
    );
    let int_prod_id = r_prod_create.json()["id"].as_str().unwrap().to_string();

    // Production ne peut pas prendre en charge (/take) -> 403 Forbidden
    let r_prod_take = srv.session_call(
        "POST",
        &format!("/api/interventions/{int_prod_id}/take"),
        Some(&prod_cookie),
        None,
    );
    assert_eq!(r_prod_take.status, 403);

    // Production ne peut pas modifier (PATCH) -> 403 Forbidden
    let r_prod_patch = srv.session_call(
        "PATCH",
        &format!("/api/interventions/{int_prod_id}"),
        Some(&prod_cookie),
        Some(&json!({ "title": "Modification non autorisée" })),
    );
    assert_eq!(r_prod_patch.status, 403);

    // Production ne peut pas supprimer (DELETE) -> 403 Forbidden
    let r_prod_del = srv.session_call(
        "DELETE",
        &format!("/api/interventions/{int_prod_id}"),
        Some(&prod_cookie),
        None,
    );
    assert_eq!(r_prod_del.status, 403);
}

#[test]
fn pagination_par_curseur_et_filtres() {
    let srv = demo("int-pagination");
    let (resp_cookie, _) = login(&srv, "responsable", "demo1234");
    let eq_id = create_equipment(&srv, &resp_cookie, "EQ-PAG-1", "Robot d'emballage");

    // Création de 5 interventions
    let mut created_ids = Vec::new();
    for i in 1..=5 {
        let r = srv.session_call(
            "POST",
            "/api/interventions",
            Some(&resp_cookie),
            Some(&json!({
                "title": format!("Intervention paginée {i}"),
                "type": "preventive",
                "priority": "medium",
                "equipment_id": eq_id
            })),
        );
        assert_eq!(r.status, 201);
        created_ids.push(r.json()["id"].as_str().unwrap().to_string());
    }

    // Parcours de 5 éléments avec limit=2 : Page 1 (2), Page 2 (2), Page 3 (1)
    let r_page1 = srv.session_call(
        "GET",
        "/api/interventions?limit=2",
        Some(&resp_cookie),
        None,
    );
    assert_eq!(r_page1.status, 200);
    let p1 = r_page1.json();
    let items1 = p1["items"].as_array().unwrap();
    assert_eq!(items1.len(), 2);
    let c1 = p1["next_cursor"].as_str().expect("curseur après page 1");

    let r_page2 = srv.session_call(
        "GET",
        &format!("/api/interventions?limit=2&cursor={c1}"),
        Some(&resp_cookie),
        None,
    );
    assert_eq!(r_page2.status, 200);
    let p2 = r_page2.json();
    let items2 = p2["items"].as_array().unwrap();
    assert_eq!(items2.len(), 2);
    let c2 = p2["next_cursor"].as_str().expect("curseur après page 2");

    let r_page3 = srv.session_call(
        "GET",
        &format!("/api/interventions?limit=2&cursor={c2}"),
        Some(&resp_cookie),
        None,
    );
    assert_eq!(r_page3.status, 200);
    let p3 = r_page3.json();
    let items3 = p3["items"].as_array().unwrap();
    assert_eq!(items3.len(), 1);
    assert!(
        p3["next_cursor"].is_null(),
        "Dernière page : next_cursor doit être null"
    );

    // Vérifie qu'aucun identifiant n'est dupliqué et que les 5 sont présents
    let mut all_fetched = Vec::new();
    for item in items1.iter().chain(items2.iter()).chain(items3.iter()) {
        all_fetched.push(item["id"].as_str().unwrap().to_string());
    }
    assert_eq!(all_fetched.len(), 5);
    for id in &created_ids {
        assert!(all_fetched.contains(id), "L'élément {id} doit être présent");
    }

    // Filtres combinés : status=open et equipment_id
    let r_filtered = srv.session_call(
        "GET",
        &format!("/api/interventions?status=open&equipment_id={eq_id}"),
        Some(&resp_cookie),
        None,
    );
    assert_eq!(r_filtered.status, 200);
    assert_eq!(r_filtered.json()["items"].as_array().unwrap().len(), 5);
}
