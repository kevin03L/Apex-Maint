# Phase 3 — Interventions

Spécification de travail de la phase 3 (voir `docs/PLAN.md` §1.1, §1.3 I1, §2.3, §3 et §4). Elle s'appuie sur la phase 1 (`docs/specs/phase-1.md` : sessions, `Actor`, permissions, audit dans la même transaction, format d'erreur `{ "error": true, "message": "…" }`) et la phase 2 (`docs/specs/phase-2.md` : équipements, statut opérationnel, lignes, composants d'IHM factorisés).

## Résultat attendu

Gestion complète du cycle de vie des interventions de maintenance (corrective, préventive, a posteriori, hors équipement) : création, prise en charge, clôture, sous-interventions, assignation des techniciens, suivi des temps réels et de l'état machine, impact dynamique et justifié sur le statut de l'équipement, liste filtrable paginée par curseur et page de détail offrant un parcours technicien fluide sur PC comme sur terminal mobile d'atelier (375 px).

## 1. Base de données — migration `0004_interventions.sql`

Conventions communes : `id TEXT PRIMARY KEY` (UUID v7), `site_id TEXT NOT NULL REFERENCES sites(id)`, `created_at`, `updated_at`, `deleted_at TEXT NULL`, tables `STRICT`, index composites commençant par `site_id`. Référence d'intervention `code` unique par site parmi les non supprimées (`CREATE UNIQUE INDEX … (site_id, code COLLATE NOCASE) WHERE deleted_at IS NULL`).

- `interventions` :
  - `id TEXT PRIMARY KEY NOT NULL`
  - `site_id TEXT NOT NULL REFERENCES sites(id)`
  - `code TEXT NOT NULL` (format indicatif `INT-AAAA-XXXX`, unique par site)
  - `title TEXT NOT NULL`
  - `description TEXT` (description de la demande ou de l'anomalie constatée)
  - `type TEXT NOT NULL CHECK (type IN ('corrective','preventive','a_posteriori','hors_equipement'))`
  - `priority TEXT NOT NULL DEFAULT 'medium' CHECK (priority IN ('low','medium','high','critical'))`
  - `status TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open','in_progress','completed'))`
  - `machine_state TEXT NOT NULL DEFAULT 'FONCTIONNEL' CHECK (machine_state IN ('ARRET','DEGRADE','FONCTIONNEL'))`
  - `equipment_id TEXT REFERENCES equipments(id)`
  - `parent_id TEXT REFERENCES interventions(id)` (sous-intervention rattachée à une intervention principale)
  - `reported_by_id TEXT NOT NULL REFERENCES users(id)` (demandeur initial)
  - `reported_at TEXT NOT NULL` (date de constat ou de demande, RFC 3339)
  - `started_at TEXT` (début effectif des travaux, RFC 3339)
  - `completed_at TEXT` (fin effective des travaux, RFC 3339)
  - `duration_minutes INTEGER CHECK (duration_minutes IS NULL OR duration_minutes >= 0)` (durée réelle passée)
  - `solution_report TEXT` (compte-rendu d'intervention : diagnostic, action corrective, cause racine)
  - `created_at TEXT NOT NULL`
  - `updated_at TEXT NOT NULL`
  - `deleted_at TEXT`
  - **Contrainte CHECK sur `equipment_id` (défaut I1 de l'ancien audit)** :
    `CHECK ((type = 'hors_equipement' AND equipment_id IS NULL) OR (type != 'hors_equipement' AND equipment_id IS NOT NULL))`
  - **Contrainte CHECK sur la cohérence temporelle** :
    `CHECK (completed_at IS NULL OR started_at IS NULL OR completed_at >= started_at)`
  - Index :
    - `(site_id, code COLLATE NOCASE) WHERE deleted_at IS NULL` (unique)
    - `(site_id, status, deleted_at)`
    - `(site_id, equipment_id, deleted_at)`
    - `(site_id, parent_id, deleted_at)`
    - `(site_id, created_at DESC)`

- `intervention_technicians` (techniciens assignés à l'intervention) :
  - `intervention_id TEXT NOT NULL REFERENCES interventions(id)`
  - `user_id TEXT NOT NULL REFERENCES users(id)`
  - `is_lead INTEGER NOT NULL DEFAULT 0 CHECK (is_lead IN (0,1))` (technicien référent / responsable principal)
  - `assigned_at TEXT NOT NULL`
  - `PRIMARY KEY (intervention_id, user_id)`
  - Index `(user_id, intervention_id)`

## 2. Cycle de vie, transitions et effet sur l'équipement

### 2.1 Machine à états finis des interventions

Le cycle de vie est strictement encadré par des transitions autorisées :
- `open` → `in_progress` : prise en charge par un ou plusieurs techniciens. Renseigne automatiquement `started_at` si non renseigné.
- `in_progress` → `completed` : clôture de l'intervention. Renseigne `completed_at`, exige `solution_report` (non vide) et `duration_minutes`.
- `open` → `completed` : clôture directe autorisée (notamment pour le type `a_posteriori` saisi après réalisation des travaux, ou dépannage immédiat). Renseigne `started_at`, `completed_at`, `solution_report` et `duration_minutes`.

**Toute autre transition est formellement interdite et répond HTTP 409 Conflict** :
- `completed` → `open` ou `completed` → `in_progress` (une intervention terminée est définitivement scellée ; toute reprise de travail exige une nouvelle intervention ou une sous-intervention).
- `in_progress` → `open` (un travail engagé ne peut pas être rétrogradé en non commencé).

### 2.2 Sous-interventions

- Une sous-intervention a `parent_id` non nul.
- La profondeur d'arborescence est limitée à 1 niveau (un parent ne peut pas lui-même avoir un parent ; pas de sous-sous-interventions).
- Le `site_id` de la sous-intervention doit être identique à celui du parent.
- L'équipement de la sous-intervention doit être identique à celui du parent ou être un sous-équipement (enfant hiérarchique) de celui-ci (ou nul si `hors_equipement`).
- **Garde-fou de clôture** : une intervention parente ne peut être passée à `completed` tant qu'au moins une de ses sous-interventions actives n'est pas elle-même `completed` (réponse 409 Conflict).
- **Garde-fou de suppression** : la suppression logique du parent est refusée si des sous-interventions non supprimées existent (réponse 409 Conflict).

### 2.3 Effet sur le statut de l'équipement (Proposition et justification)

Le système maintient la synchronisation entre l'état machine des interventions (`ARRET`, `DEGRADE`, `FONCTIONNEL`) et le statut opérationnel de l'équipement défini en phase 2 (`operational`, `degraded`, `out_of_service`).

#### Règle retenue :

1. **À la création ou modification d'une intervention active (`open` ou `in_progress`)** :
   - Si `machine_state = 'ARRET'` : le statut de l'équipement bascule immédiatement à `out_of_service`.
   - Si `machine_state = 'DEGRADE'` : le statut de l'équipement bascule à `degraded`, sauf si l'équipement est déjà `out_of_service` (une indisponibilité totale prime sur une dégradation).
   - Si `machine_state = 'FONCTIONNEL'` : le statut de l'équipement reste inchangé.
   - Les interventions `hors_equipement` n'ont aucun effet (aucun équipement lié).

2. **À la clôture (`completed`) ou suppression logique (`deleted_at`) d'une intervention** :
   - Le service Rust exécute un recalcul fondé sur la criticité résiduelle parmi toutes les **autres interventions actives non supprimées** (`status IN ('open','in_progress') AND deleted_at IS NULL`) rattachées à cet équipement :
     - S'il reste au moins une intervention active avec `machine_state = 'ARRET'` → équipement maintenu à `out_of_service`.
     - Sinon, s'il reste au moins une intervention active avec `machine_state = 'DEGRADE'` → équipement basculé à `degraded`.
     - S'il ne reste aucune intervention active avec impact machine (ou uniquement `FONCTIONNEL`) : l'équipement repasse automatiquement à `operational`.
   - *Option de restitution manuelle à la clôture* : le technicien peut facultativement préciser un `final_equipment_status` (`operational`, `degraded`, `out_of_service`) dans le corps de la requête de clôture pour signaler explicitement que l'équipement reste en service partiel après réparation provisoire. En l'absence de ce paramètre, le recalcul automatique ci-dessus fait foi.

#### Justification :
- **Vérité terrain instantanée** : dès qu'un opérateur ou technicien déclare une panne avec arrêt de machine, le parc d'équipements et le tableau de bord de production reflètent immédiatement l'indisponibilité de l'équipement sans nécessiter une double saisie fastidieuse.
- **Résolution des interventions concurrentes** : dans les ateliers, plusieurs corps de métier (mécanique, automatisme, électricité) interviennent fréquemment en parallèle sur un équipement lourd. La clôture des travaux mécaniques ne doit en aucun cas remettre l'équipement « en service » si l'électricien consigne toujours la machine sous arrêt. Le calcul par sévérité résiduelle garantit la sécurité physique et la cohérence de l'outil de production.
- **Prise en compte des réparations provisoires** : le technicien sur place est le seul juge de l'état réel de restitution ; lui permettre de qualifier l'état final à la clôture évite d'avoir à créer immédiatement une intervention fictive pour marquer un fonctionnement dégradé.

## 3. Règles d'accès

| Ressource / Action | Permission requise | Rôles autorisés par défaut |
|---|---|---|
| Consulter les interventions (liste, détail, sous-interventions) | `interventions.view` | `admin`, `responsable`, `technicien`, `production` |
| Déclarer / créer une intervention | `interventions.create` | `admin`, `responsable`, `technicien`, `production` |
| Modifier, prendre en charge, clôturer, assigner des techniciens | `interventions.edit` | `admin`, `responsable`, `technicien` |
| Supprimer logiquement une intervention | `interventions.delete` | `admin`, `responsable` |

Règles de cloisonnement et gestion des droits :
- **Cloisonnement multi-sites** : un non-admin ne peut manipuler que les interventions de son `site_id`. Toute tentative d'accès à une intervention d'un autre site répond **404 Not Found** (pour ne pas révéler l'existence de la ressource).
- **Assignation de techniciens** : seuls des utilisateurs actifs du même `site_id` (ayant le rôle `technicien`, `responsable` ou `admin`) peuvent être assignés à l'intervention. L'assignation d'un utilisateur externe au site est rejetée (400 Bad Request).
- **Rôle Production** : les opérateurs de production disposent de `interventions.create` pour signaler les incidents, mais ne disposent pas de `interventions.edit` : ils ne peuvent ni modifier le compte-rendu technique, ni changer les états après création, ni clôturer l'intervention.
- **Journal d'audit** : chaque création, changement de statut, assignation et suppression écrit une ligne dans `audit_log` (`entity: 'intervention'`) dans la même transaction de base de données.

## 4. API HTTP

Listes paginées par curseur : `?limit=` (1 à 200, défaut 50), `?cursor=` (opaque, base64url via `Cursor::with_timestamp(id, created_at)`), tri par `created_at DESC` puis `id DESC`. Réponse standard `{ "items": [...], "next_cursor": "…" | null }`.

### Tableau des routes

| Méthode et route | Accès / Permissions | Paramètres / Filtres | Remarques et Effet |
|---|---|---|---|
| `GET /api/interventions` | `interventions.view` | `q`, `site_id`, `status`, `type`, `priority`, `machine_state`, `equipment_id`, `assigned_to`, `parent_id` (`root` pour principales), `date_from`, `date_to` | Liste paginée. Représentation avec équipement `{id, code, name}`, demandeur `{id, display_name}`, techniciens assignés, et `sub_interventions_count`. |
| `POST /api/interventions` | `interventions.create` | Corps JSON | Création (201). Statut initial `open` par défaut, ou `completed` si type `a_posteriori`. Met à jour le statut de l'équipement si nécessaire. |
| `GET /api/interventions/{id}` | `interventions.view` | Aucun | Détail complet avec historique, techniciens assignés, équipement, parent, et sous-interventions directes. |
| `PATCH /api/interventions/{id}` | `interventions.edit` | Corps JSON | Mise à jour partielle (titre, description, priorité, machine_state). Ne permet pas la transition vers `completed`. Recalcule le statut de l'équipement si `machine_state` change. |
| `POST /api/interventions/{id}/take` | `interventions.edit` | Corps JSON optionnel `{ "technician_ids": [...] }` | Prise en charge : transition `open` → `in_progress`. Si aucun technicien n'est fourni et que la liste actuelle est vide, assigne l'utilisateur courant comme lead. Enregistre `started_at = now()` si nul. |
| `POST /api/interventions/{id}/complete` | `interventions.edit` | Corps JSON `{ solution_report, duration_minutes, completed_at?, final_equipment_status? }` | Clôture : transition vers `completed`. Contrôle absence de sous-interventions ouvertes. Recalcule le statut de l'équipement. 200 avec intervention mise à jour. |
| `DELETE /api/interventions/{id}` | `interventions.delete` | Aucun | Suppression logique (204). Refus (409) si sous-interventions non supprimées existantes. Recalcule le statut de l'équipement. |
| `GET /api/interventions/{id}/sub-interventions` | `interventions.view` | Aucun | Liste des sous-interventions rattachées. |
| `POST /api/interventions/{id}/sub-interventions` | `interventions.edit` | Corps JSON | Crée une sous-intervention (201) rattachée à `{id}` (`parent_id = id`). Refusé si `{id}` a déjà un parent ou si `{id}` est `completed`. |
| `POST /api/interventions/{id}/technicians` | `interventions.edit` | `{ "user_id": "...", "is_lead": bool }` | Ajoute ou met à jour un technicien assigné. |
| `DELETE /api/interventions/{id}/technicians/{user_id}` | `interventions.edit` | Aucun | Retire l'assignation d'un technicien. |

### Schémas JSON

- Corps de création `POST /api/interventions` :
  ```json
  {
    "site_id": "018d9f52-...",
    "title": "Remplacement roulement convoyeur",
    "description": "Bruit anormal constaté sur le roulement de tête",
    "type": "corrective",
    "priority": "high",
    "machine_state": "ARRET",
    "equipment_id": "018d9f52-...",
    "technician_ids": ["018d9f52-..."]
  }
  ```
- Corps de clôture `POST /api/interventions/{id}/complete` :
  ```json
  {
    "solution_report": "Remplacement du roulement 6204-2RS effectué. Essais à vide satisfaisants.",
    "duration_minutes": 75,
    "final_equipment_status": "operational"
  }
  ```

## 5. IHM (`static/`)

`CURRENT_PHASE = 3`. Mêmes règles que les phases 1 et 2 : pas de framework, pas de script ou style en ligne, délégation `data-action` avec `jsArg()`, échappement HTML strict via `esc()`, support 375 px, thèmes clair et sombre respectés. Réutilisation des composants d'IHM factorisés en phase 2 (barre d'outils, modales, confirmations, badges).

### 5.1 Module Interventions dans le menu latéral
Visible pour tout utilisateur détenant la permission `interventions.view`.

### 5.2 Vue Liste (`/interventions`)
- **Barre d'outils** :
  - Filtres rapides en onglets ou boutons segmentés : « Toutes », « En cours / Ouvertes », « Mes interventions » (filtré sur l'utilisateur connecté), « Arrêt machine » (`machine_state = 'ARRET'`), « Critiques ».
  - Menu déroulant de filtres avancés : type d'intervention, équipement, priorité, période de date.
  - Champ de recherche textuelle avec délai de saisie (`q` sur code, titre, description).
- **Tableau dynamique** :
  - Colonnes : Référence/Code, Date de signalement, Titre & Équipement, Type, Priorité (badge couleur), État machine (badge rouge alerte si `ARRET`), Techniciens assignés (avatars ou initiales), Statut.
  - Badges de statut : `open` (bleu « Ouverte »), `in_progress` (orange « En cours »), `completed` (vert « Clôturée »).
  - Bouton « Charger plus » s'appuyant sur `next_cursor`.
- **Actions contextuelles** : Bouton principal « Nouvelle intervention » (si `interventions.create`).

### 5.3 Fiche de détail de l'intervention (`/interventions/{id}`)
- **En-tête de fiche** :
  - Code et Titre en grand, badges de statut, priorité et impact machine.
  - Bandeau d'action rapide pour le technicien :
    - Si `open` : Bouton « Prendre en charge » (action `take` en un clic).
    - Si `in_progress` : Bouton « Clôturer l'intervention » (ouvre la boîte de dialogue de clôture avec saisie obligatoire du compte-rendu et de la durée) + Bouton « M'assigner ».
    - Si `completed` : Bandeau informatif vert scellé avec date de clôture et temps passé.
- **Section Informations générales** :
  - Équipement rattaché (lien direct ouvrant la fiche de l'équipement) et statut actuel de l'équipement.
  - Demandeur et date de déclaration.
  - Dates de début et de fin réelles, durée comptabilisée en heures/minutes.
  - Description détaillée de la demande.
- **Section Compte-rendu technique** :
  - Affichage de la solution apportée et rapport de dépannage.
- **Section Techniciens** :
  - Liste des techniciens intervenants avec mention de l'intervenant principal (`lead`).
  - Boutons d'ajout et de retrait pour les utilisateurs avec `interventions.edit`.
- **Section Sous-interventions** :
  - Liste des sous-tâches avec statut et technicien.
  - Bouton « Créer une sous-tâche » ouvrant le formulaire de sous-intervention pré-rempli.

### 5.4 Formulaire de création / édition
- Choix du type : Corrective, Préventive, A posteriori, Hors équipement.
- Si « Hors équipement » : champ sélecteur d'équipement masqué et désactivé. Sinon : équipement obligatoire avec recherche par code ou désignation.
- État machine : sélecteur avec mise en garde visuelle claire si « Arrêt machine » est sélectionné (information sur le passage de la machine en indisponible).
- Si type « A posteriori » : champs compte-rendu, date de début, date de fin et durée affichés directement dans le formulaire pour clôture immédiate à la création.

## 6. Tests boîte noire (`crates/apex-server/tests/`)

Tests d'intégration couvrant le binaire en mode portable sur port dynamique :

1. **Cycle de vie standard** :
   - Création d'une intervention `open` avec équipement et priorité `high`.
   - Prise en charge (`POST /take`) : passe à `in_progress`, affecte le technicien et renseigne `started_at`.
   - Clôture (`POST /complete`) avec compte-rendu et durée de 45 minutes : passe à `completed`, renseigne `completed_at`.
2. **Clôture directe / A posteriori** :
   - Création directe d'une intervention `a_posteriori` avec compte-rendu et durée : passe directement à `completed`.
3. **Refus des transitions illégales (409 Conflict)** :
   - Tentative de réouverture d'une intervention `completed` vers `open` ou `in_progress` → 409.
   - Tentative de retour arrière d'une intervention `in_progress` vers `open` → 409.
4. **Contrainte CHECK sur `equipment_id` (défaut I1)** :
   - Création d'une intervention corrective sans `equipment_id` → rejetée (400 Bad Request).
   - Création d'une intervention `hors_equipement` avec `equipment_id` → rejetée (400 Bad Request).
   - Création d'une intervention `hors_equipement` sans `equipment_id` → acceptée (201 Created).
5. **Vérification de l'effet sur le statut de l'équipement** :
   - Équipement initialement `operational`.
   - Création d'une intervention corrective avec `machine_state = 'ARRET'` → l'équipement passe à `out_of_service`.
   - Clôture de l'intervention sans autre intervention en cours → l'équipement repasse à `operational`.
   - Deux interventions simultanées à l'arrêt sur le même équipement : la clôture de la première laisse l'équipement `out_of_service` tant que la seconde est ouverte.
6. **Sous-interventions et blocage de clôture** :
   - Création d'une intervention parent et d'une sous-intervention.
   - Tentative de clôturer le parent alors que la sous-intervention est `open` → refusé (409 Conflict).
   - Clôture de la sous-intervention, puis clôture réussie du parent.
   - Tentative de créer une sous-intervention sur une sous-intervention (profondeur > 1) → rejeté (400 Bad Request).
7. **Gestion des techniciens assignés** :
   - Assignation d'un technicien du même site → acceptée.
   - Tentative d'assigner un utilisateur d'un autre site → rejetée (400 Bad Request).
8. **Isolation inter-sites et permissions RBAC** :
   - Un utilisateur du site A tentant de lire ou modifier une intervention du site B reçoit 404 Not Found.
   - Un utilisateur avec rôle `production` peut créer une intervention mais reçoit 403 Forbidden s'il tente de la prendre en charge (`/take`), de la modifier (`PATCH`) ou de la supprimer (`DELETE`).
9. **Pagination par curseur et filtres** :
   - Création de 5 interventions : parcours avec `limit=2` vérifiant l'exhaustivité des données sur 3 requêtes successives sans omission ni doublon.
   - Filtre par `status=open` et filtre combiné par `equipment_id`.

## 7. Décisions du 2026-10-18 (prioritaires sur le reste du document)

1. **Annulation** : ajouter le statut `cancelled` (`CHECK (status IN ('open','in_progress','completed','cancelled'))`)
   et les colonnes `cancelled_at TEXT NULL`, `cancelled_by_id TEXT NULL REFERENCES users(id)`,
   `cancel_reason TEXT NULL` avec `CHECK (status <> 'cancelled' OR (cancel_reason IS NOT NULL AND
   length(trim(cancel_reason)) > 0))`. Transitions autorisées : `open → cancelled` et
   `in_progress → cancelled` ; `completed` et `cancelled` sont terminaux (toute autre transition : 409).
   Route `POST /api/interventions/{id}/cancel` `{ reason }` (permission `interventions.edit`).
   L'annulation recalcule le statut de l'équipement comme une clôture, libère les réservations de
   pièces (phase 4) et est refusée si des sous-interventions sont encore actives. Une intervention
   annulée reste visible (filtre de statut, historique, statistiques) ; la suppression logique est
   réservée aux erreurs de saisie. IHM : bouton « Annuler l'intervention » avec motif obligatoire,
   badge « Annulée ».
2. **Pas de validation par la production** : la clôture par le technicien est définitive. Ne pas
   prévoir de statut `pending_validation`.
3. **Temps passé par technicien** : ajouter à `intervention_technicians` la colonne
   `duration_minutes INTEGER NULL CHECK (duration_minutes IS NULL OR duration_minutes >= 0)`.
   La durée de l'intervention (`interventions.duration_minutes`) devient la **somme** des temps
   des techniciens (calculée par le service à chaque modification, jamais saisie directement).
   Route `PUT /api/interventions/{id}/technicians/{user_id}/time` `{ duration_minutes }` :
   un technicien saisit son propre temps (`interventions.edit`) ; le responsable et l'admin
   peuvent saisir celui de tout technicien assigné. La clôture exige qu'au moins un technicien
   assigné ait un temps renseigné ; `duration_minutes` disparaît du corps de
   `POST /api/interventions/{id}/complete`. IHM : tableau des intervenants avec leur temps sur la
   fiche de détail, total affiché. Tests : somme recalculée, un technicien ne peut pas saisir le
   temps d'un autre (403), clôture refusée sans temps (400).
