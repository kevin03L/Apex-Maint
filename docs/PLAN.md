# Apex Maint — Plan d'architecture et feuille de route

> Réécriture en Rust de l'ancienne GMAO (`D:\Vs-code\App-GMAO`, Node/Express + React + Supabase).
> **Apex Maint est le deuxième produit de la gamme Apex.** Il reprend la logique, l'architecture,
> l'outillage et les conventions d'**Apex Supervision** (`D:\Vs-code\Apex-Supervision`, `0.4.0-alpha.1`).
> Choix validés : **SQLite embarquée** et **IHM web statique** (HTML/CSS/JS sans framework ni build),
> comme Apex Supervision. Dépôt privé `kevin03L/Apex-Maint` (GPL-3.0).
> Statut : plan uniquement, aucun code écrit pour l'instant.

---

## 1. Ce qu'on reprend

### 1.1 De l'ancienne GMAO : la spécification fonctionnelle

L'ancien projet sert de **spécification fonctionnelle**. On ne reprend pas son code.

| Domaine | Contenu à porter |
|---|---|
| Sites (multi-tenant) | Chaque donnée métier porte un `site_id`. L'admin voit tous les sites, les autres rôles seulement le leur. |
| Utilisateurs & rôles | Rôles `admin`, `responsable`, `production`, `technicien`. Matrice `role_permissions` modifiable par l'admin. |
| Lignes de production | CRUD, rattachement des équipements. |
| Équipements | Arborescence parent/enfant, statut (`operational`, `degraded`, `out_of_service`), pièces associées, documents, QR code. |
| Interventions | Types `corrective`, `preventive`, `a_posteriori`, `hors_equipement`. Priorités `low` à `critical`. État machine `ARRET`/`DEGRADE`/`FONCTIONNEL`. Transitions : `open → in_progress → completed`, et `open → completed`. Sous-interventions, pièces consommées. |
| Maintenance préventive | Plannings par fréquence (`daily`, `weekly`, `monthly`, `quarterly`, `yearly`, `counter`, ou `interval_days`), occurrences réalisées, calcul de la prochaine échéance, vue planning. |
| Emplacements | Hiérarchie `magasin > allee > rack > bac`. |
| Pièces & stock | Catalogue de pièces, stock par emplacement, mouvements `in`/`out`/`transfer`. Une sortie exige une intervention. Seuil d'alerte de stock bas. |
| Fournisseurs | CRUD. |
| Documents | Pièces jointes sur équipements et interventions. 50 Mo max, liste blanche de types MIME. |
| Audit | Journal de toutes les modifications (avant/après, auteur, site). |
| Tableau de bord & reporting | Indicateurs, export CSV. |
| Paramètres | `app_name`, `app_subtitle`, `stock_alert_threshold`, `session_timeout_min`, paramètres de sauvegarde et d'alerte. |

Clés de permission existantes :
`dashboard.view`, `reporting.view`, `settings.view`, `settings.edit`, `audit.view`,
`equipments.{view,create,edit,delete}`, `interventions.{view,create,edit,delete}`,
`parts.{view,create,edit,delete}`, `production_lines.{view,create,edit,delete}`,
`documents.{view,create,delete}`, `locations.{view,manage}`, `maintenance.{view,manage}`,
`stock.{view,movements}`, `suppliers.{view,manage}`, `users.{view,manage}`.

### 1.2 D'Apex Supervision : l'architecture, l'outillage et les conventions

| Élément | Repris tel quel ou adapté |
|---|---|
| Workspace `crates/apex-*` | Même découpage : `apex-config`, `apex-core`, `apex-server`, `apex-admin`, `apex-build` (§2). |
| Un seul processus | Moteur métier + serveur Axum dans le binaire `apex-maint`. Séparation par crates, pas par processus. |
| Configuration | Fichier TOML versionné (`config_version`), validation, migrations, écriture atomique sous verrou, sauvegardes horodatées, **un seul rédacteur** (CLI ou utilitaire), aucun secret dans le fichier. |
| Couche OS | Chemins système Windows/Linux et mode portable dans `apex-config/src/paths.rs` uniquement. |
| Cycle de vie | Service Windows (SCM) ou unité systemd, mode manuel/portable, instance unique (`File::try_lock`), arrêt propre sur Ctrl+C/SIGTERM/SCM, échecs détectés **avant** l'ouverture du port. |
| Codes de sortie | Mêmes codes partagés avec l'utilitaire : 0 OK, 1 erreur, 2 usage, 3 configuration, 4 instance déjà lancée, 5 port indisponible, 6 base inaccessible. |
| Jeton de contrôle local | `run/control.token`, régénéré à chaque démarrage, accepté seulement en boucle locale directe, réservé à l'état et à l'arrêt propre. |
| Utilitaire desktop | `apex-maint-admin` (egui/eframe) : assistant initial, configuration, démarrage/arrêt du service, journaux, diagnostics. Indépendant du moteur. |
| Base | SQLite via `sqlx` (WAL), migrations versionnées, report du WAL à l'arrêt propre. |
| IHM web | `static/` : HTML/CSS/JS sans framework ni build, servi à la même origine que l'API. Même thème (bleu nuit, verre dépoli, variables CSS `--bg`, `--glass`, `--accent`…), plus un **thème clair** pour les ateliers éclairés (bascule dans l'en-tête, suit le système par défaut), même gabarit (en-tête, menu latéral repliable, barre d'état), icônes SVG en ligne, pas d'émojis. |
| Sécurité | Argon2id, verrouillage 60 s après 5 échecs, même message pour compte inconnu et mot de passe faux, sessions persistantes et révocables (empreinte SHA-256 en base, jamais le jeton), en-têtes de sécurité, `Cache-Control: no-store`, écoute sur `127.0.0.1` par défaut, proxy HTTPS (Caddy, nginx, IIS) pour l'accès réseau. **Le serveur reste la référence des droits : l'IHM masque, le serveur refuse.** |
| Échappement IHM | Toute donnée insérée dans le HTML passe par `esc()`, tout argument `onclick` par `jsArg()`. |
| Erreurs | `thiserror` dans les crates ; l'erreur métier donne son code HTTP (`http_status()`), appliqué par le serveur. |
| Journaux | `tracing` + fichiers avec rotation (`tracing-appender`), journal de démarrage séparé. |
| Packaging | Installateur Windows (Inno Setup), paquet DEB (`cargo-deb`, unité systemd durcie, polkit), icône générée par script, `VERSIONINFO` Windows via `apex-build`. |
| Version | Unique dans `[workspace.package]`, SemVer à suffixe de phase : `0.1.0-alpha.N → 0.x.0-beta.N → 1.0.0`. |
| CI | GitHub Actions : `fmt`, `clippy -D warnings` et tests sous Windows et Ubuntu 22.04 ; installateur et DEB (avec SHA-256) sur tag `v*` ; signature SignPath possible plus tard (exige un dépôt public). |
| Tests | Unitaires par crate + tests d'intégration « boîte noire » qui lancent le binaire compilé dans un dossier temporaire, en mode portable, sur un port libre. |
| `AGENTS.md` | Mêmes règles communes aux agents (langue, style, vérifications, interdictions). |
| Documentation | `README.md`, `docs/INSTALLATION.md`, `docs/ADMINISTRATION.md`, `docs/DEVELOPPEMENT.md`, en français. |

### 1.3 Problèmes de l'ancienne GMAO à ne pas reproduire

Ils viennent de `docs/historical/AUDIT_COMPLET_V3.md` de l'ancien projet :

- **C1, session exposée côté frontend** : session portée par un cookie `HttpOnly`, `SameSite=Strict`, illisible en JavaScript (voir l'écart avec Supervision en §6).
- **C2 et C4, RLS trop permissive, contrôle d'accès faible sur les documents** : autorisation centralisée en Rust, couverte par des tests d'isolation entre sites.
- **C3, schéma qui diverge du code** : un seul jeu de migrations `sqlx`, requêtes vérifiées à la compilation (`sqlx::query!`, cache hors-ligne `.sqlx/` versionné).
- **C5, suppressions destructives** : suppression logique (`deleted_at`) partout, sans `DELETE` physique sur les données métier.
- **I1, intervention possible sans équipement** : `equipment_id` obligatoire sauf pour `hors_equipement`, vérifié par une contrainte `CHECK`.
- **I2, traçabilité intervention-stock incomplète** : consommation de pièces et mouvement de stock écrits dans la même transaction.
- **I5, pagination par offset** : pagination par curseur (keyset) sur les grandes listes.
- **I7, pas de tests** : tests dès le premier module.

### 1.4 Ce qu'on abandonne

- Toute la pile Supabase (Kong, GoTrue, PostgREST, Storage, schéma `auth.*`) et PostgreSQL en exploitation.
- Les tables résiduelles `oban_jobs`, `oban_peers` et `schema_file_migrations`.
- Les deux applications mobiles (Capacitor et React Native). L'IHM sera responsive et installable comme PWA (phase 9).
- Les scripts `.mjs` d'exploitation, remplacés par des sous-commandes du binaire.
- Docker comme mode de déploiement principal : on installe un service, comme Supervision.

---

## 2. Architecture cible

```
┌──────────────────────────── Poste / serveur ─────────────────────────────┐
│                                                                          │
│  apex-maint (service Windows / unité systemd)    apex-maint-admin (GUI)  │
│  ┌──────────────────────────────────────────┐    ┌─────────────────────┐ │
│  │ apex-server : CLI, service, journaux,    │    │ egui : config,      │ │
│  │               instance unique, Axum,     │◄───┤ état, journaux,     │ │
│  │               API JSON, fichiers, IHM    │HTTP│ diagnostics,        │ │
│  │ apex-core   : GMAO (domaine + dépôts     │127.│ assistant initial   │ │
│  │               SQLite + services)         │0.0.1└──────┬──────────────┘ │
│  └───────────────┬────────────┬─────────────┘           │                │
│                  │            ▼                         ▼                │
│                  │     data/ : apex-maint.db (SQLite WAL), files/        │
│                  ▼ lit au démarrage                                      │
│        ┌──────────────────────────────────────────────────────────┐      │
│        │ apex-config : TOML versionné, validation, migrations,    │      │
│        │ sauvegardes, chemins système (Windows/Linux), codes      │      │
│        └──────────────────────────────────────────────────────────┘      │
│  Navigateur (PC, tablette) ──HTTP(S) via proxy──► IHM statique + /api    │
└──────────────────────────────────────────────────────────────────────────┘
```

Pour déployer : **un installateur** (Windows ou DEB) qui pose le service, l'utilitaire et l'IHM.
Les données (base, fichiers, journaux, configuration) sont hors du dossier du programme.

### 2.1 Workspace Cargo

```
Apex-Maint/
├── Cargo.toml                 # workspace, version unique du produit
├── AGENTS.md
├── crates/
│   ├── apex-config/           # modèle TOML v1, validation, migrations, paths.rs, exit.rs, version
│   ├── apex-core/             # moteur GMAO, sans Axum
│   │   ├── src/domain/        # types et règles pures, sans I/O : equipment, intervention,
│   │   │                      #   maintenance, stock, permission, actor, pagination…
│   │   ├── src/store/         # dépôts SQLite (sqlx), un par agrégat, filtre site_id obligatoire
│   │   ├── src/services/      # transactions métier (stock, occurrences, audit), FileStore
│   │   └── migrations/        # migrations SQL versionnées
│   ├── apex-server/           # binaire apex-maint : CLI, app.rs, http.rs, service.rs, instance.rs,
│   │   └── tests/             #   logging.rs, legacy.rs (import de l'ancienne GMAO) ; boîte noire
│   ├── apex-admin/            # binaire apex-maint-admin (egui), adapté de Supervision
│   └── apex-build/            # build.rs : icône et VERSIONINFO Windows (copié de Supervision)
├── static/                    # IHM : index.html, styles.css, app.js, pages/*.js
├── packaging/                 # windows/ (Inno Setup), linux/ (debian, systemd, polkit), icons/
├── .github/workflows/ci.yml
└── docs/
```

Règles de dépendance, comme Supervision :
- `apex-config` ne dépend ni de Tokio ni de SQLite (partagée par le moteur et l'utilitaire).
- `apex-core` ne dépend pas d'Axum. Le module `domain` ne fait aucune I/O et se teste sans base :
  calcul d'échéance, transitions de statut, règles de mouvement de stock, vérification des permissions.
- `apex-admin` ne dépend pas du moteur : il fonctionne même si le moteur ne démarre pas.
- Aucun chemin ni commande propre à un OS hors de `paths.rs`, `service.rs` et `apex-admin/src/control.rs`.

### 2.2 Choix techniques

Mêmes versions que Supervision pour faciliter le partage de code ; montées de version faites ensemble.

| Besoin | Crate | Remarque |
|---|---|---|
| HTTP | `axum` 0.7, `tower-http` 0.5 | comme Supervision ; limites de taille par route |
| Base | `sqlx` 0.8 (`sqlite`, `macros`, `chrono`, `migrate`) | WAL, `foreign_keys=ON`, `busy_timeout`, transactions `BEGIN IMMEDIATE` pour le stock |
| Auth | `argon2`, `sha2`, `rand` | sessions maison comme Supervision (pas de `tower-sessions`) |
| Sérialisation, config | `serde`, `serde_json`, `toml` | |
| Erreurs | `thiserror` dans les crates, `anyhow` au besoin dans les binaires | |
| Logs | `tracing`, `tracing-subscriber`, `tracing-appender` | |
| Dates | `chrono` | comme Supervision |
| Identifiants | `uuid` v7 stocké en `TEXT` | triables par date, utiles pour la pagination par curseur ; conserve les UUID de l'ancienne base |
| CLI | analyse manuelle comme Supervision (`cli.rs`) | pas de `clap`, pour rester homogène |
| Service | `windows-service` 0.7 ; systemd côté paquet | |
| Utilitaire | `eframe` 0.37 (wgpu, AccessKit) | |
| Détection MIME | `infer` | on ne fait pas confiance au type déclaré |
| QR codes | `qrcode` (rendu SVG) | étiquettes des équipements |
| Import de l'ancienne base | `sqlx` feature `postgres` + `bcrypt`, derrière une feature Cargo `legacy` | n'alourdit pas le binaire par défaut si on le souhaite |
| Tests | `cargo test`, tests boîte noire sur `std::net` | Playwright éventuel pour l'IHM en phase 8 |

### 2.3 Correspondance Supabase / Postgres → Apex Maint

| Ancien | Remplacement |
|---|---|
| GoTrue (auth) | table `users` (`password_hash` Argon2id), table `sessions`, premier compte `admin` créé au premier démarrage (comme Supervision) avec avertissement tant que le mot de passe d'usine est conservé |
| PostgREST | API JSON Axum `/api/...` |
| RLS (`same_tenant`, `is_admin`, `has_role_permission`) | type `Actor { user_id, site_id, role, permissions }` obligatoire dans chaque fonction de dépôt. Le filtre `site_id` est appliqué dans le dépôt, pas dans le handler. |
| Storage | `data_dir/files/{site_id}/{uuid}` + table `documents`, téléchargement en streaming après contrôle d'accès |
| Enums Postgres | `TEXT` + contrainte `CHECK`, liés à des enums Rust via `sqlx::Type` |
| `process_stock_movement()` (PL/pgSQL) | service Rust, transaction `BEGIN IMMEDIATE` (un seul rédacteur SQLite à la fois), règles dans `domain::stock` |
| `process_maintenance_occurrence()` | service Rust, calcul de l'échéance dans `domain::maintenance` |
| Triggers d'audit (`SET LOCAL apex.user_id`) | SQLite n'a pas de variable de session : l'audit (avant/après, auteur, site) est écrit par le service Rust **dans la même transaction** que la modification |
| Triggers `updated_at` | conservés en SQL (triggers SQLite) ou posés par les dépôts |

---

## 3. Modèle de données

On reprend les 18 tables utiles de l'ancien schéma, nettoyées, et on ajoute `sessions` :

`sites`, `users`, `role_permissions`, `sessions`, `app_settings`, `production_lines`, `equipments`,
`equipment_parts`, `interventions`, `intervention_parts`, `maintenance_schedules`,
`maintenance_occurrences`, `locations`, `parts`, `stock`, `stock_movements`, `suppliers`,
`documents`, `audit_log`.

Conventions :

- Clé primaire `id TEXT` (UUID), `created_at`, `updated_at`, `deleted_at` sur les tables métier.
- `site_id TEXT NOT NULL REFERENCES sites` sur toutes les données métier.
- Index composites qui commencent par `site_id` sur les listes filtrées.
- Contraintes `CHECK` : valeurs d'enum, `quantity >= 0` sur `stock`, `equipment_id` obligatoire hors `hors_equipement`, `intervention_id` obligatoire pour un mouvement `out`.
- Tables `STRICT` lorsque c'est possible.

### Configuration (format v1)

| Section | Paramètres |
|---|---|
| `config_version` | `1` |
| `[instance]` | `name` |
| `[server]` | `bind` (défaut `127.0.0.1`), `port` (défaut **`3100`**, pour cohabiter avec Supervision sur `3000`) |
| `[paths]` | `data_dir`, `log_dir`, `web_dir`, `files_dir` (vides = défauts système) |
| `[database]` | `file` (défaut `apex-maint.db`) |
| `[security]` | `session_ttl_hours`, `secure_cookie` (vrai derrière un proxy HTTPS) |
| `[files]` | `max_size_mb` (défaut 50) |
| `[logging]` | `level`, `file_enabled`, `retention_files` |
| `[demo]` | `enabled` : jeu de données et comptes de démonstration (équivalent de la simulation de Supervision) |

Les paramètres métier (`app_name`, seuil de stock, durée de session côté IHM…) restent en base, dans `app_settings`, et se gèrent depuis l'IHM.

Arborescence installée : `%ProgramData%\Apex Maint\{config,data,logs,run}` sous Windows,
`/etc/apex-maint`, `/var/lib/apex-maint`, `/var/log/apex-maint`, `/run/apex-maint` sous Linux, `./…` en mode portable.

### Reprise des données existantes

Sous-commande `apex-maint import-legacy --from <url-postgres-ancienne-base> [--files <dossier storage>]` :

1. Copie table par table en conservant les UUID, donc les liens restent valides. Enums et dates convertis vers le format SQLite.
2. Utilisateurs : les hachages bcrypt de `auth.users.encrypted_password` sont importés tels quels. À la première connexion, le mot de passe est vérifié en bcrypt puis re-haché en Argon2id. Personne n'a à réinitialiser son mot de passe.
3. Fichiers : copie du volume `supabase/volumes/storage` vers `data/files`.
4. Rapport de contrôle : nombre de lignes par table, documents orphelins.
5. Refus si la base cible n'est pas vierge, sauf `--force` avec sauvegarde préalable (comme `migrate-legacy` de Supervision).

Les dumps de `backups/gmao/*.sql.gz` serviront de jeu d'essai.

---

## 4. Feuille de route

Chaque phase se termine par une version qui fonctionne de bout en bout et qui est testée.
Les phases 0 et 1 reprennent largement le code de Supervision (`apex-config`, `apex-build`, cycle de vie, sécurité, gabarit de l'IHM) : on copie et on adapte, on ne réinvente pas.

| Phase | Contenu | Résultat attendu |
|---|---|---|
| **0. Socle** | Workspace, `AGENTS.md`, `apex-config` et `apex-build` repris de Supervision, CLI (`run`, `config check/show/init/migrate`, `version`), instance unique, codes de sortie, journaux, base SQLite + migrations initiales, `GET /api/health`, IHM statique minimale (gabarit et thème de Supervision), CI Windows + Ubuntu | `apex-maint config init --portable` puis `apex-maint run` affiche l'IHM ; CI verte |
| **1. Auth & sites** | `users`, `sites`, sessions (cookie `HttpOnly`), verrouillage, matrice de permissions, compte `admin` initial, page de connexion, menu filtré selon les permissions, profil et changement de mot de passe | tests boîte noire : connexion, verrouillage concurrent, révocation, isolation entre sites |
| **2. Référentiel** | Lignes de production, équipements (arborescence, statut), emplacements, fournisseurs | CRUD complets, suppression logique, audit actif (backend fait) |
| **3. Interventions** | CRUD, transitions de statut, sous-interventions, état machine, liste filtrable, page de détail | parcours technicien complet (backend fait) |
| **4. Pièces & stock** | Catalogue, stock par emplacement, mouvements transactionnels, pièces consommées sur intervention, alertes de stock bas | aucun stock négatif, même en concurrence (fait) |
| **5. Maintenance préventive** | Plannings, occurrences, calcul d'échéance, vue planning ou calendrier, génération d'interventions préventives | les échéances se mettent à jour à chaque réalisation (fait) |
| **6. Documents & QR** | Upload en streaming, contrôle MIME et taille, téléchargement contrôlé, QR code par équipement | fichiers servis seulement aux utilisateurs autorisés |
| **7. Pilotage** | Tableau de bord, reporting, export CSV, journal d'audit, paramètres | parité fonctionnelle avec l'ancienne application |
| **8. Exploitation** | Service Windows et systemd, `apex-maint-admin` (adapté de Supervision), installateur Inno Setup, paquet DEB, `backup`/`restore` (sauvegarde SQLite à chaud + fichiers), purge, `import-legacy` | première version alpha installable, migration des données |
| **9. Terrain** | PWA installable, mode hors-ligne pour les interventions (service worker + IndexedDB, file d'attente synchronisée), scan de QR code | utilisable sur tablette ou téléphone en atelier |
| **10. Gamme Apex** | Liaison avec Apex Supervision : une alarme ou un compteur d'heures de marche crée une demande d'intervention ou alimente un planning `counter` | voir §5 |

Ordre conseillé : 0 → 1 → 2 → 3 → 4 → 5, puis 6 à 10 selon les priorités.
Les phases 0 et 1 fixent les conventions (Actor, erreurs, pagination, composants d'IHM) que tous les modules réutiliseront.

---

## 5. Points à trancher plus tard

> Les points 1 à 5 ont été tranchés le 2026-10-18 : voir `docs/specs/decisions.md`, qui prime sur
> cette section et sur le tableau du §4 (notamment : pas d'`import-legacy`, stockage des fichiers
> en dossier ou compatible S3, liaison avec Supervision facultative).

Phase 1 : les dépôts utilisent provisoirement `sqlx::query` / `query_as` et `FromRow`, sans macros vérifiées à la compilation ni dépendance à `DATABASE_URL`.

1. **Hors-ligne** : prévu en phase 9. Faut-il l'avancer ?
2. **Génération automatique d'interventions préventives** : à vérifier dans l'ancien code si elle existait ou si c'était manuel.
3. **Mot de passe oublié** : nécessite SMTP. Sinon, réinitialisation par un admin seulement (comme Supervision).
4. **Langue** : interface en français seulement (comme Supervision), ou i18n dès le départ.
5. **Liaison Supervision ↔ Maint** (phase 10) : API à clé de service entre les deux, ou lecture directe ? Format de l'événement (alarme, compteur), sens des échanges.
6. **Code partagé** : aujourd'hui on copie `apex-config`, `apex-build` et la sécurité depuis Supervision. Si les deux divergent peu, extraire plus tard un dépôt commun (`apex-common`) consommé par les deux produits.
7. **Dépôt public et signature** : SignPath exige un dépôt public. À décider au moment de la première version installable.

---

## 6. Écarts assumés par rapport à Apex Supervision

| Sujet | Supervision | Maint | Raison |
|---|---|---|---|
| Transport de la session | jeton dans `localStorage`, en-tête `X-Scada-Token` / `Bearer` | cookie `HttpOnly`, `SameSite=Strict`, `Secure` derrière HTTPS ; contrôle de l'en-tête `Origin` sur les requêtes qui modifient | défaut C1 de l'ancienne GMAO ; un jeton lisible en JavaScript est exposé à toute faille XSS. À reporter dans Supervision si le modèle donne satisfaction. |
| Multi-sites | non | oui, `site_id` partout | besoin métier de la GMAO |
| Port par défaut | 3000 | 3100 | cohabitation sur un même serveur |
| Temps réel | WebSocket au cœur du produit | pas nécessaire au départ ; éventuellement pour rafraîchir listes et tableau de bord | |
| Fichiers utilisateurs | symboles SVG assainis | documents jusqu'à 50 Mo, en streaming, hors base | |

---

## 7. Prérequis sur le poste

- Rust stable (via `rustup`), 1.95 ou plus récent (utilitaire egui), chaîne MSVC de préférence (comme la CI)
- `cargo install sqlx-cli --no-default-features --features sqlite`
- Node.js pour `node --check static/*.js`
- Plus tard : Inno Setup 6 (installateur Windows), `cargo-deb` dans un conteneur Ubuntu 22.04 (paquet DEB)
