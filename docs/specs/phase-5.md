# Phase 5 — Maintenance préventive

Spécification de travail de la phase 5 (voir `docs/PLAN.md` §1.1 et §4, et
`docs/specs/decisions.md`, qui prime). Elle s'appuie sur les phases 1 à 4 : `Actor`, filtre de
site, 404 hors site, audit dans la même transaction, `BEGIN IMMEDIATE`, pagination
`{ items, next_cursor }`, format d'erreur `{ "error": true, "message": "…" }`, interventions de la
phase 3 (type `preventive`).

## Résultat attendu

- Des plannings de maintenance préventive par équipement, avec une fréquence calendaire ou de
  compteur.
- Des compteurs facultatifs par équipement, avec des relevés saisis à la main.
- Une génération automatique des interventions préventives N jours avant l'échéance, et une
  génération manuelle.
- Un calendrier fixe : l'échéance suivante est la date prévue + la fréquence.
- Une vue planning avec les échéances en retard, proches et à venir.
- Les échéances se mettent à jour à chaque réalisation, report ou annulation.

## 1. Base de données — migration `0006_maintenance.sql`

La migration `0005` est celle du stock (phase 4). Les conventions communes sont celles des phases
précédentes :
- `id TEXT PRIMARY KEY` (UUID v7), `site_id TEXT NOT NULL REFERENCES sites(id)` ;
- `created_at`, `updated_at`, `deleted_at TEXT NULL` ;
- tables `STRICT`, index composites commençant par `site_id`.

Les dates d'échéance sont des dates civiles `AAAA-MM-JJ` dans le fuseau local du serveur ; les
horodatages sont en RFC 3339.

### `equipment_counters` — compteurs d'un équipement (facultatifs)

| Colonne | Type et contrainte |
|---|---|
| `equipment_id` | `TEXT NOT NULL REFERENCES equipments(id)` |
| `name` | `TEXT NOT NULL` (ex. « Heures de marche », « Cycles ») |
| `unit` | `TEXT NOT NULL` (ex. `h`, `cycles`) |
| `current_value` | `INTEGER NOT NULL DEFAULT 0 CHECK (current_value >= 0)` |
| `last_reading_at` | `TEXT NULL` |
| `active` | `INTEGER NOT NULL DEFAULT 1 CHECK (active IN (0,1))` |

Le nom est unique par équipement parmi les compteurs non supprimés.

### `counter_readings` — historique des relevés

| Colonne | Type et contrainte |
|---|---|
| `counter_id` | `TEXT NOT NULL REFERENCES equipment_counters(id)` |
| `value` | `INTEGER NOT NULL CHECK (value >= 0)` |
| `read_at` | `TEXT NOT NULL` |
| `source` | `TEXT NOT NULL CHECK (source IN ('manual','supervision'))` ; `supervision` est réservé à la phase 10 |
| `is_reset` | `INTEGER NOT NULL DEFAULT 0` (remplacement ou remise à zéro du compteur) |
| `recorded_by_id` | `TEXT NULL REFERENCES users(id)` |
| `note` | `TEXT NULL` |

Un relevé ne peut pas être inférieur au précédent (400), sauf avec `is_reset = 1`. Une remise à zéro
décale aussi les échéances de compteur des plannings liés (voir §2.3).

### `maintenance_plans` — plannings

| Colonne | Type et contrainte |
|---|---|
| `code`, `title` | `TEXT NOT NULL` ; `code` unique par site parmi les non supprimés |
| `equipment_id` | `TEXT NOT NULL REFERENCES equipments(id)` |
| `description`, `instructions` | `TEXT NULL` ; `instructions` est la gamme opératoire, recopiée dans l'intervention générée |
| `priority` | `TEXT NOT NULL DEFAULT 'medium'`, mêmes valeurs que les interventions |
| `frequency` | `TEXT NOT NULL CHECK (frequency IN ('daily','weekly','monthly','quarterly','yearly','interval_days','counter'))` |
| `interval_days` | `INTEGER NULL CHECK (interval_days IS NULL OR interval_days BETWEEN 1 AND 3650)` ; obligatoire si et seulement si `frequency = 'interval_days'` |
| `counter_id` | `TEXT NULL REFERENCES equipment_counters(id)` ; obligatoire si et seulement si `frequency = 'counter'` |
| `counter_interval` | `INTEGER NULL CHECK (counter_interval IS NULL OR counter_interval > 0)` ; obligatoire si et seulement si `frequency = 'counter'` |
| `anchor_date` | `TEXT NULL` : date de la première échéance des plannings calendaires, qui sert d'ancrage à toutes les suivantes |
| `next_due_date` | `TEXT NULL` : prochaine échéance des plannings calendaires |
| `anchor_counter` | `INTEGER NULL` : valeur de référence des plannings de compteur (par défaut la valeur actuelle du compteur) ; la première échéance vaut `anchor_counter + counter_interval` |
| `next_due_counter` | `INTEGER NULL` : prochaine valeur d'échéance des plannings de compteur |
| `lead_days` | `INTEGER NOT NULL DEFAULT 7 CHECK (lead_days BETWEEN 0 AND 365)` : avance de génération des plannings calendaires |
| `lead_counter` | `INTEGER NOT NULL DEFAULT 0 CHECK (lead_counter >= 0)` : avance de génération des plannings de compteur |
| `auto_generate` | `INTEGER NOT NULL DEFAULT 1` |
| `active` | `INTEGER NOT NULL DEFAULT 1` |
| `estimated_minutes` | `INTEGER NULL` |
| `default_technician_id` | `TEXT NULL REFERENCES users(id)` : technicien assigné d'office à l'intervention générée |

Les contraintes `CHECK` imposent la cohérence entre `frequency` et ses colonnes. L'équipement, le
compteur et le technicien appartiennent au même site que le planning (vérifié par le service).

### `maintenance_occurrences` — échéances traitées

| Colonne | Type et contrainte |
|---|---|
| `plan_id` | `TEXT NOT NULL REFERENCES maintenance_plans(id)` |
| `due_date` | `TEXT NULL` |
| `due_counter` | `INTEGER NULL` |
| `status` | `TEXT NOT NULL CHECK (status IN ('generated','done','skipped','missed'))` |
| `intervention_id` | `TEXT NULL REFERENCES interventions(id)` |
| `generated_at`, `closed_at` | `TEXT NULL` |
| `generation` | `TEXT NULL CHECK (generation IN ('auto','manual'))` |
| `skip_reason` | `TEXT NULL` ; obligatoire si `status = 'skipped'` |

Index et contraintes d'unicité :
- `UNIQUE (plan_id, due_date)` et `UNIQUE (plan_id, due_counter)` ;
- **une seule occurrence `generated` par planning**, par l'index unique partiel
  `(plan_id) WHERE status = 'generated'` ;
- index `(site_id, status)`.

### `interventions`

Ajout de la colonne `maintenance_occurrence_id TEXT NULL REFERENCES maintenance_occurrences(id)`
(lien inverse, pour la fiche d'intervention).

## 2. Règles de calcul

### 2.1 Échéances calendaires (calendrier fixe)

La n-ième échéance vaut `anchor_date + n × période`. Les périodes sont : jour, semaine, mois,
3 mois, an, ou `interval_days` jours. Pour les mois et les années, le jour est ramené au dernier
jour du mois si besoin : un ancrage au 31 janvier donne le 28 ou 29 février, puis le 31 mars, sans
dérive.

Un retard de réalisation **ne décale pas** les échéances suivantes. À la clôture d'une occurrence
(réalisée, reportée ou annulée), `next_due_date` devient la première échéance ancrée strictement
postérieure à celle qui vient d'être traitée.

Si cette échéance est elle-même déjà dépassée de plus d'une période (planning resté longtemps
ouvert), les échéances entièrement écoulées entre-temps sont enregistrées en `missed`, pour la
traçabilité. `next_due_date` passe alors à la dernière échéance ancrée inférieure ou égale à
aujourd'hui, qui apparaît en retard. On ne génère jamais de rattrapage en rafale.

### 2.2 Échéances de compteur

La n-ième échéance vaut `anchor_counter + n × counter_interval`. Le calendrier est fixe ici aussi :
à la clôture, `next_due_counter` augmente de `counter_interval`, et les échéances déjà franchies
passent en `missed` selon la même règle.

Un planning de compteur ne peut être créé que pour un compteur **actif** de l'équipement du
planning (400 sinon). Désactiver un compteur suspend la génération des plannings liés, qui sont
signalés dans l'IHM.

### 2.3 Remise à zéro d'un compteur

Un relevé avec `is_reset = 1` ramène `current_value` à la valeur saisie. Pour chaque planning lié,
la distance restante jusqu'à l'échéance est conservée : `next_due_counter = nouvelle valeur +
(ancienne échéance − ancienne valeur)`. Les ancrages sont recalculés en conséquence, et l'opération
est auditée.

### 2.4 Génération des interventions

Un planning est **dû à générer** s'il remplit toutes ces conditions :
- il est actif et non supprimé ;
- il n'a pas d'occurrence `generated` ;
- pour un planning calendaire, `next_due_date − lead_days ≤ aujourd'hui` ;
- pour un planning de compteur, son compteur est actif et `current_value ≥ next_due_counter − lead_counter`.

**Génération automatique** : une tâche de fond du moteur tourne au démarrage puis toutes les
15 minutes, et traite les plannings dus qui ont `auto_generate = 1`. Elle s'exécute aussi juste
après chaque relevé de compteur, pour les plannings de ce compteur.

**Génération manuelle** : `POST /api/maintenance/plans/{id}/generate`. Elle est possible même
avant la fenêtre d'avance et même si `auto_generate = 0`. Elle répond 409 si une occurrence est déjà
ouverte.

Chaque génération se fait **dans une seule transaction `BEGIN IMMEDIATE`** :
- création de l'occurrence `generated` ;
- création de l'intervention `preventive` sur l'équipement du planning. Son titre est celui du
  planning ; sa description reprend les `instructions` et l'échéance ; sa priorité est celle du
  planning ; elle est assignée au technicien par défaut s'il est encore actif sur le site ;
- écriture de l'audit. Pour la tâche de fond, `user_id` est `NULL` et `details.source = "auto"`.

L'index unique partiel empêche toute double génération concurrente : la tâche de fond et la
génération manuelle simultanées ne créent qu'une intervention. Le perdant reçoit 409 ou ignore
le planning.

### 2.5 Clôture d'une occurrence

| Événement | Effet sur l'occurrence | Effet sur le planning |
|---|---|---|
| L'intervention liée passe à `completed` | `done`, avec `closed_at` | avance de l'échéance |
| L'intervention liée passe à `cancelled` | `skipped`, avec `skip_reason` = motif d'annulation | avance de l'échéance |
| Report d'une échéance sans intervention (`POST /api/maintenance/plans/{id}/skip { reason }`) | occurrence `skipped` créée | avance de l'échéance ; refusé (409) si une occurrence est ouverte |

Le hook se place dans le service des interventions (phase 3), dans la même transaction que la
clôture ou l'annulation.

La suppression logique d'une intervention générée remet l'occurrence en échéance non traitée :
l'occurrence est supprimée et le planning garde son échéance.

### 2.6 Effets sur les autres modules

- Supprimer un équipement qui a des plannings actifs est refusé (409), en complément des règles de
  la phase 2.
- Supprimer un compteur utilisé par un planning non supprimé est refusé (409).
- Modifier la fréquence, l'ancrage ou l'intervalle d'un planning recalcule `next_due_*` à partir du
  nouvel ancrage. C'est refusé (409) tant qu'une occurrence est ouverte.

## 3. Règles d'accès

| Action | Permission | Rôles par défaut |
|---|---|---|
| Voir les plannings, le planning et les compteurs | `maintenance.view` | tous |
| Créer, modifier ou supprimer un planning, générer, reporter | `maintenance.manage` | `admin`, `responsable` |
| Configurer les compteurs d'un équipement | `equipments.edit` | `admin`, `responsable` |
| Saisir un relevé de compteur | `counters.record` (nouvelle) | `admin`, `responsable`, `technicien`, `production` |

Mêmes règles de site que les phases précédentes :
- un non-admin ne voit que son site, et une ressource d'un autre site répond 404 ;
- l'admin filtre avec `?site_id=` ;
- `site_id` est imposé à la création.

## 4. API HTTP

| Méthode et route | Remarques |
|---|---|
| `GET/POST /api/equipments/{id}/counters`, `PATCH/DELETE /api/counters/{id}` | La représentation inclut `plans_count`. |
| `GET/POST /api/counters/{id}/readings` | Liste paginée, du plus récent au plus ancien. Création : `{ value, read_at?, is_reset?, note? }`, réponse 201. Déclenche la génération des plannings dus de ce compteur. |
| `GET/POST /api/maintenance/plans`, `GET/PATCH/DELETE /api/maintenance/plans/{id}` | Filtres : `q`, `site_id`, `equipment_id`, `frequency`, `active`, `due` (`overdue`, `soon` sous 30 jours, `all`). La représentation inclut `equipment {id, code, name}`, `counter {id, name, unit, current_value}` ou `null`, `open_occurrence {id, intervention_id, intervention_code}` ou `null`, et `state` (`overdue`, `due_soon`, `ok`, `suspended`). |
| `POST /api/maintenance/plans/{id}/generate` | Réponse 201 `{ occurrence, intervention }`. |
| `POST /api/maintenance/plans/{id}/skip` | Corps `{ reason }`, réponse 200 avec le planning mis à jour. |
| `GET /api/maintenance/plans/{id}/occurrences` | Historique paginé. |
| `GET /api/maintenance/schedule?from=&to=&site_id=&equipment_id=` | Vue planning, non paginée, période de 366 jours au plus (400 au-delà). Elle renvoie les échéances projetées des plannings calendaires actifs (dates futures calculées, et occurrences passées réelles) : `[{ date, plan {id, code, title}, equipment {id, code, name}, status: "projected" \| "generated" \| "done" \| "skipped" \| "missed" \| "overdue", intervention_id? }]`. Les plannings de compteur figurent avec leur valeur restante, sans date (`remaining`). |

`PATCH` est partiel, avec `null` explicite pour vider un champ. Les représentations incluent
`site_id`, `created_at` et `updated_at`.

## 5. IHM (`static/`)

`CURRENT_PHASE = 5`. Le module « Maintenance préventive » devient actif. Mêmes règles que les phases
précédentes : `data-action`, `esc()`, `jsArg()`, pas de style ni de script en ligne, deux thèmes,
375 px. On réutilise les composants factorisés en phase 2.

- **Plannings** : liste filtrable (équipement, fréquence, état), avec un badge d'état (En retard,
  Bientôt, À jour, Suspendu) et la prochaine échéance (date ou « dans 120 h »).
  - Formulaire : le choix de la fréquence fait apparaître l'intervalle, ou le compteur et son pas.
    La liste des compteurs se limite à ceux de l'équipement choisi.
  - Actions : « Générer maintenant », « Reporter » (motif obligatoire), activer ou désactiver.
- **Fiche planning** : paramètres, échéance, occurrence ouverte avec un lien vers l'intervention,
  historique des occurrences.
- **Vue planning** : vue mensuelle (grille calendrier ; liste par jour à 375 px) et liste des
  échéances des 30, 60 ou 90 prochains jours, avec les retards en tête. Les échéances en retard sont
  mises en évidence, et un clic ouvre le planning ou l'intervention.
- **Fiche équipement** (phase 2) : section « Compteurs ». On y voit la valeur et le dernier relevé
  de chaque compteur. Le bouton « Saisir un relevé » et l'option de remise à zéro, avec
  confirmation, sont visibles avec `counters.record`. L'historique des relevés s'affiche. Ajout et
  modification des compteurs avec `equipments.edit`. On y liste aussi les plannings de l'équipement.
- **Fiche intervention** (phase 3) : pour une intervention préventive, lien vers son planning et
  son échéance.

## 6. Tests

### Tests unitaires (calcul des échéances)

- Chaque fréquence.
- Ancrage au 31 janvier : 28 ou 29 février, puis 31 mars.
- Années bissextiles.
- Calendrier fixe malgré un retard.
- Échéances `missed` après une longue interruption.
- Échéances de compteur, et remise à zéro qui conserve la distance restante.

### Tests boîte noire

- CRUD des plannings et des compteurs.
- Isolation entre sites (404).
- 403 sans `maintenance.manage`.
- Cohérence fréquence et colonnes (400).
- Compteur d'un autre équipement refusé (400).
- Génération manuelle :
  - elle crée l'intervention `preventive` et l'occurrence ;
  - une deuxième génération répond 409.
- Clôture de l'intervention :
  - l'occurrence passe à `done` ;
  - `next_due_date` vaut l'échéance précédente + la période, pas la date de clôture.
- Annulation de l'intervention :
  - l'occurrence passe à `skipped` ;
  - l'échéance avance.
- Report sans intervention.
- Relevé :
  - un relevé inférieur au précédent est refusé (400) ;
  - un relevé qui franchit le seuil génère l'intervention automatiquement si `auto_generate = 1`, et
    ne génère rien sinon.
- Génération automatique par la tâche de fond : intervalle de passage réglable en test par la
  variable `APEX_MAINT_TEST_SCHEDULER_SECONDS`.
- Deux générations concurrentes ne créent qu'une seule intervention.
- Suppression d'un équipement qui a des plannings : 409.
- Vue planning :
  - les échéances sont projetées sur la période ;
  - une période de plus de 366 jours est refusée.
- Audit écrit, y compris pour la génération automatique (`user_id` nul).
