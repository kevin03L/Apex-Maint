# Phase 1 — Authentification, sites et utilisateurs

Spécification de travail de la phase 1 (voir `docs/PLAN.md` §1.1, §1.3, §2.3, §4 et §6).
Elle s'appuie sur le socle de la phase 0 : `apex-core` (`MaintError`, `db`, `domain::{actor,
permission, pagination}`, migration `0001_socle.sql`) et `apex-server` (`app.rs`, `http.rs`,
tests boîte noire dans `tests/`). Lire `AGENTS.md` avant de commencer : ses règles sont impératives.

Référence de comportement : Apex Supervision, **en lecture seule**
(`D:\Vs-code\Apex-Supervision\crates\apex-core\src\security.rs` pour le hachage, le jeton et le coût
constant ; `core.rs` pour le verrouillage, `LOGIN_MAX_FAILURES`, le sémaphore de vérification et le
compte admin d'usine ; `apex-server/src/http.rs` pour les routes `/api/auth/*`). On reprend ses
idées et son style ; l'écart assumé est le transport de la session (cookie `HttpOnly`, PLAN §6).

## Résultat attendu

`apex-maint config init --portable` puis `apex-maint run` : l'IHM affiche une page de connexion ;
`admin` / `admin123` ouvre une session et impose le changement du mot de passe d'usine ; ensuite
l'admin gère les sites, les utilisateurs et la matrice des permissions ; un utilisateur d'un site ne
voit et ne modifie que les données de son site ; le menu n'affiche que les modules autorisés.

## 1. Base de données — migration `0002_auth.sql`

Tables `STRICT`, identifiants UUID v7 en `TEXT`, dates RFC 3339 UTC en `TEXT`.

- `users` : `id`, `site_id TEXT NULL REFERENCES sites(id)`, `username TEXT NOT NULL`,
  `display_name TEXT NOT NULL`, `role TEXT NOT NULL CHECK (role IN ('admin','responsable',
  'production','technicien'))`, `password_hash TEXT NOT NULL`, `must_change_password INTEGER NOT NULL
  DEFAULT 0 CHECK (… IN (0,1))`, `active INTEGER NOT NULL DEFAULT 1 CHECK (… IN (0,1))`,
  `failed_attempts INTEGER NOT NULL DEFAULT 0`, `locked_until TEXT NULL`, `last_login_at TEXT NULL`,
  `created_at`, `updated_at`, `deleted_at TEXT NULL`.
  - `CHECK (role = 'admin' OR site_id IS NOT NULL)` : seul l'admin peut être sans site (il voit tous
    les sites) ; tout autre rôle est rattaché à exactement un site.
  - Index unique `username COLLATE NOCASE` **parmi les comptes non supprimés**
    (`CREATE UNIQUE INDEX … WHERE deleted_at IS NULL`).
  - Index `(site_id, deleted_at)`.
- `role_permissions` : `role TEXT NOT NULL` (même `CHECK`), `permission TEXT NOT NULL`,
  `PRIMARY KEY (role, permission)`. Pré-remplie avec la matrice par défaut ci-dessous. Le rôle `admin`
  n'y figure pas : il a toujours toutes les permissions (codé en Rust, non modifiable).
- `sessions` : `token_hash TEXT PRIMARY KEY` (SHA-256 hexadécimal du jeton, le jeton brut n'est
  jamais stocké), `user_id TEXT NOT NULL REFERENCES users(id)`, `created_at`, `last_seen_at`,
  `ip TEXT NULL`, `user_agent TEXT NULL`, `revoked_at TEXT NULL`. Index `(user_id)`.
- `audit_log` : `id`, `site_id TEXT NULL`, `user_id TEXT NULL`, `action TEXT NOT NULL`
  (`create`, `update`, `delete`, `login`, `login_failed`, `logout`, `password_change`,
  `password_reset`, `lock`), `entity TEXT NOT NULL` (`site`, `user`, `role_permissions`, `session`),
  `entity_id TEXT NULL`, `before_json TEXT NULL`, `after_json TEXT NULL`, `ip TEXT NULL`,
  `created_at`. Index `(site_id, created_at)`. Ne jamais y écrire de hachage ni de mot de passe.

Matrice par défaut (clés de `Permission`, déjà définies en phase 0) :

| Rôle | Permissions |
|---|---|
| `responsable` | toutes sauf `settings.edit` (`users.view` et `users.manage` restent limités à son site, voir §4) |
| `technicien` | `dashboard.view`, `equipments.view`, `production_lines.view`, `interventions.{view,create,edit}`, `parts.view`, `stock.{view,movements}`, `documents.{view,create}`, `locations.view`, `maintenance.view`, `suppliers.view` |
| `production` | `dashboard.view`, `equipments.view`, `production_lines.view`, `interventions.{view,create}`, `documents.view`, `maintenance.view` |

## 2. Premier démarrage et démonstration

- Si aucun site non supprimé n'existe : créer « Site principal ».
- Si aucun admin actif non supprimé n'existe : créer `admin` / `admin123` (rôle `admin`,
  `site_id` NULL, `must_change_password = 1`) et écrire un avertissement `warn!` dans les journaux.
  À chaque démarrage, tant qu'un admin a encore le mot de passe d'usine, répéter l'avertissement
  (comme `warn_factory_passwords` de Supervision).
- Si `[demo] enabled = true` : créer, une seule fois, le site « Site de démonstration » et les
  comptes `responsable`, `technicien`, `production` (mot de passe `demo1234`, sans changement
  imposé), rattachés à ce site. Documenter ces comptes dans le README. `config init --demo`
  existe déjà : vérifier qu'il active bien `[demo]`.

## 3. Sécurité de l'authentification (`apex-core`)

Dépendances à ajouter au workspace : `argon2`, `sha2` (et ce qu'il faut pour le hexadécimal ou le
base64, sans crate lourde). Les profils `argon2`/`blake2` du `Cargo.toml` racine existent déjà.

- `domain::password` (ou équivalent) : `hash_password` (Argon2id, paramètres par défaut de la crate),
  `verify_password`, vérification à **coût constant** quand le compte n'existe pas (hachage factice
  préparé une fois), `validate_new_password` (8 caractères minimum, différent de l'ancien,
  différent de `admin123`), messages en français.
- Jeton de session : 256 bits aléatoires (`rand`), encodés en hexadécimal ; stocké en base sous
  forme de SHA-256.
- Verrouillage : après **5 échecs consécutifs**, compte verrouillé **60 s** (`locked_until`).
  L'incrément de `failed_attempts` et la pose du verrou se font en **une seule requête `UPDATE`
  atomique** (pas de lecture puis écriture), pour qu'un tir concurrent ne contourne pas la limite.
  Pendant le verrou, même le bon mot de passe est refusé (HTTP 429, message avec le délai restant).
  Un succès remet le compteur à zéro. Durée et seuil en constantes ; prévoir une variable
  d'environnement **réservée aux tests** `APEX_MAINT_TEST_LOCK_SECONDS` pour raccourcir le verrou
  dans les tests boîte noire.
- Sémaphore : au plus 4 vérifications Argon2 simultanées, exécutées dans `spawn_blocking`.
- Message d'échec générique « Identifiant ou mot de passe incorrect » (pas d'énumération des
  comptes), sauf pour le verrou et le compte désactivé (désactivé : même message générique).
- Expiration d'inactivité : `[security] session_ttl_hours` de la configuration. `last_seen_at` mis à
  jour au plus une fois par minute et par session. Session expirée ou révoquée = 401.
- Révocation (pose de `revoked_at` sur toutes les sessions du compte) quand : le compte est
  désactivé, supprimé, change de rôle ou de site, son mot de passe est réinitialisé par un
  gestionnaire. Changement de son propre mot de passe : révoque toutes les **autres** sessions.
- Ajouter `MaintError::TooManyRequests` (429) et son test de correspondance HTTP.

## 4. Dépôts, services et règles d'accès (`apex-core`)

Organisation suggérée : `store/{sites,users,sessions,permissions,audit}.rs` (SQL), `services/
{auth,users,sites}.rs` (règles, transactions, audit). Requêtes `sqlx::query`/`query_as` avec
`FromRow` (pas les macros `query!`, pour que la CI n'ait pas besoin de `DATABASE_URL`).

- Toute fonction de dépôt et de service métier reçoit un `&Actor` et applique le filtre de site
  **dans le dépôt** : un non-admin ne voit que `site_id = actor.site_id`. Une ressource d'un autre
  site répond **404** (et non 403), pour ne pas révéler son existence.
- Suppression logique uniquement (`deleted_at`) ; les listes excluent les supprimés.
- Chaque modification de site, d'utilisateur ou de matrice écrit sa ligne d'`audit_log` **dans la
  même transaction** (avant/après en JSON, sans `password_hash`).
- `Actor` : construit à partir de la session ; permissions = `Permission::ALL` pour l'admin, sinon
  celles de `role_permissions` pour son rôle.
- Sites : lecture pour tout utilisateur connecté (admin : tous ; autres : le sien) ; création,
  modification, suppression logique réservées à l'admin. Refuser la suppression d'un site qui a
  encore des utilisateurs actifs (409).
- Utilisateurs : `users.view` pour lister/consulter, `users.manage` pour créer, modifier,
  désactiver, réinitialiser le mot de passe, supprimer. Un non-admin avec `users.manage` :
  uniquement sur son site, ne peut ni créer ni promouvoir un `admin`, ni déplacer un compte vers un
  autre site. Garde-fous pour tous : impossible de se désactiver, se supprimer ou se rétrograder
  soi-même ; impossible de désactiver, supprimer ou rétrograder le **dernier admin actif** (409).
- Matrice : lecture et modification réservées à l'admin ; le rôle `admin` n'est pas modifiable ;
  toute clé inconnue est refusée (400).

Tests unitaires (base en mémoire, `db::open(None)`) : hachage et vérification, verrouillage
atomique, filtre de site des dépôts, garde-fous du dernier admin, audit écrit dans la transaction
(et absent si la transaction échoue).

## 5. API HTTP (`apex-server`)

Découper `http.rs` en modules (`http/mod.rs`, `http/auth.rs`, `http/sites.rs`, `http/users.rs`,
`http/permissions.rs`) si cela reste lisible. Toutes les réponses d'erreur gardent le format JSON
actuel `{ "error": true, "message": "…" }` avec le code de `MaintError::http_status`.

Cookie de session : nom `apex_maint_session`, `HttpOnly`, `SameSite=Strict`, `Path=/`, `Secure`
si `[security] secure_cookie = true`, sans `Max-Age` (cookie de session navigateur). La déconnexion
renvoie le même cookie expiré. Aucun jeton dans un corps de réponse JSON.

Protection CSRF : sur toute requête `POST`/`PUT`/`PATCH`/`DELETE` vers `/api/`, refuser (403) si
l'en-tête `Origin` est présent et ne correspond pas à l'hôte servi (`Host`), ou si
`Sec-Fetch-Site` vaut `cross-site` ou `same-site`. Les routes `/api/system/*` protégées par le
jeton de contrôle local restent inchangées.

Changement de mot de passe imposé : tant que `must_change_password = 1`, toute route authentifiée
répond 403 avec `"code": "password_change_required"` dans le JSON, **sauf** `GET /api/auth/me`,
`POST /api/auth/password` et `POST /api/auth/logout`.

Limites de taille des corps JSON : 64 Kio.

| Méthode et route | Accès | Effet |
|---|---|---|
| `POST /api/auth/login` | public | `{username, password}` → 200 `{user}` + cookie ; 401 ; 429 si verrouillé |
| `POST /api/auth/logout` | session | révoque la session, expire le cookie, 204 |
| `GET /api/auth/me` | session | `{user, permissions, site, must_change_password}` |
| `POST /api/auth/password` | session | `{current_password, new_password}` ; révoque les autres sessions |
| `GET /api/sites` | session | liste filtrée selon l'acteur |
| `POST /api/sites`, `PATCH /api/sites/{id}`, `DELETE /api/sites/{id}` | admin | |
| `GET /api/users`, `GET /api/users/{id}` | `users.view` | filtrés par site |
| `POST /api/users`, `PATCH /api/users/{id}`, `DELETE /api/users/{id}` | `users.manage` | |
| `POST /api/users/{id}/password` | `users.manage` | réinitialise, `must_change_password = 1`, révoque |
| `GET /api/permissions` | admin | matrice + liste des clés connues |
| `PUT /api/permissions/{role}` | admin | remplace les permissions du rôle |

`GET /api/health` reste public et inchangé. La représentation JSON d'un utilisateur ne contient
jamais `password_hash`, `failed_attempts` ni `locked_until` (seulement un booléen `locked`).

## 6. IHM (`static/`)

Rappels : pas de framework, pas de script ni de style en ligne (CSP `default-src 'self'`), donc
**pas d'attribut `onclick`** : délégation d'événements sur `data-action` avec les arguments dans
`data-args="${jsArg(…)}"`. Toute donnée insérée dans le HTML passe par `esc()`. Textes en français,
icônes SVG de la bibliothèque d'`index.html` (en ajouter si besoin), thèmes sombre et clair à
respecter (utiliser les variables CSS existantes, rien en dur). Aucun jeton lu ou stocké en JS.

- `CURRENT_PHASE = 1`.
- Au démarrage : `GET /api/auth/me`. 401 → page de connexion plein écran (logo, nom de l'instance,
  identifiant, mot de passe, message d'erreur, état « Connexion… »). Toute réponse 401 ultérieure
  ramène à la page de connexion.
- Changement imposé : écran dédié tant que `must_change_password` est vrai (ancien, nouveau,
  confirmation), avec rappel des règles.
- En-tête : le bouton utilisateur affiche le nom et le rôle, et ouvre un menu (Mon profil, Changer
  mon mot de passe, Se déconnecter). Le sélecteur de site liste les sites accessibles ; pour un
  non-admin il est figé sur son site ; pour l'admin, le choix (« Tous les sites » ou un site) est
  conservé dans l'état de l'IHM pour les modules des phases suivantes.
- Menu filtré : chaque module de `MODULES` porte la permission `*.view` qui l'affiche
  (`tableau-de-bord` → `dashboard.view`, `utilisateurs` → `users.view`, etc.) ; ajouter le module
  « Sites » (groupe Administration, admin seulement, phase 1). Les modules non autorisés
  disparaissent ; la route directe affiche « Accès refusé ».
- Page Utilisateurs : tableau (nom, identifiant, rôle, site, état actif/verrouillé, dernière
  connexion), création et modification dans un panneau ou une boîte de dialogue, réinitialisation
  du mot de passe, désactivation, suppression avec confirmation. Boutons masqués sans
  `users.manage`. Onglet « Permissions » (admin) : matrice rôles × permissions à cases à cocher,
  regroupées par domaine, colonne admin cochée et grisée, bouton Enregistrer.
- Page Sites (admin) : liste, création, renommage, suppression avec confirmation.
- Page Mon profil : informations du compte et changement de mot de passe.
- Responsive : utilisable à 375 px de large (tableaux qui défilent horizontalement dans leur
  conteneur, pas la page).

## 7. Tests boîte noire (`crates/apex-server/tests/`)

Étendre l'outillage `common` (cookies, `Origin`, JSON) et ajouter `tests/auth.rs` au minimum :

1. Connexion : bon mot de passe → 200 et cookie avec `HttpOnly`, `SameSite=Strict`, `Path=/` ;
   mauvais → 401 ; aucune route protégée sans cookie (401).
2. `must_change_password` : l'admin d'usine reçoit 403 `password_change_required` sur
   `/api/users`, puis accès après changement.
3. Verrouillage **concurrent** : 10 tentatives erronées lancées en parallèle → le compte est
   verrouillé et le bon mot de passe renvoie 429 ; après le délai (variable de test courte), la
   connexion réussit.
4. Révocation : déconnexion → l'ancien cookie renvoie 401 ; changement de mot de passe → les autres
   sessions renvoient 401 ; désactivation par l'admin → la session du compte renvoie 401.
5. Isolation entre sites : un `responsable` du site A ne voit pas les utilisateurs du site B (absent
   des listes, 404 en accès direct, 404 en modification) ; un `technicien` reçoit 403 sur
   `/api/users`.
6. CSRF : `POST /api/auth/logout` avec `Origin: http://evil.example` → 403.
7. Garde-fou : impossible de supprimer ou rétrograder le dernier admin (409).
8. Persistance : une session survit au redémarrage du moteur (même base).

Ports : uniquement des ports dynamiques (`common::free_port`), jamais 3000 ni 3100.

## 8. Documentation

- README : statut « phase 1 », connexion `admin` / `admin123` avec changement imposé, comptes de
  démonstration.
- `docs/PLAN.md` : ne pas réécrire ; au besoin, une ligne au §5 pour noter l'usage provisoire de
  `sqlx::query` sans macros vérifiées à la compilation.

## 9. Fin de tâche

Avant de rendre la main, lancer et faire passer : `cargo fmt --all`, `cargo clippy --workspace
--all-targets -- -D warnings`, `cargo test --workspace`, `node --check static/app.js static/theme.js`.
Ne rien commiter. Ne pas toucher à `D:\Vs-code\Apex-Supervision`, ni à la toolchain, ni aux
fichiers hors du dépôt. Rendre un résumé : fichiers créés ou modifiés, décisions prises, points
restés ouverts.
