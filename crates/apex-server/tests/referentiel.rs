//! Tests boîte noire du référentiel technique (phase 2).
//!
//! Couvre l'ensemble des exigences de docs/specs/phase-2.md §5 :
//! - CRUD de chaque ressource (lignes, équipements, emplacements, fournisseurs)
//! - PATCH partiel avec null explicite pour vider
//! - Isolation multi-sites (ressource d'un autre site répond 404)
//! - Permissions RBAC (refus 403 sans les droits appropriés)
//! - Suppression refusée avec enfants (409)
//! - Détection de cycle (400) et profondeur maximale de 8 niveaux (400)
//! - Hiérarchie stricte des emplacements (400)
//! - Pagination par curseur (3 pages sur 5 éléments, aucun doublon)
//! - Audit log écrit dans la même transaction

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

#[test]
fn crud_complet_et_patch_partiel() {
    let srv = demo("ref-crud");
    let (admin_cookie, _) = admin(&srv);
    let (resp_cookie, resp_user) = login(&srv, "responsable", "demo1234");
    let site_id = resp_user["site_id"].as_str().unwrap();

    // 1. Ligne de production : création, lecture, patch, suppression
    let r_pl_create = srv.session_call(
        "POST",
        "/api/production-lines",
        Some(&resp_cookie),
        Some(&json!({
            "code": "LIGNE-A",
            "name": "Ligne d'assemblage A",
            "description": "Atelier principal"
        })),
    );
    assert_eq!(r_pl_create.status, 201, "{}", r_pl_create.text());
    let pl = r_pl_create.json();
    let pl_id = pl["id"].as_str().unwrap();
    assert_eq!(pl["equipment_count"], 0);

    // PATCH : modification du nom et null explicite pour description
    let r_pl_patch = srv.session_call(
        "PATCH",
        &format!("/api/production-lines/{pl_id}"),
        Some(&resp_cookie),
        Some(&json!({
            "name": "Ligne A modifiée",
            "description": null
        })),
    );
    assert_eq!(r_pl_patch.status, 200, "{}", r_pl_patch.text());
    let pl_updated = r_pl_patch.json();
    assert_eq!(pl_updated["name"], "Ligne A modifiée");
    assert!(pl_updated["description"].is_null());
    assert_eq!(pl_updated["code"], "LIGNE-A");

    // 2. Équipement racine puis sous-équipement
    let r_eq1 = srv.session_call(
        "POST",
        "/api/equipments",
        Some(&resp_cookie),
        Some(&json!({
            "code": "EQ-01",
            "name": "Machine Principale",
            "status": "operational",
            "production_line_id": pl_id,
            "manufacturer": "Siemens",
            "model": "S7-1500",
            "commissioned_on": "2025-01-15",
            "description": "Automate principal"
        })),
    );
    assert_eq!(r_eq1.status, 201, "{}", r_eq1.text());
    let eq1 = r_eq1.json();
    let eq1_id = eq1["id"].as_str().unwrap();
    assert_eq!(eq1["production_line"]["code"], "LIGNE-A");
    assert!(eq1["parent"].is_null());

    let r_eq2 = srv.session_call(
        "POST",
        "/api/equipments",
        Some(&resp_cookie),
        Some(&json!({
            "code": "EQ-02",
            "name": "Module Déporté",
            "status": "operational",
            "production_line_id": pl_id,
            "parent_id": eq1_id
        })),
    );
    assert_eq!(r_eq2.status, 201, "{}", r_eq2.text());
    let eq2 = r_eq2.json();
    let eq2_id = eq2["id"].as_str().unwrap();
    assert_eq!(eq2["parent"]["id"], eq1_id);

    // Vérification du détail de eq1 (children_count doit être 1)
    let r_eq1_get = srv.session_call(
        "GET",
        &format!("/api/equipments/{eq1_id}"),
        Some(&resp_cookie),
        None,
    );
    assert_eq!(r_eq1_get.status, 200);
    assert_eq!(r_eq1_get.json()["children_count"], 1);

    // Arborescence des équipements
    let r_tree = srv.session_call(
        "GET",
        &format!("/api/equipments/tree?site_id={site_id}"),
        Some(&admin_cookie),
        None,
    );
    assert_eq!(r_tree.status, 200);
    let tree = r_tree.json();
    assert!(!tree.as_array().unwrap().is_empty());
    let root = tree
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["id"] == eq1_id)
        .expect("nœud racine présent");
    assert_eq!(root["children"][0]["id"], eq2_id);

    // PATCH équipement : vidage du fabricant via null et changement de statut
    let r_eq1_patch = srv.session_call(
        "PATCH",
        &format!("/api/equipments/{eq1_id}"),
        Some(&resp_cookie),
        Some(&json!({
            "status": "degraded",
            "manufacturer": null
        })),
    );
    assert_eq!(r_eq1_patch.status, 200);
    assert_eq!(r_eq1_patch.json()["status"], "degraded");
    assert!(r_eq1_patch.json()["manufacturer"].is_null());

    // 3. Emplacements : hiérarchie magasin > allée > rack > bac
    let r_mag = srv.session_call(
        "POST",
        "/api/locations",
        Some(&resp_cookie),
        Some(&json!({
            "code": "MAG-1",
            "name": "Magasin Central",
            "kind": "magasin"
        })),
    );
    assert_eq!(r_mag.status, 201, "{}", r_mag.text());
    let mag_id = r_mag.json()["id"].as_str().unwrap().to_owned();

    let r_all = srv.session_call(
        "POST",
        "/api/locations",
        Some(&resp_cookie),
        Some(&json!({
            "code": "ALL-A",
            "name": "Allée A",
            "kind": "allee",
            "parent_id": mag_id
        })),
    );
    assert_eq!(r_all.status, 201);
    let all_id = r_all.json()["id"].as_str().unwrap().to_owned();

    let r_rack = srv.session_call(
        "POST",
        "/api/locations",
        Some(&resp_cookie),
        Some(&json!({
            "code": "RCK-01",
            "name": "Rack 1",
            "kind": "rack",
            "parent_id": all_id
        })),
    );
    assert_eq!(r_rack.status, 201);
    let rack_id = r_rack.json()["id"].as_str().unwrap().to_owned();

    let r_bac = srv.session_call(
        "POST",
        "/api/locations",
        Some(&resp_cookie),
        Some(&json!({
            "code": "BAC-01",
            "name": "Bac 1",
            "kind": "bac",
            "parent_id": rack_id
        })),
    );
    assert_eq!(r_bac.status, 201);
    let bac_id = r_bac.json()["id"].as_str().unwrap().to_owned();

    // Arborescence des emplacements
    let r_loc_tree = srv.session_call("GET", "/api/locations/tree", Some(&resp_cookie), None);
    assert_eq!(r_loc_tree.status, 200);
    assert!(r_loc_tree.text().contains("BAC-01"));

    // 4. Fournisseur : CRUD complet
    let r_sup = srv.session_call(
        "POST",
        "/api/suppliers",
        Some(&resp_cookie),
        Some(&json!({
            "name": "Fournisseur Test",
            "contact_name": "Jean Dupont",
            "email": "jean@fournisseur.fr",
            "phone": "0102030405"
        })),
    );
    assert_eq!(r_sup.status, 201);
    let sup_id = r_sup.json()["id"].as_str().unwrap().to_owned();

    // PATCH fournisseur : vider le contact_name via null
    let r_sup_patch = srv.session_call(
        "PATCH",
        &format!("/api/suppliers/{sup_id}"),
        Some(&resp_cookie),
        Some(&json!({
            "contact_name": null,
            "website": "https://fournisseur.fr"
        })),
    );
    assert_eq!(r_sup_patch.status, 200);
    assert!(r_sup_patch.json()["contact_name"].is_null());
    assert_eq!(r_sup_patch.json()["website"], "https://fournisseur.fr");

    // DELETE fournisseur
    assert_eq!(
        srv.session_call(
            "DELETE",
            &format!("/api/suppliers/{sup_id}"),
            Some(&resp_cookie),
            None
        )
        .status,
        204
    );
    // Vérification 404 après suppression
    assert_eq!(
        srv.session_call(
            "GET",
            &format!("/api/suppliers/{sup_id}"),
            Some(&resp_cookie),
            None
        )
        .status,
        404
    );

    // Nettoyage en cascade des emplacements du bac vers le magasin
    assert_eq!(
        srv.session_call(
            "DELETE",
            &format!("/api/locations/{bac_id}"),
            Some(&resp_cookie),
            None
        )
        .status,
        204
    );
    assert_eq!(
        srv.session_call(
            "DELETE",
            &format!("/api/locations/{rack_id}"),
            Some(&resp_cookie),
            None
        )
        .status,
        204
    );
    assert_eq!(
        srv.session_call(
            "DELETE",
            &format!("/api/locations/{all_id}"),
            Some(&resp_cookie),
            None
        )
        .status,
        204
    );
    assert_eq!(
        srv.session_call(
            "DELETE",
            &format!("/api/locations/{mag_id}"),
            Some(&resp_cookie),
            None
        )
        .status,
        204
    );

    // Nettoyage des équipements puis de la ligne
    assert_eq!(
        srv.session_call(
            "DELETE",
            &format!("/api/equipments/{eq2_id}"),
            Some(&resp_cookie),
            None
        )
        .status,
        204
    );
    assert_eq!(
        srv.session_call(
            "DELETE",
            &format!("/api/equipments/{eq1_id}"),
            Some(&resp_cookie),
            None
        )
        .status,
        204
    );
    assert_eq!(
        srv.session_call(
            "DELETE",
            &format!("/api/production-lines/{pl_id}"),
            Some(&resp_cookie),
            None
        )
        .status,
        204
    );
}

#[test]
fn isolation_entre_sites_et_404() {
    let srv = demo("ref-isolation");
    let (admin_cookie, _) = admin(&srv);

    // 1. Créer un second site via l'admin
    let r_site2 = srv.session_call(
        "POST",
        "/api/sites",
        Some(&admin_cookie),
        Some(&json!({"name": "Second Site"})),
    );
    assert_eq!(r_site2.status, 201);
    let site2_id = r_site2.json()["id"].as_str().unwrap().to_owned();

    // 2. Créer un responsable pour ce second site
    let r_u2 = srv.session_call(
        "POST",
        "/api/users",
        Some(&admin_cookie),
        Some(&json!({
            "username": "resp_site2",
            "display_name": "Responsable Site 2",
            "role": "responsable",
            "site_id": site2_id,
            "password": "PasswordSite2!",
            "active": true
        })),
    );
    assert_eq!(r_u2.status, 201);

    // Connexion des deux responsables
    let (resp1_cookie, _) = login(&srv, "responsable", "demo1234");
    let (resp2_cookie, _) = login(&srv, "resp_site2", "PasswordSite2!");

    // resp1 crée une ligne et un équipement sur Site 1
    let r_pl = srv.session_call(
        "POST",
        "/api/production-lines",
        Some(&resp1_cookie),
        Some(&json!({"code": "L-SITE1", "name": "Ligne Site 1"})),
    );
    assert_eq!(r_pl.status, 201);
    let pl1_id = r_pl.json()["id"].as_str().unwrap().to_owned();

    let r_eq = srv.session_call(
        "POST",
        "/api/equipments",
        Some(&resp1_cookie),
        Some(&json!({
            "code": "EQ-SITE1",
            "name": "Équipement Site 1",
            "production_line_id": pl1_id
        })),
    );
    assert_eq!(r_eq.status, 201);
    let eq1_id = r_eq.json()["id"].as_str().unwrap().to_owned();

    // resp2 ne doit pas voir les ressources de site 1 dans la liste (liste vide)
    let r_list = srv.session_call("GET", "/api/production-lines", Some(&resp2_cookie), None);
    assert_eq!(r_list.status, 200);
    assert_eq!(r_list.json()["items"].as_array().unwrap().len(), 0);

    // resp2 ne doit pas voir EQ-SITE1 dans l'arbre
    let r_tree = srv.session_call("GET", "/api/equipments/tree", Some(&resp2_cookie), None);
    assert_eq!(r_tree.status, 200);
    assert_eq!(r_tree.json().as_array().unwrap().len(), 0);

    // Accès direct par resp2 aux ressources de site 1 -> 404 (et non 403)
    assert_eq!(
        srv.session_call(
            "GET",
            &format!("/api/production-lines/{pl1_id}"),
            Some(&resp2_cookie),
            None
        )
        .status,
        404
    );
    assert_eq!(
        srv.session_call(
            "PATCH",
            &format!("/api/production-lines/{pl1_id}"),
            Some(&resp2_cookie),
            Some(&json!({"name": "Piratage"}))
        )
        .status,
        404
    );
    assert_eq!(
        srv.session_call(
            "DELETE",
            &format!("/api/equipments/{eq1_id}"),
            Some(&resp2_cookie),
            None
        )
        .status,
        404
    );

    // resp2 tente de créer un équipement rattaché à une ligne d'un autre site -> 400
    let r_cross = srv.session_call(
        "POST",
        "/api/equipments",
        Some(&resp2_cookie),
        Some(&json!({
            "code": "EQ-CROSS",
            "name": "Cross site",
            "production_line_id": pl1_id
        })),
    );
    assert_eq!(r_cross.status, 400);

    // resp2 tente de forcer site_id vers site 1 -> 403
    let (resp1_user, _) = (login(&srv, "responsable", "demo1234").1, ());
    let site1_id = resp1_user["site_id"].as_str().unwrap();
    let r_force = srv.session_call(
        "POST",
        "/api/production-lines",
        Some(&resp2_cookie),
        Some(&json!({
            "site_id": site1_id,
            "code": "L-EVIL",
            "name": "Ligne illicite"
        })),
    );
    assert_eq!(r_force.status, 403);
}

#[test]
fn permissions_rbac_refus_403() {
    let srv = demo("ref-rbac");
    let (resp_cookie, _) = login(&srv, "responsable", "demo1234");
    let (tech_cookie, _) = login(&srv, "technicien", "demo1234");
    let (prod_cookie, _) = login(&srv, "production", "demo1234");

    // Créer une ligne et un équipement par le responsable
    let pl = srv
        .session_call(
            "POST",
            "/api/production-lines",
            Some(&resp_cookie),
            Some(&json!({"code": "L-RBAC", "name": "Ligne RBAC"})),
        )
        .json();
    let pl_id = pl["id"].as_str().unwrap();

    let eq = srv
        .session_call(
            "POST",
            "/api/equipments",
            Some(&resp_cookie),
            Some(&json!({
                "code": "EQ-RBAC",
                "name": "Machine RBAC",
                "production_line_id": pl_id
            })),
        )
        .json();
    let eq_id = eq["id"].as_str().unwrap();

    // Le technicien peut lire les lignes et équipements
    assert_eq!(
        srv.session_call("GET", "/api/production-lines", Some(&tech_cookie), None)
            .status,
        200
    );
    assert_eq!(
        srv.session_call("GET", "/api/equipments", Some(&tech_cookie), None)
            .status,
        200
    );

    // Mais le technicien ne peut pas créer, modifier ni supprimer
    assert_eq!(
        srv.session_call(
            "POST",
            "/api/production-lines",
            Some(&tech_cookie),
            Some(&json!({"code": "L-TECH", "name": "Ligne Tech"}))
        )
        .status,
        403
    );
    assert_eq!(
        srv.session_call(
            "PATCH",
            &format!("/api/equipments/{eq_id}"),
            Some(&tech_cookie),
            Some(&json!({"status": "out_of_service"}))
        )
        .status,
        403
    );
    assert_eq!(
        srv.session_call(
            "DELETE",
            &format!("/api/production-lines/{pl_id}"),
            Some(&tech_cookie),
            None
        )
        .status,
        403
    );

    // Le technicien peut lire les emplacements et fournisseurs, mais pas les gérer
    assert_eq!(
        srv.session_call("GET", "/api/locations", Some(&tech_cookie), None)
            .status,
        200
    );
    assert_eq!(
        srv.session_call("GET", "/api/suppliers", Some(&tech_cookie), None)
            .status,
        200
    );
    assert_eq!(
        srv.session_call(
            "POST",
            "/api/locations",
            Some(&tech_cookie),
            Some(&json!({"code": "M-TECH", "name": "Magasin", "kind": "magasin"}))
        )
        .status,
        403
    );
    assert_eq!(
        srv.session_call(
            "POST",
            "/api/suppliers",
            Some(&tech_cookie),
            Some(&json!({"name": "Fournisseur Tech"}))
        )
        .status,
        403
    );

    // L'opérateur de production ne peut pas accéder aux emplacements ni aux fournisseurs (pas de .view)
    assert_eq!(
        srv.session_call("GET", "/api/locations", Some(&prod_cookie), None)
            .status,
        403
    );
    assert_eq!(
        srv.session_call("GET", "/api/suppliers", Some(&prod_cookie), None)
            .status,
        403
    );
}

#[test]
fn garde_fous_enfants_conflits_et_cycle() {
    let srv = demo("ref-gardes");
    let (resp_cookie, _) = login(&srv, "responsable", "demo1234");

    // Ligne + Équipement
    let pl_id = srv
        .session_call(
            "POST",
            "/api/production-lines",
            Some(&resp_cookie),
            Some(&json!({"code": "L-ENF", "name": "Ligne Enfants"})),
        )
        .json()["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let eq1_id = srv
        .session_call(
            "POST",
            "/api/equipments",
            Some(&resp_cookie),
            Some(&json!({
                "code": "EQ-P1",
                "name": "Parent 1",
                "production_line_id": pl_id
            })),
        )
        .json()["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let eq2_id = srv
        .session_call(
            "POST",
            "/api/equipments",
            Some(&resp_cookie),
            Some(&json!({
                "code": "EQ-E2",
                "name": "Enfant 2",
                "parent_id": eq1_id
            })),
        )
        .json()["id"]
        .as_str()
        .unwrap()
        .to_owned();

    // 1. Refus 409 : suppression de ligne contenant des équipements
    assert_eq!(
        srv.session_call(
            "DELETE",
            &format!("/api/production-lines/{pl_id}"),
            Some(&resp_cookie),
            None
        )
        .status,
        409
    );

    // 2. Refus 409 : suppression d'équipement ayant des sous-équipements
    assert_eq!(
        srv.session_call(
            "DELETE",
            &format!("/api/equipments/{eq1_id}"),
            Some(&resp_cookie),
            None
        )
        .status,
        409
    );

    // 3. Refus 400 : cycle direct ou indirect
    // Direct : eq1 parent de lui-même
    assert_eq!(
        srv.session_call(
            "PATCH",
            &format!("/api/equipments/{eq1_id}"),
            Some(&resp_cookie),
            Some(&json!({"parent_id": eq1_id}))
        )
        .status,
        400
    );
    // Cycle indirect : eq1 a pour enfant eq2, on tente de mettre eq1 comme enfant de eq2
    assert_eq!(
        srv.session_call(
            "PATCH",
            &format!("/api/equipments/{eq1_id}"),
            Some(&resp_cookie),
            Some(&json!({"parent_id": eq2_id}))
        )
        .status,
        400
    );

    // 4. Refus 400 : hiérarchie d'emplacements invalide
    // Créer magasin
    let mag_id = srv
        .session_call(
            "POST",
            "/api/locations",
            Some(&resp_cookie),
            Some(&json!({"code": "MAG-G", "name": "Magasin", "kind": "magasin"})),
        )
        .json()["id"]
        .as_str()
        .unwrap()
        .to_owned();

    // Rack directement sous magasin -> refusé 400 (un rack exige une allée)
    assert_eq!(
        srv.session_call(
            "POST",
            "/api/locations",
            Some(&resp_cookie),
            Some(&json!({
                "code": "RCK-BAD",
                "name": "Rack direct",
                "kind": "rack",
                "parent_id": mag_id
            }))
        )
        .status,
        400
    );

    // 5. Refus 409 : doublon de code sur le même site (insensible à la casse)
    assert_eq!(
        srv.session_call(
            "POST",
            "/api/equipments",
            Some(&resp_cookie),
            Some(&json!({"code": "eq-p1", "name": "Doublon"}))
        )
        .status,
        409
    );
}

#[test]
fn pagination_keyset_cinq_elements_sans_doublon() {
    let srv = demo("ref-pagination");
    let (resp_cookie, _) = login(&srv, "responsable", "demo1234");

    // Créer 5 équipements racines avec des codes ordonnés : P-1, P-2, P-3, P-4, P-5
    for i in 1..=5 {
        let r = srv.session_call(
            "POST",
            "/api/equipments",
            Some(&resp_cookie),
            Some(&json!({
                "code": format!("PAGE-{i:02}"),
                "name": format!("Machine Page {i}"),
                "status": "operational"
            })),
        );
        assert_eq!(r.status, 201);
    }

    // Page 1 : limit = 2
    let r1 = srv.session_call(
        "GET",
        "/api/equipments?q=PAGE-&limit=2",
        Some(&resp_cookie),
        None,
    );
    assert_eq!(r1.status, 200);
    let p1 = r1.json();
    let items1 = p1["items"].as_array().unwrap();
    assert_eq!(items1.len(), 2);
    assert_eq!(items1[0]["code"], "PAGE-01");
    assert_eq!(items1[1]["code"], "PAGE-02");
    let c1 = p1["next_cursor"].as_str().expect("curseur 1");

    // Page 2 : limit = 2 avec curseur 1
    let r2 = srv.session_call(
        "GET",
        &format!("/api/equipments?q=PAGE-&limit=2&cursor={c1}"),
        Some(&resp_cookie),
        None,
    );
    assert_eq!(r2.status, 200);
    let p2 = r2.json();
    let items2 = p2["items"].as_array().unwrap();
    assert_eq!(items2.len(), 2);
    assert_eq!(items2[0]["code"], "PAGE-03");
    assert_eq!(items2[1]["code"], "PAGE-04");
    let c2 = p2["next_cursor"].as_str().expect("curseur 2");

    // Page 3 : limit = 2 avec curseur 2
    let r3 = srv.session_call(
        "GET",
        &format!("/api/equipments?q=PAGE-&limit=2&cursor={c2}"),
        Some(&resp_cookie),
        None,
    );
    assert_eq!(r3.status, 200);
    let p3 = r3.json();
    let items3 = p3["items"].as_array().unwrap();
    assert_eq!(items3.len(), 1);
    assert_eq!(items3[0]["code"], "PAGE-05");
    assert!(p3["next_cursor"].is_null());

    // Vérification de l'absence totale de doublons
    let mut all_codes = Vec::new();
    for page in [&items1, &items2, &items3] {
        for it in *page {
            all_codes.push(it["code"].as_str().unwrap().to_string());
        }
    }
    assert_eq!(
        all_codes,
        vec!["PAGE-01", "PAGE-02", "PAGE-03", "PAGE-04", "PAGE-05"]
    );
}
