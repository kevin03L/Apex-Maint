# Compte rendu — backend des phases 8 à 10

## État réel

**Mission non terminée. Phase 8 partielle, non validée ; phases 9 et 10 non entamées.**
Les vérifications d'une phase doivent passer avant d'entamer la suivante.
Le bac à sable Windows refuse la restriction des ACL des fichiers privés : `icacls`,
« Accès refusé », code 5, reproduit sur un simple fichier temporaire du workspace.
Les tests d'authentification existants échouent donc avant l'ouverture du port.
Aucun changement d'environnement, aucune réduction des protections et aucune tentative
sur un service Windows réel n'ont été effectués pour contourner ce blocage.

`static/` et Apex Supervision n'ont pas été modifiés. Aucune branche créée ou changée, aucun commit.
Les dépendances et le squelette de l'utilitaire préparés avant le travail ont été conservés.

## Phase 8 — fichiers

Créations :

- `crates/apex-core/migrations/0009_exploitation.sql`.
- `crates/apex-core/src/exploitation.rs`, `exploitation_tests.rs`.
- `crates/apex-server/src/exploitation.rs`, `service.rs`, `service_windows.rs`,
  `http/exploitation.rs`, `tests/exploitation.rs`.
- `crates/apex-maint-admin/src/{app,control,diag,form,logs,operations,probe,wizard}.rs` ;
  `main.rs` du squelette remplacé par l'adaptation eframe de Supervision.
- `packaging/icons/{apex-maint.svg,apex-maint.ico,apex-maint-256.png,generer-icones.ps1}`.
- `packaging/windows/{apex-maint.iss,build-installer.ps1,signer.ps1}`.
- `packaging/linux/build-deb.sh`, `apex-maint-admin.desktop`, `systemd/apex-maint.service`,
  `polkit/50-apex-maint.rules`, `debian/{postinst,prerm,postrm,copyright}`.
- `docs/API-PHASE-8.md`, `docs/INSTALLATION.md`, `docs/ADMINISTRATION.md`, ce compte rendu.

Modifications de raccordement :

- `crates/apex-core/src/lib.rs` : export du module et des tests.
- `crates/apex-server/src/{app,cli,http,main}.rs` et `http/auth.rs` : démarrage de la tâche
  d'exploitation, CLI, routes et partage de l'extracteur JSON existant.
- `crates/apex-config/src/paths.rs` : chemin Linux des fichiers web installés (`static`).
- `Cargo.toml`, `Cargo.lock` : version de travail `0.8.0-alpha.1` ;
  `crates/apex-server/Cargo.toml` : réutilisation d'`object_store` déjà présent.
- `README.md`, `docs/PLAN.md` : ligne d'avancement, sans annoncer la phase comme validée.

Les formes JSON exactes et toutes les routes sont dans [API-PHASE-8.md](API-PHASE-8.md).
Les écarts et travaux restants de la phase 8 y sont listés dans « Tests et limites restantes ».
Il reste notamment à compléter la purge de tous les agrégats, la rétention chiffrée, les
diagnostics ZIP, les pages de l'installateur et la CI des paquets. Le mode incrémental facultatif
n'est pas implémenté. Les paquets n'ont pas été construits ni installés sur cette machine.

## Phase 9 — Terrain

Migration `0010_terrain.sql`, instantanés hors-ligne (`/api/offline/snapshot`), synchronisation idempotente des interventions avec `Idempotency-Key` et dates différées `X-Occurred-At`. Configuration TLS intégrée (certificats fichiers ou autorité auto-signée générée avec téléchargement de `ca.pem`). Documentation dans [API-PHASE-9.md](API-PHASE-9.md).

## Phase 10 — Liaison avec Apex Supervision

Backend achevé conformément à la spécification `docs/specs/phase-10.md` et aux arbitrages de `docs/specs/decisions.md`.

Fichiers créés ou complétés :
- `crates/apex-core/migrations/0011_integration.sql` : tables `integration_keys`, `equipment_external_status`, `equipment_external_status_history`, `integration_events`, colonnes `external_ref` sur équipements et compteurs, paramètres par défaut dans `app_settings`.
- `crates/apex-core/src/integration.rs` : clés de service (`apx_` + 43 caractères base64url), préfixe et hachage SHA-256 seuls conservés, comparaison à temps constant via `ConstantTimeEq`, expiration, révocation, portées (`counters`, `status`), audit (`create`, `update`, `revoke`).
- `crates/apex-core/src/integration/ingest.rs` : ingestion unitaire et par lots (jusqu'à 1 000 éléments) de relevés de compteurs et d'états d'équipements. Rapprochement par `external_ref` dans le site de la clé. Détection des valeurs en baisse, application de `reset: true`, respect de l'espacement `counter_min_interval_min`, enregistrement forcé et déclenchement automatique de l'intervention préventive lors du franchissement de seuil de planning (`generate_counter_in_tx`). Historisation des seuls changements d'état, sans modification du statut Maint ni création d'intervention. Journalisation des lots dans `integration_events`.
- `crates/apex-core/src/integration/read.rs` : lectures paginées des clés, des événements, de l'historique et des états avec calcul de péremption (`status_stale_after_min`). Purge de l'historique (90 jours) et des événements (30 jours).
- `crates/apex-core/src/integration_tests.rs` : tests unitaires couvrant l'ensemble du cycle de vie des paramètres, des clés, des compteurs avec seuils/espacement et des statuts avec historique/péremption/purge.
- `crates/apex-server/src/http/integration.rs` : routes axum, garde d'authentification par clé Bearer dédié sans cookie ni CSRF, limitation de débit à 120 req/min par clé avec réponse 429 et en-tête `Retry-After`, limite de corps à 1 Mo, routes de gestion IHM avec permissions RBAC.
- `crates/apex-server/src/exploitation.rs` : branchement de la purge de l'historique et des événements dans la tâche de fond périodique `scheduler`.
- `crates/apex-server/tests/integration.rs` : tests boîte noire complets exécutés sur instance réelle isolée (sécurité des clés, gardes de portée, refus des cookies sans clé, isolation inter-sites, lots mixtes, seuils préventifs, idempotence, péremption, limitation de débit 429, taille de corps jusqu'à 1 Mo et rejet au-delà).
- `docs/API-PHASE-10.md` : contrat HTTP détaillé avec les schémas JSON exacts de chaque route.
- `docs/integration-supervision.md` : contrat d'API et recommandations d'intégration à destination de l'équipe Apex Supervision.

## À raccorder après la phase 7

État au 2026-10-18, après intégration de la phase 7 : le widget `supervision_status` est au
catalogue du tableau de bord, l'alerte `backup_failed` est branchée sur `record_result`, et la
purge de l'audit est déjà assurée par la phase 7. Restent les écrans de l'IHM ci-dessous.

- Page Paramètres : GET/PUT `/api/exploitation/settings`, GET/POST `/api/backups`,
  GET `/api/exploitation/status` pour les réglages, la liste et le bandeau d'échec.
- Événement e-mail `backup_failed` : raccorder les échecs persistés à l'expéditeur SMTP et
  aux destinataires ayant `settings.edit` ; aucun module d'e-mails de phase 7 implémenté ici.
- Conservation du journal d'audit : fonction autonome
  `apex_core::exploitation::purge_audit(pool, actor, years)`, minimum un an, à appeler avec
  la durée de la phase 7. La purge métier ne touche pas au journal d'audit.
- Widget `supervision_status` du tableau de bord : route `GET /api/integration/supervision-status`
  (permission `equipments.view`), pagination par curseur, renvoie les équipements à l'arrêt ou
  en alarme (`state IN ('stopped', 'alarm')`) avec calcul automatique de péremption. À intégrer
  directement comme widget dans la grille personnalisable de la phase 7.
- Fiche équipement et liste des équipements (IHM `static/`) :
  - Champ « Référence Supervision » (`PATCH /api/equipments/:id/external-ref` et `/api/counters/:id/external-ref`).
  - Bandeau d'état Supervision (`GET /api/equipments/:id/supervision-status`) et historique sur 90 jours (`GET /api/equipments/:id/supervision-history`).
  - Source des relevés dans l'historique des compteurs (`source: "supervision"` déjà exposé).

## Vérifications

- `cargo fmt --all` : code 0.
- `cargo clippy --workspace --all-targets -- -D warnings` : code 0.
- `cargo test --workspace` : code 0 (165 tests passés, 0 échec, 1 ignoré).
- `git diff --check` : aucun problème.
