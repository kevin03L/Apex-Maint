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

## Phases 9 et 10

Aucune migration 0010/0011, route, configuration TLS ou implémentation d'intégration ajoutée.
Les documents API de ces phases et le contrat Supervision ne sont pas créés : leur contenu
doit décrire une implémentation vérifiée, pas des routes encore absentes.

## À raccorder après la phase 7

- Page Paramètres : GET/PUT `/api/exploitation/settings`, GET/POST `/api/backups`,
  GET `/api/exploitation/status` pour les réglages, la liste et le bandeau d'échec.
- Événement e-mail `backup_failed` : raccorder les échecs persistés à l'expéditeur SMTP et
  aux destinataires ayant `settings.edit` ; aucun module d'e-mails de phase 7 implémenté ici.
- Conservation du journal d'audit : fonction autonome
  `apex_core::exploitation::purge_audit(pool, actor, years)`, minimum un an, à appeler avec
  la durée de la phase 7. La purge métier ne touche pas au journal d'audit.
- Widget `supervision_status` : aucun raccordement préparé, puisque la phase 10 n'est pas entamée.

## Vérifications

- `cargo fmt --all` : code 0.
- `cargo clippy --workspace --all-targets -- -D warnings` : code 0.
- `cargo test --workspace` : code 101. Avant l'échec : 4 tests apex-build, 24 apex-config,
  43 apex-core (1 ignoré), 17 utilitaire et 5 serveur réussis, soit 93 réussis et 1 ignoré.
  Puis 7 tests de `crates/apex-server/tests/auth.rs` échouent au démarrage des instances
  temporaires, restriction des permissions de `secret.key` refusée. Les suites suivantes
  ne sont pas exécutées par cette commande. Journal local : `target/phase-8-tests.log`.
- `cargo test --offline -p apex-core exploitation_tests -- --nocapture` : code 0,
  2 nouveaux tests réussis (également inclus dans les 43 tests core ci-dessus).
- `git diff --check` : aucun problème après nettoyage de la documentation.

Ces résultats ne valent pas validation de la phase 8 ; le blocage Windows et les limites
fonctionnelles doivent être traités avant d'entamer la phase 9.
