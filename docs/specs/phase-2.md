# Phase 2 — Référentiel

Spécification de travail de la phase 2 (voir `docs/PLAN.md` §1.1, §3 et §4). Elle s'appuie sur la
phase 1 (`docs/specs/phase-1.md`) : sessions, `Actor`, filtre de site dans les dépôts, audit dans
la même transaction, format d'erreur `{ "error": true, "message": "…" }`.

## Résultat attendu

Lignes de production, équipements (arborescence et statut), emplacements (hiérarchie magasin >
allée > rack > bac) et fournisseurs : CRUD complets, suppression logique, audit, isolation entre
sites, listes paginées par curseur et filtrables.

## 1. Base de données — migration `0003_referentiel.sql`

Conventions communes : `id TEXT PRIMARY KEY` (UUID v7), `site_id TEXT NOT NULL REFERENCES
sites(id)`, `created_at`, `updated_at`, `deleted_at TEXT NULL`, tables `STRICT`, index composites
commençant par `site_id`. `code` unique par site parmi les non supprimés
(`CREATE UNIQUE INDEX … (site_id, code COLLATE NOCASE) WHERE deleted_at IS NULL`).

- `production_lines` : `code`, `name`, `description TEXT NULL`.
- `equipments` : `code`, `name`, `production_line_id TEXT NULL REFERENCES production_lines(id)`,
  `parent_id TEXT NULL REFERENCES equipments(id)`, `status TEXT NOT NULL DEFAULT 'operational'
  CHECK (status IN ('operational','degraded','out_of_service'))`, `manufacturer`, `model`,
  `serial_number`, `commissioned_on TEXT NULL` (date AAAA-MM-JJ), `description` (tous `TEXT NULL`).
  Le parent et la ligne appartiennent au même site (vérifié par le service). Pas de cycle dans
  l'arborescence (vérifié par le service). Profondeur maximale 8.
- `locations` : `code`, `name`, `kind TEXT NOT NULL CHECK (kind IN ('magasin','allee','rack',
  'bac'))`, `parent_id TEXT NULL REFERENCES locations(id)`. Règle de hiérarchie (service) : un
  `magasin` n'a pas de parent ; une `allee` a un `magasin` pour parent, un `rack` une `allee`, un
  `bac` un `rack`.
- `suppliers` : `name`, `contact_name`, `email`, `phone`, `address`, `website`, `notes`
  (tous `TEXT NULL` sauf `name`). Nom unique par site parmi les non supprimés.

## 2. Règles d'accès

| Ressource | Lecture | Création | Modification | Suppression |
|---|---|---|---|---|
| Lignes | `production_lines.view` | `production_lines.create` | `production_lines.edit` | `production_lines.delete` |
| Équipements | `equipments.view` | `equipments.create` | `equipments.edit` | `equipments.delete` |
| Emplacements | `locations.view` | `locations.manage` | `locations.manage` | `locations.manage` |
| Fournisseurs | `suppliers.view` | `suppliers.manage` | `suppliers.manage` | `suppliers.manage` |

- Non-admin : uniquement son site ; une ressource d'un autre site répond 404.
- Admin : les listes acceptent `?site_id=` (absent = tous les sites) ; la création exige
  `site_id` dans le corps (400 sinon) ; pour un non-admin, `site_id` est imposé par le serveur et
  toute valeur différente envoyée est refusée (403).
- Suppression logique refusée (409) si la ressource a des enfants non supprimés : ligne avec
  équipements, équipement avec sous-équipements, emplacement avec sous-emplacements.
- Chaque création, modification et suppression écrit `audit_log` dans la même transaction
  (`entity` : `production_line`, `equipment`, `location`, `supplier`).

## 3. API HTTP

Listes paginées par curseur : `?limit=` (1 à 200, défaut 50), `?cursor=` (opaque, `Cursor` de la
phase 0), réponse `{ "items": [...], "next_cursor": "…" | null }`, tri par `code` puis `id`
(fournisseurs : par `name` puis `id`). Filtre texte `?q=` (sur code et nom, insensible à la casse).

| Méthode et route | Filtres de liste | Remarques |
|---|---|---|
| `GET/POST /api/production-lines`, `GET/PATCH/DELETE /api/production-lines/{id}` | `q`, `site_id` | la représentation inclut `equipment_count` |
| `GET/POST /api/equipments`, `GET/PATCH/DELETE /api/equipments/{id}` | `q`, `site_id`, `status`, `production_line_id`, `parent_id` (`root` = racines) | la représentation inclut `production_line` `{id, code, name}` et `parent` `{id, code, name}` (ou `null`) et `children_count` |
| `GET /api/equipments/tree` | `site_id` | arborescence complète `[{id, code, name, status, children: [...]}]`, non paginée |
| `GET/POST /api/locations`, `GET/PATCH/DELETE /api/locations/{id}` | `q`, `site_id`, `kind`, `parent_id` (`root`) | |
| `GET /api/locations/tree` | `site_id` | comme l'arbre des équipements, avec `kind` |
| `GET/POST /api/suppliers`, `GET/PATCH/DELETE /api/suppliers/{id}` | `q`, `site_id` | |

`PATCH` : mise à jour partielle (champs absents inchangés, `null` explicite pour vider un champ
facultatif). Création : 201 et la ressource créée. Suppression : 204. Toute représentation inclut
`site_id`, `created_at`, `updated_at`.

## 4. IHM (`static/`)

`CURRENT_PHASE = 2`. Mêmes règles que la phase 1 (délégation `data-action`, `esc()`, `jsArg()`,
pas de style ni de script en ligne, thèmes sombre et clair, 375 px). Le site choisi dans
l'en-tête par l'admin filtre toutes les listes (`site_id`) ; à la création, l'admin choisit le
site si « Tous les sites » est sélectionné.

- Composants réutilisables à factoriser (ils serviront aux phases suivantes) : barre d'outils de
  liste (recherche avec délai de saisie, filtres), tableau paginé « Charger plus » sur
  `next_cursor`, panneau ou boîte de dialogue de formulaire avec validation et erreurs du serveur,
  confirmation de suppression, badge de statut.
- Lignes de production : liste (code, nom, nombre d'équipements), formulaire.
- Équipements : deux vues commutables, liste filtrable (statut, ligne) et arborescence repliable ;
  formulaire avec choix de la ligne et de l'équipement parent ; badge de statut coloré
  (`operational` = En service, `degraded` = Dégradé, `out_of_service` = Hors service) ; fiche de
  détail (informations, parent, sous-équipements).
- Emplacements : arborescence magasin > allée > rack > bac, ajout d'un enfant depuis un nœud (le
  type est déduit du parent), renommage, suppression.
- Fournisseurs : liste et formulaire (liens `mailto:` et `tel:` construits avec `esc()`).
- Boutons de création, modification et suppression masqués selon les permissions.

## 5. Tests boîte noire

CRUD de chaque ressource ; isolation entre sites (liste, accès direct, modification : 404) ;
refus 403 sans la permission ; suppression refusée avec enfants (409) ; cycle refusé dans
l'arborescence des équipements (400) ; hiérarchie d'emplacements invalide refusée (400) ;
pagination (`limit=2` sur 5 éléments : trois pages, aucun doublon) ; audit écrit.
