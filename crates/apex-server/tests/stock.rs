//! Tests boîte noire du catalogue de pièces et gestion du stock (Phase 4).
//!
//! Couvre l'ensemble des exigences de docs/specs/phase-4.md §6 et les décisions du 2026-10-18 (§7) :
//! 1. CRUD et catalogue de pièces (référence unique par site, patch partiel, blocage suppression si stock > 0)
//! 2. Entrée de stock (in) et valorisation au prix moyen pondéré (PMP en centimes)
//! 3. Transfert d'emplacement (transfer) avec vérification de disponibilité
//! 4. Sortie sur intervention (résolution du défaut I2, blocage sortie directe hors intervention, prix figé)
//! 5. Concurrence stricte (BEGIN IMMEDIATE) : aucun stock négatif, atomicité sous forte charge
//! 6. Ajustements d'inventaire et rebuts (§7.2, permission stock.adjust)
//! 7. Réservations de pièces préparées pour intervention active (§7.3, préparation, consommation partielle, libération à la clôture/annulation)
//! 8. Alertes dynamiques de stock bas (seuil spécifique ou global)
//! 9. Nomenclature et pièces rattachées aux équipements (equipment_parts)
//! 10. Isolation inter-sites (404) et permissions RBAC (403)

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

fn create_location_hierarchy(srv: &Server, cookie: &str, suffix: &str) -> (String, String) {
    let r_mag = srv.session_call(
        "POST",
        "/api/locations",
        Some(cookie),
        Some(&json!({
            "code": format!("MAG-{}", suffix),
            "name": format!("Magasin {}", suffix),
            "kind": "magasin"
        })),
    );
    assert_eq!(r_mag.status, 201, "création magasin : {}", r_mag.text());
    let mag_id = r_mag.json()["id"].as_str().unwrap().to_string();

    let r_all = srv.session_call(
        "POST",
        "/api/locations",
        Some(cookie),
        Some(&json!({
            "code": format!("ALL-{}", suffix),
            "name": format!("Allée {}", suffix),
            "kind": "allee",
            "parent_id": mag_id
        })),
    );
    assert_eq!(r_all.status, 201, "création allée : {}", r_all.text());
    let all_id = r_all.json()["id"].as_str().unwrap().to_string();

    let r_rack = srv.session_call(
        "POST",
        "/api/locations",
        Some(cookie),
        Some(&json!({
            "code": format!("RCK-{}", suffix),
            "name": format!("Rack {}", suffix),
            "kind": "rack",
            "parent_id": all_id
        })),
    );
    assert_eq!(r_rack.status, 201, "création rack : {}", r_rack.text());
    let rack_id = r_rack.json()["id"].as_str().unwrap().to_string();

    let r_bac_a = srv.session_call(
        "POST",
        "/api/locations",
        Some(cookie),
        Some(&json!({
            "code": format!("BAC-A-{}", suffix),
            "name": format!("Bac A {}", suffix),
            "kind": "bac",
            "parent_id": rack_id
        })),
    );
    assert_eq!(r_bac_a.status, 201, "création bac A : {}", r_bac_a.text());
    let bac_a_id = r_bac_a.json()["id"].as_str().unwrap().to_string();

    let r_bac_b = srv.session_call(
        "POST",
        "/api/locations",
        Some(cookie),
        Some(&json!({
            "code": format!("BAC-B-{}", suffix),
            "name": format!("Bac B {}", suffix),
            "kind": "bac",
            "parent_id": rack_id
        })),
    );
    assert_eq!(r_bac_b.status, 201, "création bac B : {}", r_bac_b.text());
    let bac_b_id = r_bac_b.json()["id"].as_str().unwrap().to_string();

    (bac_a_id, bac_b_id)
}

fn create_part(
    srv: &Server,
    cookie: &str,
    reference: &str,
    name: &str,
    price_cents: i64,
    threshold: Option<i64>,
) -> String {
    let mut body = json!({
        "reference": reference,
        "name": name,
        "category": "roulement",
        "unit": "piece",
        "unit_price_cents": price_cents
    });
    if let Some(t) = threshold {
        body["alert_threshold"] = json!(t);
    }
    let r = srv.session_call("POST", "/api/parts", Some(cookie), Some(&body));
    assert_eq!(r.status, 201, "création pièce {reference} : {}", r.text());
    r.json()["id"].as_str().unwrap().to_string()
}

// ============================================================================
// 1. CRUD ET CATALOGUE DE PIÈCES
// ============================================================================

#[test]
fn crud_catalogue_pieces_et_contraintes() {
    let srv = demo("stock-crud-parts");
    let (resp_cookie, _) = login(&srv, "responsable", "demo1234");

    // Création d'une pièce
    let part_id = create_part(
        &srv,
        &resp_cookie,
        "ROUL-6204-2RS",
        "Roulement à billes 6204",
        1250,
        Some(4),
    );

    // Refus de doublon sur le même site (409 Conflict)
    let r_dup = srv.session_call(
        "POST",
        "/api/parts",
        Some(&resp_cookie),
        Some(&json!({
            "reference": "ROUL-6204-2RS",
            "name": "Autre désignation",
            "unit": "piece",
            "unit_price_cents": 1000
        })),
    );
    assert_eq!(
        r_dup.status, 409,
        "Doublon de référence doit être refusé en 409"
    );

    // Consultation fiche
    let r_get = srv.session_call(
        "GET",
        &format!("/api/parts/{part_id}"),
        Some(&resp_cookie),
        None,
    );
    assert_eq!(r_get.status, 200);
    let p = r_get.json();
    assert_eq!(p["reference"], "ROUL-6204-2RS");
    assert_eq!(p["total_stock"], 0);
    assert_eq!(p["effective_threshold"], 4);
    assert_eq!(p["is_low_stock"], true); // 0 <= 4

    // Modification partielle PATCH
    let r_patch = srv.session_call(
        "PATCH",
        &format!("/api/parts/{part_id}"),
        Some(&resp_cookie),
        Some(&json!({
            "name": "Roulement à billes étanche 6204-2RS",
            "alert_threshold": 6
        })),
    );
    assert_eq!(r_patch.status, 200);
    assert_eq!(
        r_patch.json()["name"],
        "Roulement à billes étanche 6204-2RS"
    );
    assert_eq!(r_patch.json()["effective_threshold"], 6);

    // Suppression quand stock = 0 -> 204
    let r_del = srv.session_call(
        "DELETE",
        &format!("/api/parts/{part_id}"),
        Some(&resp_cookie),
        None,
    );
    assert_eq!(r_del.status, 204);

    // Vérification que la pièce supprimée n'est plus accessible (404)
    let r_get_after = srv.session_call(
        "GET",
        &format!("/api/parts/{part_id}"),
        Some(&resp_cookie),
        None,
    );
    assert_eq!(r_get_after.status, 404);
}

// ============================================================================
// 2. ENTRÉE DE STOCK ET CALCUL DU PMP
// ============================================================================

#[test]
fn entree_stock_et_calcul_du_pmp() {
    let srv = demo("stock-pmp-in");
    let (resp_cookie, _) = login(&srv, "responsable", "demo1234");
    let (tech_cookie, _) = login(&srv, "technicien", "demo1234");

    let (bac_a, _) = create_location_hierarchy(&srv, &resp_cookie, "PMP");
    let part_id = create_part(
        &srv,
        &resp_cookie,
        "JOINT-VITON-50",
        "Joint torique Viton 50mm",
        0,
        Some(2),
    );

    // 1ère entrée : 10 unités à 10,00 € (1000 centimes)
    let r_in1 = srv.session_call(
        "POST",
        "/api/stock/movements",
        Some(&tech_cookie),
        Some(&json!({
            "part_id": part_id,
            "movement_type": "in",
            "to_location_id": bac_a,
            "quantity": 10,
            "unit_price_cents": 1000,
            "reason": "Réception BL-100"
        })),
    );
    assert_eq!(r_in1.status, 201, "{}", r_in1.text());

    // Vérification pièce : stock = 10, PMP = 1000
    let p1 = srv
        .session_call(
            "GET",
            &format!("/api/parts/{part_id}"),
            Some(&tech_cookie),
            None,
        )
        .json();
    assert_eq!(p1["total_stock"], 10);
    assert_eq!(p1["unit_price_cents"], 1000);

    // 2ème entrée : 5 unités à 13,00 € (1300 centimes)
    // Calcul PMP : (10 * 1000 + 5 * 1300) / (10 + 5) = 16500 / 15 = 1100 centimes
    let r_in2 = srv.session_call(
        "POST",
        "/api/stock/movements",
        Some(&tech_cookie),
        Some(&json!({
            "part_id": part_id,
            "movement_type": "in",
            "to_location_id": bac_a,
            "quantity": 5,
            "unit_price_cents": 1300,
            "reason": "Réception BL-101"
        })),
    );
    assert_eq!(r_in2.status, 201, "{}", r_in2.text());

    // Vérification : stock = 15, PMP = 1100
    let p2 = srv
        .session_call(
            "GET",
            &format!("/api/parts/{part_id}"),
            Some(&tech_cookie),
            None,
        )
        .json();
    assert_eq!(p2["total_stock"], 15);
    assert_eq!(p2["unit_price_cents"], 1100);

    // Tentative de suppression refusée car stock > 0 (409 Conflict)
    let r_del_fail = srv.session_call(
        "DELETE",
        &format!("/api/parts/{part_id}"),
        Some(&resp_cookie),
        None,
    );
    assert_eq!(r_del_fail.status, 409, "Suppression refusée si stock > 0");
}

// ============================================================================
// 3. TRANSFERT D'EMPLACEMENT
// ============================================================================

#[test]
fn transfert_emplacement_et_garde_fous() {
    let srv = demo("stock-transfer");
    let (resp_cookie, _) = login(&srv, "responsable", "demo1234");
    let (tech_cookie, _) = login(&srv, "technicien", "demo1234");

    let (bac_a, bac_b) = create_location_hierarchy(&srv, &resp_cookie, "TRF");
    let part_id = create_part(
        &srv,
        &resp_cookie,
        "COURROIE-SPZ",
        "Courroie SPZ-987",
        0,
        Some(1),
    );

    // Entrée de 15 unités dans bac A à 2000 centimes
    srv.session_call(
        "POST",
        "/api/stock/movements",
        Some(&tech_cookie),
        Some(&json!({
            "part_id": part_id,
            "movement_type": "in",
            "to_location_id": bac_a,
            "quantity": 15,
            "unit_price_cents": 2000,
            "reason": "Lot initial"
        })),
    );

    // Transfert de 4 unités du bac A vers bac B
    let r_trf = srv.session_call(
        "POST",
        "/api/stock/movements",
        Some(&tech_cookie),
        Some(&json!({
            "part_id": part_id,
            "movement_type": "transfer",
            "from_location_id": bac_a,
            "to_location_id": bac_b,
            "quantity": 4,
            "reason": "Réapprovisionnement atelier"
        })),
    );
    assert_eq!(r_trf.status, 201, "{}", r_trf.text());

    // Vérification des stocks : A = 11, B = 4
    let r_stock = srv
        .session_call(
            "GET",
            &format!("/api/stock?part_id={part_id}"),
            Some(&tech_cookie),
            None,
        )
        .json();
    let stock_list = r_stock.as_array().unwrap();
    let stock_a = stock_list
        .iter()
        .find(|s| s["location_id"] == bac_a)
        .unwrap();
    let stock_b = stock_list
        .iter()
        .find(|s| s["location_id"] == bac_b)
        .unwrap();
    assert_eq!(stock_a["quantity"], 11);
    assert_eq!(stock_b["quantity"], 4);

    // Tentative de transférer 20 unités depuis bac A -> 409 Conflict
    let r_trf_excess = srv.session_call(
        "POST",
        "/api/stock/movements",
        Some(&tech_cookie),
        Some(&json!({
            "part_id": part_id,
            "movement_type": "transfer",
            "from_location_id": bac_a,
            "to_location_id": bac_b,
            "quantity": 20
        })),
    );
    assert_eq!(
        r_trf_excess.status, 409,
        "Transfert excédant le disponible doit échouer en 409"
    );
}

// ============================================================================
// 4. SORTIE SUR INTERVENTION (RÉSOLUTION DÉFAUT I2) ET ANNULATION
// ============================================================================

#[test]
fn sorties_sur_intervention_tracabilite_et_annulation() {
    let srv = demo("stock-out-int");
    let (resp_cookie, _) = login(&srv, "responsable", "demo1234");
    let (tech_cookie, tech_user) = login(&srv, "technicien", "demo1234");
    let tech_id = tech_user["id"].as_str().unwrap();

    let (bac_a, _) = create_location_hierarchy(&srv, &resp_cookie, "OUT");
    let part_id = create_part(
        &srv,
        &resp_cookie,
        "FUS-10A",
        "Fusible 10A Temporisé",
        0,
        Some(2),
    );
    let eq_id = create_equipment(&srv, &resp_cookie, "EQ-OUT-1", "Machine de test fusible");

    // Entrée de 10 unités à 500 centimes
    srv.session_call(
        "POST",
        "/api/stock/movements",
        Some(&tech_cookie),
        Some(&json!({
            "part_id": part_id,
            "movement_type": "in",
            "to_location_id": bac_a,
            "quantity": 10,
            "unit_price_cents": 500,
            "reason": "Stock fusibles"
        })),
    );

    // 1. Refus strict d'une sortie directe hors intervention
    let r_direct_out = srv.session_call(
        "POST",
        "/api/stock/movements",
        Some(&tech_cookie),
        Some(&json!({
            "part_id": part_id,
            "movement_type": "out",
            "from_location_id": bac_a,
            "quantity": 2
        })),
    );
    assert_eq!(
        r_direct_out.status, 400,
        "Une sortie directe sans intervention doit être refusée en 400"
    );

    // 2. Création d'une intervention
    let r_int = srv.session_call(
        "POST",
        "/api/interventions",
        Some(&resp_cookie),
        Some(&json!({
            "title": "Remplacement fusible HS",
            "type": "corrective",
            "priority": "high",
            "equipment_id": eq_id
        })),
    );
    assert_eq!(r_int.status, 201);
    let int_id = r_int.json()["id"].as_str().unwrap().to_string();

    // 3. Consommation de 3 fusibles sur l'intervention
    let r_cons = srv.session_call(
        "POST",
        &format!("/api/interventions/{int_id}/parts"),
        Some(&tech_cookie),
        Some(&json!({
            "part_id": part_id,
            "location_id": bac_a,
            "quantity": 3
        })),
    );
    assert_eq!(r_cons.status, 201, "{}", r_cons.text());
    let cons_item = r_cons.json();
    let cons_id = cons_item["id"].as_str().unwrap().to_string();
    assert_eq!(cons_item["quantity"], 3);
    assert_eq!(cons_item["unit_price_cents"], 500);

    // Vérification stock : 10 - 3 = 7
    let p_after = srv
        .session_call(
            "GET",
            &format!("/api/parts/{part_id}"),
            Some(&tech_cookie),
            None,
        )
        .json();
    assert_eq!(p_after["total_stock"], 7);

    // 4. Liste des pièces consommées sur l'intervention
    let r_list = srv.session_call(
        "GET",
        &format!("/api/interventions/{int_id}/parts"),
        Some(&tech_cookie),
        None,
    );
    assert_eq!(r_list.status, 200);
    assert_eq!(r_list.json().as_array().unwrap().len(), 1);

    // 5. Annulation de la consommation (DELETE)
    let r_cancel_cons = srv.session_call(
        "DELETE",
        &format!("/api/interventions/{int_id}/parts/{cons_id}"),
        Some(&tech_cookie),
        None,
    );
    assert_eq!(
        r_cancel_cons.status, 204,
        "Annulation de consommation réussie"
    );

    // Vérification stock réintégré : 7 + 3 = 10
    let p_reverted = srv
        .session_call(
            "GET",
            &format!("/api/parts/{part_id}"),
            Some(&tech_cookie),
            None,
        )
        .json();
    assert_eq!(p_reverted["total_stock"], 10);

    // 6. Nouvelle consommation de 2 fusibles puis clôture de l'intervention
    srv.session_call(
        "POST",
        &format!("/api/interventions/{int_id}/parts"),
        Some(&tech_cookie),
        Some(&json!({
            "part_id": part_id,
            "location_id": bac_a,
            "quantity": 2
        })),
    );

    // Assignation et saisie du temps pour permettre la clôture
    srv.session_call(
        "POST",
        &format!("/api/interventions/{int_id}/technicians"),
        Some(&resp_cookie),
        Some(&json!({ "user_id": tech_id, "is_lead": true })),
    );
    srv.session_call(
        "PUT",
        &format!("/api/interventions/{int_id}/technicians/{tech_id}/time"),
        Some(&tech_cookie),
        Some(&json!({ "duration_minutes": 45 })),
    );

    // Clôture
    let r_close = srv.session_call(
        "POST",
        &format!("/api/interventions/{int_id}/complete"),
        Some(&tech_cookie),
        Some(&json!({
            "solution_report": "Fusibles remplacés avec succès",
            "final_equipment_status": "operational"
        })),
    );
    assert_eq!(r_close.status, 200);

    // 7. Tentative de consommation sur l'intervention terminée -> 409 Conflict
    let r_cons_closed = srv.session_call(
        "POST",
        &format!("/api/interventions/{int_id}/parts"),
        Some(&tech_cookie),
        Some(&json!({
            "part_id": part_id,
            "location_id": bac_a,
            "quantity": 1
        })),
    );
    assert_eq!(
        r_cons_closed.status, 409,
        "Consommation sur intervention clôturée doit échouer en 409"
    );
}

// ============================================================================
// 5. TEST DE CONCURRENCE DÉDIÉ : AUCUN STOCK NÉGATIF
// ============================================================================

#[test]
fn concurrence_strict_aucun_stock_negatif() {
    let srv = demo("stock-concurrency");
    let (resp_cookie, _) = login(&srv, "responsable", "demo1234");
    let (tech_cookie, _) = login(&srv, "technicien", "demo1234");

    let (bac_a, _) = create_location_hierarchy(&srv, &resp_cookie, "CNC");
    let part_id = create_part(
        &srv,
        &resp_cookie,
        "VANN-DN25",
        "Vanne électromécanique DN25",
        0,
        Some(1),
    );
    let eq_id = create_equipment(&srv, &resp_cookie, "EQ-CNC-1", "Banc hydraulique");

    // 1. Stock initial exactement égal à 5 unités
    srv.session_call(
        "POST",
        "/api/stock/movements",
        Some(&tech_cookie),
        Some(&json!({
            "part_id": part_id,
            "movement_type": "in",
            "to_location_id": bac_a,
            "quantity": 5,
            "unit_price_cents": 8000,
            "reason": "Stock de test concurrence"
        })),
    );

    // 2. Création de 10 interventions distinctes
    let mut intervention_ids = Vec::new();
    for i in 1..=10 {
        let r = srv.session_call(
            "POST",
            "/api/interventions",
            Some(&resp_cookie),
            Some(&json!({
                "title": format!("Intervention concurrente {}", i),
                "type": "corrective",
                "priority": "high",
                "equipment_id": eq_id
            })),
        );
        assert_eq!(r.status, 201);
        intervention_ids.push(r.json()["id"].as_str().unwrap().to_string());
    }

    // 3. Lancement de 10 requêtes concurrentes via std::thread::scope
    // Chaque thread tente de consommer 2 unités sur son intervention respective.
    let port = srv.port();
    let barrier = std::sync::Barrier::new(10);
    let cookie = tech_cookie.clone();
    let p_id = part_id.clone();
    let loc_id = bac_a.clone();

    let statuses = std::thread::scope(|scope| {
        let handles: Vec<_> = intervention_ids
            .into_iter()
            .map(|int_id| {
                let barrier = &barrier;
                let cookie = &cookie;
                let p_id = &p_id;
                let loc_id = &loc_id;
                scope.spawn(move || {
                    barrier.wait();
                    let payload = json!({
                        "part_id": p_id,
                        "location_id": loc_id,
                        "quantity": 2
                    });
                    common::request_with_headers(
                        port,
                        "POST",
                        &format!("/api/interventions/{int_id}/parts"),
                        None,
                        Some(&payload),
                        &[("Cookie", cookie)],
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

    let success_count = statuses.iter().filter(|&&s| s == 201).count();
    let conflict_count = statuses.iter().filter(|&&s| s == 409).count();

    // Avec 5 unités disponibles et des demandes de 2 unités chacune,
    // exactement 2 requêtes doivent réussir (2 * 2 = 4 unités consommées)
    // et les 8 autres doivent recevoir 409 Conflict.
    assert_eq!(success_count, 2, "Exactement 2 requêtes doivent réussir");
    assert_eq!(
        conflict_count, 8,
        "Les 8 autres requêtes doivent être en conflit 409"
    );

    // 4. Vérification finale absolue : le stock restant est exactement égal à 1
    let final_part = srv
        .session_call(
            "GET",
            &format!("/api/parts/{part_id}"),
            Some(&tech_cookie),
            None,
        )
        .json();
    assert_eq!(
        final_part["total_stock"], 1,
        "Le stock résiduel doit être exactement 1 (5 - 4)"
    );

    // Vérification du nombre de mouvements out : exactement 2 mouvements de 2 unités
    let r_movs = srv.session_call(
        "GET",
        &format!("/api/stock/movements?part_id={part_id}&movement_type=out"),
        Some(&tech_cookie),
        None,
    );
    assert_eq!(r_movs.status, 200);
    let mov_items = r_movs.json()["items"].as_array().unwrap().clone();
    assert_eq!(
        mov_items.len(),
        2,
        "Exactement 2 mouvements de sortie en base"
    );
    for m in mov_items {
        assert_eq!(m["quantity"], 2);
    }
}

// ============================================================================
// 6. AJUSTEMENTS D'INVENTAIRE ET REBUTS (§7.2)
// ============================================================================

#[test]
fn ajustements_inventaire_et_rebuts() {
    let srv = demo("stock-adjustments");
    let (resp_cookie, _) = login(&srv, "responsable", "demo1234");
    let (tech_cookie, _) = login(&srv, "technicien", "demo1234");

    let (bac_a, _) = create_location_hierarchy(&srv, &resp_cookie, "ADJ");
    let part_id = create_part(
        &srv,
        &resp_cookie,
        "DISJ-20A",
        "Disjoncteur modulaire 20A",
        0,
        Some(1),
    );

    // Stock initial : 10 unités à 3000 centimes
    srv.session_call(
        "POST",
        "/api/stock/movements",
        Some(&resp_cookie),
        Some(&json!({
            "part_id": part_id,
            "movement_type": "in",
            "to_location_id": bac_a,
            "quantity": 10,
            "unit_price_cents": 3000,
            "reason": "Stock de base"
        })),
    );

    // Technicien n'a pas la permission stock.adjust -> 403 Forbidden
    let r_tech_adj = srv.session_call(
        "POST",
        "/api/stock/adjustments",
        Some(&tech_cookie),
        Some(&json!({
            "part_id": part_id,
            "location_id": bac_a,
            "delta": -2,
            "adjustment_kind": "casse",
            "reason": "Boîtier fissuré"
        })),
    );
    assert_eq!(
        r_tech_adj.status, 403,
        "Technicien sans stock.adjust doit recevoir 403"
    );

    // Responsable : ajustement négatif (casse de 2 unités)
    let r_adj_neg = srv.session_call(
        "POST",
        "/api/stock/adjustments",
        Some(&resp_cookie),
        Some(&json!({
            "part_id": part_id,
            "location_id": bac_a,
            "delta": -2,
            "adjustment_kind": "casse",
            "reason": "Boîtier fissuré lors du déballage"
        })),
    );
    assert_eq!(r_adj_neg.status, 201, "{}", r_adj_neg.text());

    // Vérification stock : 10 - 2 = 8
    let p1 = srv
        .session_call(
            "GET",
            &format!("/api/parts/{part_id}"),
            Some(&resp_cookie),
            None,
        )
        .json();
    assert_eq!(p1["total_stock"], 8);

    // Responsable : ajustement positif (retrouvé lors de l'inventaire : +3 unités)
    let r_adj_pos = srv.session_call(
        "POST",
        "/api/stock/adjustments",
        Some(&resp_cookie),
        Some(&json!({
            "part_id": part_id,
            "location_id": bac_a,
            "delta": 3,
            "adjustment_kind": "inventaire",
            "reason": "Écart inventaire tournant"
        })),
    );
    assert_eq!(r_adj_pos.status, 201, "{}", r_adj_pos.text());

    // Vérification stock : 8 + 3 = 11
    let p2 = srv
        .session_call(
            "GET",
            &format!("/api/parts/{part_id}"),
            Some(&resp_cookie),
            None,
        )
        .json();
    assert_eq!(p2["total_stock"], 11);
}

// ============================================================================
// 7. RÉSERVATIONS DE PIÈCES PRÉPARÉES (§7.3)
// ============================================================================

#[test]
fn reservations_pieces_preparees_et_liberation() {
    let srv = demo("stock-reservations");
    let (resp_cookie, _) = login(&srv, "responsable", "demo1234");
    let (tech_cookie, _) = login(&srv, "technicien", "demo1234");

    let (bac_a, _) = create_location_hierarchy(&srv, &resp_cookie, "RES");
    let part_id = create_part(
        &srv,
        &resp_cookie,
        "SERV-MTR-01",
        "Servomoteur brushless",
        0,
        Some(2),
    );
    let eq_id = create_equipment(&srv, &resp_cookie, "EQ-RES-1", "Axe numérique");

    // Stock de 10 unités à 25000 centimes
    srv.session_call(
        "POST",
        "/api/stock/movements",
        Some(&resp_cookie),
        Some(&json!({
            "part_id": part_id,
            "movement_type": "in",
            "to_location_id": bac_a,
            "quantity": 10,
            "unit_price_cents": 25000,
            "reason": "Stock servomoteurs"
        })),
    );

    // Création d'une intervention
    let r_int = srv.session_call(
        "POST",
        "/api/interventions",
        Some(&resp_cookie),
        Some(&json!({
            "title": "Préparation changement servomoteur",
            "type": "preventive",
            "priority": "medium",
            "equipment_id": eq_id
        })),
    );
    let int_id = r_int.json()["id"].as_str().unwrap().to_string();

    // 1. Préparation de réservation de 4 unités
    let r_res = srv.session_call(
        "POST",
        &format!("/api/interventions/{int_id}/reservations"),
        Some(&tech_cookie),
        Some(&json!({
            "part_id": part_id,
            "location_id": bac_a,
            "quantity": 4
        })),
    );
    assert_eq!(r_res.status, 201, "{}", r_res.text());
    let res_item = r_res.json();
    let res_id = res_item["id"].as_str().unwrap().to_string();
    assert_eq!(res_item["status"], "prepared");

    // Vérification disponibilité : Stock total = 10, Réservé = 4, Disponible = 6
    let p_check = srv
        .session_call(
            "GET",
            &format!("/api/parts/{part_id}"),
            Some(&tech_cookie),
            None,
        )
        .json();
    assert_eq!(p_check["total_stock"], 10);
    assert_eq!(p_check["total_reserved"], 4);
    assert_eq!(p_check["total_available"], 6);

    // Tentative de réserver 7 unités alors que disponible = 6 -> 409 Conflict
    let r_res_excess = srv.session_call(
        "POST",
        &format!("/api/interventions/{int_id}/reservations"),
        Some(&tech_cookie),
        Some(&json!({
            "part_id": part_id,
            "location_id": bac_a,
            "quantity": 7
        })),
    );
    assert_eq!(
        r_res_excess.status, 409,
        "Réservation supérieure au disponible doit échouer"
    );

    // 2. Consommation partielle : on consomme 2 unités sur les 4 préparées
    let r_cons_part = srv.session_call(
        "POST",
        &format!("/api/reservations/{res_id}/consume"),
        Some(&tech_cookie),
        Some(&json!({ "quantity": 2 })),
    );
    assert_eq!(r_cons_part.status, 200, "{}", r_cons_part.text());
    assert_eq!(r_cons_part.json()["quantity"], 2);

    // Après consommation partielle :
    // - 2 unités sorties physiquement (stock physique passe de 10 à 8)
    // - 2 unités restantes libérées en rayon (réservé passe à 0, disponible = 8)
    let p_after_part = srv
        .session_call(
            "GET",
            &format!("/api/parts/{part_id}"),
            Some(&tech_cookie),
            None,
        )
        .json();
    assert_eq!(p_after_part["total_stock"], 8);
    assert_eq!(p_after_part["total_reserved"], 0);
    assert_eq!(p_after_part["total_available"], 8);

    // 3. Deuxième réservation puis annulation de l'intervention -> pièces remises en rayon (released)
    let r_int2 = srv.session_call(
        "POST",
        "/api/interventions",
        Some(&resp_cookie),
        Some(&json!({
            "title": "Intervention à annuler",
            "type": "corrective",
            "priority": "low",
            "equipment_id": eq_id
        })),
    );
    let int2_id = r_int2.json()["id"].as_str().unwrap().to_string();

    let r_res2 = srv.session_call(
        "POST",
        &format!("/api/interventions/{int2_id}/reservations"),
        Some(&tech_cookie),
        Some(&json!({
            "part_id": part_id,
            "location_id": bac_a,
            "quantity": 3
        })),
    );
    assert_eq!(r_res2.status, 201);
    let res2_id = r_res2.json()["id"].as_str().unwrap().to_string();

    // Annulation de l'intervention int2
    let r_cancel_int = srv.session_call(
        "POST",
        &format!("/api/interventions/{int2_id}/cancel"),
        Some(&tech_cookie),
        Some(&json!({ "reason": "Fausse alerte atelier" })),
    );
    assert_eq!(r_cancel_int.status, 200);

    // Vérification que la réservation est automatiquement passée à 'released'
    let r_res_list = srv.session_call(
        "GET",
        &format!("/api/interventions/{int2_id}/reservations"),
        Some(&tech_cookie),
        None,
    );
    assert_eq!(r_res_list.status, 200);
    let res2_item = r_res_list
        .json()
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == res2_id)
        .unwrap()
        .clone();
    assert_eq!(
        res2_item["status"], "released",
        "La réservation doit être libérée à l'annulation"
    );
}

// ============================================================================
// 8. ALERTES DE STOCK BAS
// ============================================================================

#[test]
fn alertes_de_stock_bas_dynamiques() {
    let srv = demo("stock-alerts");
    let (resp_cookie, _) = login(&srv, "responsable", "demo1234");
    let (tech_cookie, _) = login(&srv, "technicien", "demo1234");

    let (bac_a, _) = create_location_hierarchy(&srv, &resp_cookie, "ALT");

    // Pièce A avec seuil spécifique à 4, stock initial à 3 -> en alerte
    let part_a = create_part(
        &srv,
        &resp_cookie,
        "ALT-PART-A",
        "Capteur proximité A",
        0,
        Some(4),
    );
    srv.session_call(
        "POST",
        "/api/stock/movements",
        Some(&tech_cookie),
        Some(&json!({
            "part_id": part_a,
            "movement_type": "in",
            "to_location_id": bac_a,
            "quantity": 3,
            "unit_price_cents": 1500,
            "reason": "Stock 3"
        })),
    );

    // Pièce B sans seuil spécifique (utilise le global app_settings = 5), stock initial = 6 -> pas d'alerte
    let part_b = create_part(
        &srv,
        &resp_cookie,
        "ALT-PART-B",
        "Capteur proximité B",
        0,
        None,
    );
    srv.session_call(
        "POST",
        "/api/stock/movements",
        Some(&tech_cookie),
        Some(&json!({
            "part_id": part_b,
            "movement_type": "in",
            "to_location_id": bac_a,
            "quantity": 6,
            "unit_price_cents": 1500,
            "reason": "Stock 6"
        })),
    );

    // Consultation des alertes : Part A doit être présente, Part B absente
    let r_alerts1 = srv.session_call("GET", "/api/stock/alerts", Some(&tech_cookie), None);
    assert_eq!(r_alerts1.status, 200);
    let alerts1 = r_alerts1.json().as_array().unwrap().clone();
    assert!(alerts1.iter().any(|p| p["id"] == part_a));
    assert!(!alerts1.iter().any(|p| p["id"] == part_b));

    // Ajustement négatif de 2 unités sur Part B (stock passe de 6 à 4 <= seuil global 5)
    srv.session_call(
        "POST",
        "/api/stock/adjustments",
        Some(&resp_cookie),
        Some(&json!({
            "part_id": part_b,
            "location_id": bac_a,
            "delta": -2,
            "adjustment_kind": "rebut",
            "reason": "Rebut capteurs défectueux"
        })),
    );

    // Part B doit maintenant figurer dans les alertes
    let r_alerts2 = srv.session_call("GET", "/api/stock/alerts", Some(&tech_cookie), None);
    let alerts2 = r_alerts2.json().as_array().unwrap().clone();
    assert!(alerts2.iter().any(|p| p["id"] == part_b));
}

// ============================================================================
// 9. PIÈCES ASSOCIÉES AUX ÉQUIPEMENTS (NOMENCLATURE)
// ============================================================================

#[test]
fn nomenclature_pieces_equipements() {
    let srv = demo("stock-eq-parts");
    let (resp_cookie, _) = login(&srv, "responsable", "demo1234");
    let (tech_cookie, _) = login(&srv, "technicien", "demo1234");

    let eq_id = create_equipment(&srv, &resp_cookie, "EQ-NOM-1", "Machine de fraisage");
    let part_id = create_part(
        &srv,
        &resp_cookie,
        "FRAISE-T10",
        "Fraise carbure T10",
        4500,
        Some(2),
    );

    // 1. Rattachement de la pièce à l'équipement
    let r_assoc = srv.session_call(
        "POST",
        &format!("/api/equipments/{eq_id}/parts"),
        Some(&resp_cookie),
        Some(&json!({
            "part_id": part_id,
            "quantity_recommended": 3,
            "notes": "Outil monté sur la broche principale"
        })),
    );
    assert_eq!(r_assoc.status, 201, "{}", r_assoc.text());
    assert_eq!(r_assoc.json()["quantity_recommended"], 3);

    // 2. Consultation de la nomenclature
    let r_list = srv.session_call(
        "GET",
        &format!("/api/equipments/{eq_id}/parts"),
        Some(&tech_cookie),
        None,
    );
    assert_eq!(r_list.status, 200);
    let list = r_list.json().as_array().unwrap().clone();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["part_reference"], "FRAISE-T10");

    // 3. Dissociation
    let r_del = srv.session_call(
        "DELETE",
        &format!("/api/equipments/{eq_id}/parts/{part_id}"),
        Some(&resp_cookie),
        None,
    );
    assert_eq!(r_del.status, 204);

    // Vérification liste vide
    let r_after = srv.session_call(
        "GET",
        &format!("/api/equipments/{eq_id}/parts"),
        Some(&tech_cookie),
        None,
    );
    assert_eq!(r_after.json().as_array().unwrap().len(), 0);
}

// ============================================================================
// 10. ISOLATION INTER-SITES ET PERMISSIONS RBAC
// ============================================================================

#[test]
fn isolation_inter_sites_et_permissions_rbac() {
    let srv = demo("stock-rbac-iso");
    let (admin_cookie, _) = admin(&srv);
    let (resp_cookie, _) = login(&srv, "responsable", "demo1234");
    let (prod_cookie, _) = login(&srv, "production", "demo1234");

    // 1. Création d'un second site B
    let r_site_b = srv.session_call(
        "POST",
        "/api/sites",
        Some(&admin_cookie),
        Some(&json!({ "name": "Site Secondaire B" })),
    );
    assert_eq!(r_site_b.status, 201);
    let site_b_id = r_site_b.json()["id"].as_str().unwrap().to_string();

    // Utilisateur technicien sur site B
    srv.session_call(
        "POST",
        "/api/users",
        Some(&admin_cookie),
        Some(&json!({
            "username": "tech_b",
            "display_name": "Tech B",
            "role": "technicien",
            "site_id": site_b_id,
            "password": "Password123!"
        })),
    );
    let (tech_b_cookie, _) = login(&srv, "tech_b", "Password123!");

    // Pièce sur site A
    let part_a = create_part(
        &srv,
        &resp_cookie,
        "PART-SITE-A",
        "Pièce Site A",
        1000,
        Some(1),
    );

    // Technicien du site B tente de consulter la pièce du site A -> 404 Not Found
    let r_cross_part = srv.session_call(
        "GET",
        &format!("/api/parts/{part_a}"),
        Some(&tech_b_cookie),
        None,
    );
    assert_eq!(
        r_cross_part.status, 404,
        "Une pièce d'un autre site doit répondre 404"
    );

    // 2. Utilisateur Production (sans stock.movements ni parts.create)
    let (bac_a, _) = create_location_hierarchy(&srv, &resp_cookie, "RBC");

    // Production tente de créer une pièce -> 403 Forbidden
    let r_prod_create_part = srv.session_call(
        "POST",
        "/api/parts",
        Some(&prod_cookie),
        Some(&json!({
            "reference": "PROD-ILLEGAL",
            "name": "Pièce interdite",
            "unit": "piece"
        })),
    );
    assert_eq!(
        r_prod_create_part.status, 403,
        "Production ne peut pas créer de pièce"
    );

    // Production tente d'enregistrer une entrée de stock -> 403 Forbidden
    let r_prod_in = srv.session_call(
        "POST",
        "/api/stock/movements",
        Some(&prod_cookie),
        Some(&json!({
            "part_id": part_a,
            "movement_type": "in",
            "to_location_id": bac_a,
            "quantity": 5,
            "unit_price_cents": 1000
        })),
    );
    assert_eq!(
        r_prod_in.status, 403,
        "Production ne peut pas faire de mouvement de stock"
    );

    // 3. Suppression d'un emplacement contenant du stock -> 409 Conflict
    srv.session_call(
        "POST",
        "/api/stock/movements",
        Some(&resp_cookie),
        Some(&json!({
            "part_id": part_a,
            "movement_type": "in",
            "to_location_id": bac_a,
            "quantity": 5,
            "unit_price_cents": 1000,
            "reason": "Stock pour test suppression emplacement"
        })),
    );

    let r_del_loc = srv.session_call(
        "DELETE",
        &format!("/api/locations/{bac_a}"),
        Some(&resp_cookie),
        None,
    );
    assert_eq!(
        r_del_loc.status, 409,
        "Suppression d'un emplacement contenant du stock doit échouer en 409"
    );
}
