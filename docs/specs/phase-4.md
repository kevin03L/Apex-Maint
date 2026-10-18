# Phase 4 — Pièces & stock

Spécification de travail de la phase 4 (voir `docs/PLAN.md` §1.1, §1.3 I2, §2.3, §3 et §4). Elle s'appuie sur la phase 1 (`docs/specs/phase-1.md` : sessions, `Actor`, permissions, audit dans la même transaction), la phase 2 (`docs/specs/phase-2.md` : emplacements hiérarchiques, fournisseurs, équipements) et la phase 3 (`docs/specs/phase-3.md` : interventions).

## Résultat attendu

Catalogue complet de pièces détachées par site, gestion du stock multi-emplacements sous transaction stricte `BEGIN IMMEDIATE`, traçabilité absolue des flux (entrées, sorties, transferts), obligation stricte d'intervention pour toute sortie de stock (défaut I2 de l'ancien audit résolu), consommation de pièces directement intégrée à la fiche intervention, association de pièces aux équipements, alertes de stock bas dynamiques, et garantie mathématique d'absence de stock négatif même sous forte concurrence.

## 1. Base de données — migration `0005_stock.sql`

Conventions communes : `id TEXT PRIMARY KEY` (UUID v7), `site_id TEXT NOT NULL REFERENCES sites(id)`, `created_at`, `updated_at`, `deleted_at TEXT NULL`, tables `STRICT`, index composites commençant par `site_id`. Référence de pièce `reference` unique par site parmi les non supprimées (`CREATE UNIQUE INDEX … (site_id, reference COLLATE NOCASE) WHERE deleted_at IS NULL`).

- `parts` (catalogue de pièces détachées) :
  - `id TEXT PRIMARY KEY NOT NULL`
  - `site_id TEXT NOT NULL REFERENCES sites(id)`
  - `reference TEXT NOT NULL` (référence interne de la pièce, ex. `ROUL-6204-2RS`)
  - `name TEXT NOT NULL` (désignation de la pièce)
  - `description TEXT`
  - `category TEXT` (ex. `roulement`, `visserie`, `moteur`, `pneumatique`, `hydraulique`, `electrique`, `capteur`)
  - `unit TEXT NOT NULL DEFAULT 'piece' CHECK (unit IN ('piece','metre','litre','kg','lot','boite'))`
  - `unit_price REAL NOT NULL DEFAULT 0.0 CHECK (unit_price >= 0.0)` (prix unitaire moyen pondéré ou dernier prix d'achat)
  - `supplier_id TEXT REFERENCES suppliers(id)` (fournisseur principal)
  - `supplier_reference TEXT` (référence chez le fournisseur)
  - `manufacturer TEXT`
  - `manufacturer_reference TEXT`
  - `barcode TEXT` (code-barres / EAN13 ou QR code pour scan)
  - `alert_threshold INTEGER CHECK (alert_threshold IS NULL OR alert_threshold >= 0)` (seuil d'alerte spécifique ; si `NULL`, utilise le seuil global de `app_settings`)
  - `created_at TEXT NOT NULL`
  - `updated_at TEXT NOT NULL`
  - `deleted_at TEXT`
  - Index :
    - `(site_id, reference COLLATE NOCASE) WHERE deleted_at IS NULL` (unique)
    - `(site_id, supplier_id, deleted_at)`
    - `(site_id, category, deleted_at)`

- `stock` (niveaux de stock par emplacement physique) :
  - `id TEXT PRIMARY KEY NOT NULL`
  - `site_id TEXT NOT NULL REFERENCES sites(id)`
  - `part_id TEXT NOT NULL REFERENCES parts(id)`
  - `location_id TEXT NOT NULL REFERENCES locations(id)`
  - `quantity INTEGER NOT NULL DEFAULT 0 CHECK (quantity >= 0)` (**règle impérative : aucun stock négatif au niveau SQL**)
  - `updated_at TEXT NOT NULL`
  - Contrainte d'unicité et index :
    - `UNIQUE (part_id, location_id)`
    - `(site_id, part_id)`
    - `(site_id, location_id)`

- `stock_movements` (registre d'audit et de traçabilité des flux) :
  - `id TEXT PRIMARY KEY NOT NULL`
  - `site_id TEXT NOT NULL REFERENCES sites(id)`
  - `part_id TEXT NOT NULL REFERENCES parts(id)`
  - `movement_type TEXT NOT NULL CHECK (movement_type IN ('in','out','transfer'))`
  - `quantity INTEGER NOT NULL CHECK (quantity > 0)`
  - `from_location_id TEXT REFERENCES locations(id)`
  - `to_location_id TEXT REFERENCES locations(id)`
  - `intervention_id TEXT REFERENCES interventions(id)`
  - `user_id TEXT NOT NULL REFERENCES users(id)` (auteur du mouvement)
  - `unit_price REAL CHECK (unit_price IS NULL OR unit_price >= 0.0)`
  - `reason TEXT` (commentaire, numéro de bon de livraison ou motif d'ajustement)
  - `created_at TEXT NOT NULL`
  - **Contraintes CHECK d'intégrité métier (défaut I2 de l'ancien audit)** :
    - `CHECK (movement_type != 'out' OR intervention_id IS NOT NULL)` (**une sortie exige obligatoirement une intervention**)
    - `CHECK (movement_type != 'in' OR (from_location_id IS NULL AND to_location_id IS NOT NULL))` (une entrée a obligatoirement une destination et aucune source)
    - `CHECK (movement_type != 'out' OR (from_location_id IS NOT NULL AND to_location_id IS NULL))` (une sortie a obligatoirement une source et aucune destination)
    - `CHECK (movement_type != 'transfer' OR (from_location_id IS NOT NULL AND to_location_id IS NOT NULL AND from_location_id != to_location_id))` (un transfert a une source et une destination distinctes)
  - Index :
    - `(site_id, part_id, created_at DESC)`
    - `(intervention_id) WHERE intervention_id IS NOT NULL`
    - `(site_id, created_at DESC)`

- `intervention_parts` (pièces consommées lors d'une intervention) :
  - `id TEXT PRIMARY KEY NOT NULL`
  - `site_id TEXT NOT NULL REFERENCES sites(id)`
  - `intervention_id TEXT NOT NULL REFERENCES interventions(id)`
  - `part_id TEXT NOT NULL REFERENCES parts(id)`
  - `location_id TEXT NOT NULL REFERENCES locations(id)`
  - `quantity INTEGER NOT NULL CHECK (quantity > 0)`
  - `unit_price REAL NOT NULL CHECK (unit_price >= 0.0)` (prix figé au moment de la consommation)
  - `movement_id TEXT NOT NULL REFERENCES stock_movements(id)`
  - `created_at TEXT NOT NULL`
  - Index :
    - `(intervention_id)`
    - `(part_id)`

- `equipment_parts` (nomenclature / pièces recommandées pour un équipement) :
  - `equipment_id TEXT NOT NULL REFERENCES equipments(id)`
  - `part_id TEXT NOT NULL REFERENCES parts(id)`
  - `quantity_recommended INTEGER NOT NULL DEFAULT 1 CHECK (quantity_recommended > 0)`
  - `notes TEXT`
  - `PRIMARY KEY (equipment_id, part_id)`
  - Index `(part_id)`

## 2. Service transactionnel et concurrence (`BEGIN IMMEDIATE`)

### 2.1 Concurrence SQLite et atomicité

En SQLite WAL, plusieurs connexions peuvent lire simultanément, mais un seul rédacteur peut modifier la base. Une transaction ouverte par défaut (`BEGIN DEFERRED`) n'acquiert le verrou d'écriture qu'au moment de la première instruction d'écriture. Si deux transactions concurrentes lisent un stock puis tentent d'écrire en même temps, l'une d'elles échoue avec l'erreur `SQLITE_BUSY`.

**Règle impérative** : Tout traitement de mouvement de stock dans `apex-core/src/services/stock.rs` démarre obligatoirement par une transaction **`BEGIN IMMEDIATE`** :
- Le verrou d'écriture réservé est acquis immédiatement avant toute lecture de vérification.
- Les requêtes concurrentes de modification de stock attendent la libération du verrou (dans la limite du `busy_timeout` configuré).
- Les décréments de stock se font avec une clause stricte `UPDATE stock SET quantity = quantity - ? WHERE part_id = ? AND location_id = ? AND quantity >= ?` : si aucune ligne n'est modifiée, la transaction échoue immédiatement avec une erreur explicite **HTTP 409 Conflict** (`StockInsuffisant`).

### 2.2 Traçabilité stricte intervention-stock (résolution du défaut I2)

Lorsqu'un technicien enregistre la consommation d'une pièce sur une intervention :
1. Vérification que l'intervention existe sur le site de l'acteur et n'est pas clôturée (`status IN ('open','in_progress')`).
2. Vérification que la pièce et l'emplacement appartiennent au même site et ne sont pas supprimés.
3. Décrément atomique de la quantité dans `stock` (avec blocage si stock insuffisant).
4. Création du mouvement de type `out` dans `stock_movements`, rattaché obligatoirement à l'identifiant de l'intervention (`intervention_id`), à l'emplacement source et à l'utilisateur.
5. Création de la ligne dans `intervention_parts` associant la pièce consommée, la quantité, le prix unitaire actuel et la référence du mouvement `movement_id`.
6. Écriture dans `audit_log` (`entity: 'stock_movement'`).

**Toutes ces étapes sont exécutées au sein de la même transaction `BEGIN IMMEDIATE`**. En cas d'erreur à n'importe quelle étape, l'ensemble est annulé (`ROLLBACK`). Il est rigoureusement impossible d'avoir une consommation sans mouvement de stock ou une sortie sans intervention.

### 2.3 Mouvements d'entrée et de transfert

- **Entrée de stock (`in`)** :
  - Incrémente la quantité dans `stock` pour l'emplacement cible via `INSERT INTO stock (id, site_id, part_id, location_id, quantity, updated_at) VALUES (?, ?, ?, ?, ?, ?) ON CONFLICT (part_id, location_id) DO UPDATE SET quantity = quantity + excluded.quantity, updated_at = excluded.updated_at`.
  - Crée le mouvement `in` dans `stock_movements` avec `reason` et l'auteur.
- **Transfert de stock (`transfer`)** :
  - Décrémente l'emplacement source `from_location_id` sous contrainte `quantity >= ?`.
  - Incrémente l'emplacement cible `to_location_id`.
  - Crée le mouvement `transfer` dans `stock_movements`.

### 2.4 Alertes de stock bas

Le niveau d'alerte pour une pièce est évalué dynamiquement :
- **Stock total de la pièce** : somme des quantités de la pièce sur tous les emplacements du site (`SELECT COALESCE(SUM(quantity), 0) FROM stock WHERE part_id = ?`).
- **Seuil d'alerte effectif** :
  - Si `parts.alert_threshold` n'est pas nul : `parts.alert_threshold`.
  - Sinon : valeur extraite de `app_settings` pour la clé `stock_alert_threshold` (défaut : 5).
- **Condition d'alerte** : `total_stock <= effective_threshold`.

## 3. Règles d'accès

| Ressource / Action | Permission requise | Rôles autorisés par défaut |
|---|---|---|
| Consulter le catalogue des pièces, pièces des équipements | `parts.view` | `admin`, `responsable`, `technicien` |
| Créer une pièce au catalogue | `parts.create` | `admin`, `responsable` |
| Modifier une pièce au catalogue | `parts.edit` | `admin`, `responsable` |
| Supprimer logiquement une pièce | `parts.delete` | `admin`, `responsable` |
| Consulter les stocks, alertes et journal des mouvements | `stock.view` | `admin`, `responsable`, `technicien` |
| Enregistrer des mouvements (entrées, transferts, sorties sur intervention) | `stock.movements` | `admin`, `responsable`, `technicien` |

Règles de cloisonnement et garde-fous :
- **Multi-sites** : un non-admin ne peut voir ni agir sur les pièces et stocks d'un autre site. Toute ressource d'un autre site renvoie **HTTP 404**.
- **Suppression logique d'une pièce** : refusée (HTTP 409 Conflict) si le stock total de la pièce sur le site est strictement supérieur à 0 (`total_stock > 0`). Le stock physique résiduel doit d'abord être évacué.
- **Suppression d'emplacement** : la suppression logique d'un emplacement (définie en phase 2) est refusée s'il contient des pièces en stock (`quantity > 0`).

## 4. API HTTP

Listes paginées par curseur : `?limit=` (1 à 200, défaut 50), `?cursor=` (opaque). Tri des pièces par `reference ASC` puis `id ASC`. Tri des mouvements par `created_at DESC` puis `id DESC`.

### Tableau des routes

| Méthode et route | Accès / Permissions | Paramètres / Filtres | Remarques et Effet |
|---|---|---|---|
| `GET /api/parts` | `parts.view` | `q`, `site_id`, `category`, `supplier_id`, `low_stock` (booléen) | Liste paginée. Représentation avec `total_stock`, `effective_threshold`, `is_low_stock`, et le détail par emplacement. |
| `POST /api/parts` | `parts.create` | Corps JSON | Création d'une pièce (201). Référence unique par site. |
| `GET /api/parts/{id}` | `parts.view` | Aucun | Fiche complète de la pièce, détail des stocks par emplacement, équipements rattachés. |
| `PATCH /api/parts/{id}` | `parts.edit` | Corps JSON | Modification partielle (nom, prix, seuil, fournisseur, etc.). |
| `DELETE /api/parts/{id}` | `parts.delete` | Aucun | Suppression logique (204). Refus 409 si stock total > 0. |
| `GET /api/stock` | `stock.view` | `part_id`, `location_id`, `site_id` | Niveaux de stock par emplacement. |
| `GET /api/stock/alerts` | `stock.view` | `site_id` | Liste des pièces dont le stock est inférieur ou égal à leur seuil d'alerte. |
| `GET /api/stock/movements` | `stock.view` | `q`, `part_id`, `movement_type`, `intervention_id`, `location_id`, `user_id`, `date_from`, `date_to`, `site_id` | Journal complet des mouvements de stock, paginé par curseur. |
| `POST /api/stock/movements` | `stock.movements` | Corps JSON | Mouvement manuel d'entrée (`in`) ou de transfert (`transfer`). Les sorties directes hors intervention sont refusées (400). Transaction `BEGIN IMMEDIATE`. |
| `GET /api/interventions/{id}/parts` | `interventions.view` + `stock.view` | Aucun | Liste des pièces consommées sur l'intervention avec quantités, prix et emplacements d'origine. |
| `POST /api/interventions/{id}/parts` | `interventions.edit` + `stock.movements` | `{ part_id, location_id, quantity }` | Déclare la consommation d'une pièce : sortie de stock atomique en transaction `BEGIN IMMEDIATE` (201). |
| `DELETE /api/interventions/{id}/parts/{item_id}` | `interventions.edit` + `stock.movements` | Aucun | Annulation d'une consommation : réintègre le stock sur l'emplacement d'origine via mouvement tracé et supprime la ligne (204). |
| `GET /api/equipments/{id}/parts` | `equipments.view` + `parts.view` | Aucun | Liste des pièces recommandées associées à l'équipement. |
| `POST /api/equipments/{id}/parts` | `equipments.edit` + `parts.edit` | `{ part_id, quantity_recommended, notes }` | Associe une pièce à un équipement (201). |
| `DELETE /api/equipments/{id}/parts/{part_id}` | `equipments.edit` + `parts.edit` | Aucun | Dissocie une pièce de l'équipement (204). |

### Schémas JSON

- Corps d'entrée de stock `POST /api/stock/movements` :
  ```json
  {
    "part_id": "018d9f52-...",
    "movement_type": "in",
    "to_location_id": "018d9f52-...",
    "quantity": 10,
    "unit_price": 14.50,
    "reason": "Réception commande fournisseur BL-4892"
  }
  ```
- Corps de consommation sur intervention `POST /api/interventions/{id}/parts` :
  ```json
  {
    "part_id": "018d9f52-...",
    "location_id": "018d9f52-...",
    "quantity": 2
  }
  ```

## 5. IHM (`static/`)

`CURRENT_PHASE = 4`. Respect strict des consignes du socle : pas de script ni de style en ligne, délégation `data-action` avec `jsArg()`, `esc()` systématique, réutilisation des composants factorisés en phase 2 (barre d'outils, modales, confirmations, badges).

### 5.1 Module Magasin & Pièces dans le menu latéral
Visible pour tout utilisateur détenant la permission `parts.view` ou `stock.view`.

### 5.2 Vue Catalogue des pièces (`/pieces`)
- **Barre d'outils** :
  - Filtre rapide « Alertes stock bas » avec badge numérique affichant le nombre de références critiques.
  - Filtres par catégorie et par fournisseur.
  - Champ de recherche textuelle avec délai de saisie (`q` sur référence, désignation, code-barres).
- **Tableau des pièces** :
  - Colonnes : Référence interne, Désignation, Catégorie, Fournisseur, Stock disponible (badge vert si OK, badge orange/rouge « Stock bas » si `is_low_stock`), Prix unitaire, Actions.
  - Bouton « Charger plus » s'appuyant sur `next_cursor`.
  - Boutons d'action : « Nouvelle pièce » (si `parts.create`), « Réception / Entrée » (si `stock.movements`), « Transfert » (si `stock.movements`).

### 5.3 Fiche de détail d'une pièce (`/pieces/{id}`)
- **En-tête** : Référence, désignation, badge d'alerte si applicable.
- **Niveaux de stock par emplacement** : Tableau ventilant les quantités par emplacement physique (ex. Magasin Principal > Allée B > Rack 02 > Bac 14) avec bouton direct pour réapprovisionner ou transférer.
- **Journal des mouvements récents** : Historique des 20 derniers mouvements (date, type `in`/`out`/`transfer`, quantité, intervenant, intervention liée).
- **Équipements associés** : Liste des équipements sur lesquels cette pièce est montée ou préconisée.

### 5.4 Intégration dans la fiche Intervention (Parcours technicien)
- **Section « Pièces consommées »** :
  - Tableau des pièces sorties pour l'intervention : référence, désignation, emplacement de prélèvement, quantité, coût unitaire, coût total de la ligne.
  - Total cumulé des coûts de pièces pour l'intervention.
- **Boîte de dialogue « Sortie de pièce »** (bouton accessible si `stock.movements` et intervention non clôturée) :
  - Si l'intervention est liée à un équipement : **mise en avant immédiate des pièces recommandées de cet équipement (`equipment_parts`) en tête de liste** pour un gain de temps maximal en atelier.
  - Champ de recherche rapide de pièce avec complétion automatique.
  - Sélecteur de l'emplacement de prélèvement affichant le stock réel disponible dans chaque bac.
  - Saisie de la quantité avec contrôle de saisie instantané : si la quantité dépasse le stock disponible sur l'emplacement sélectionné, le bouton de validation est désactivé et un message d'alerte explicite s'affiche.

### 5.5 Intégration dans la fiche Équipement
- Onglet « Pièces détachées » sur la fiche d'un équipement : tableau des pièces préconisées avec quantité recommandée, notes d'assemblage, stock actuellement disponible au magasin, et bouton pour associer/dissocier une référence.

## 6. Tests boîte noire (`crates/apex-server/tests/`)

Tests d'intégration couvrant le binaire sur port dynamique :

1. **CRUD et catalogue de pièces** :
   - Création d'une pièce avec référence, catégorie, prix et seuil d'alerte.
   - Refus de doublon de référence sur le même site (409 Conflict).
   - Modification partielle via `PATCH`.
   - Tentative de suppression d'une pièce avec stock > 0 : rejetée (409 Conflict).
2. **Entrée de stock (`in`)** :
   - Entrée de 10 unités dans un bac : vérification de la création de la ligne `stock` et de l'enregistrement dans `stock_movements`.
   - Seconde entrée de 5 unités dans le même bac : vérification que le stock passe à 15 unités.
3. **Transfert d'emplacement (`transfer`)** :
   - Transfert de 4 unités du bac A vers le bac B : stock A = 11, stock B = 4.
   - Tentative de transférer 20 unités depuis le bac A : rejetée (409 Conflict, stock insuffisant), stocks inchangés.
4. **Sortie sur intervention (résolution défaut I2)** :
   - Tentative de mouvement direct `out` via `/api/stock/movements` sans `intervention_id` : rejetée par la contrainte CHECK (400 Bad Request).
   - Sortie via `POST /api/interventions/{id}/parts` de 3 unités : décrément de stock (11 → 8), création simultanée du mouvement `out` et de la ligne `intervention_parts`.
   - Tentative de prélever sur une intervention clôturée (`completed`) : rejetée (409 Conflict).
5. **Test de concurrence dédié : AUCUN STOCK NÉGATIF** :
   - Création d'une pièce avec un stock initial de **5 unités** dans un emplacement donné.
   - Lancement simultané de **10 requêtes concurrentes** (via `tokio::spawn`), chacune tentant de consommer **2 unités** de cette pièce sur 10 interventions distinctes.
   - Constat : exactement 2 requêtes réussissent (4 unités consommées au total), ou les requêtes valides dans la limite des 5 unités disponibles sont traitées ; toutes les autres requêtes reçoivent **409 Conflict**.
   - **Vérification finale absolue en base** : le stock restant est exactement égal à 1 (5 - 4 = 1), `quantity >= 0` est scrupuleusement respecté, et le nombre de mouvements `out` en base correspond exactement aux unités consommées.
6. **Alertes de stock bas** :
   - Pièce A avec seuil spécifique à 4, stock à 3 → signalée en alerte (`is_low_stock = true`).
   - Pièce B sans seuil spécifique (`NULL`), seuil global `app_settings` à 5, stock à 6 → pas d'alerte ; sortie de 2 unités (stock passe à 4) → passe en alerte.
   - Vérification de la route `GET /api/stock/alerts`.
7. **Pièces associées aux équipements (`equipment_parts`)** :
   - Rattachement d'une pièce à un équipement, consultation via `/api/equipments/{id}/parts`, puis dissociation.
8. **Isolation inter-sites et permissions RBAC** :
   - Tentative de consommer une pièce du site B sur une intervention du site A : rejetée (404 Not Found).
   - Utilisateur de rôle `production` (sans `stock.movements`) : tentative d'entrée ou de sortie de stock rejetée (403 Forbidden).

## 7. Décisions du 2026-10-18 (prioritaires sur le reste du document)

0. **Montants en centimes** : tous les prix sont stockés en `INTEGER` de centimes d'euro
   (`unit_price_cents`, `CHECK (… >= 0)`), jamais en `REAL` ; l'API expose des centimes, l'IHM
   affiche des euros formatés en `fr-FR`.
1. **Valorisation au prix moyen pondéré (PMP)** : `parts.unit_price_cents` est le PMP de la pièce,
   tous emplacements du site confondus. À chaque entrée (`in`) avec un prix d'achat `p` pour une
   quantité `q`, dans la transaction `BEGIN IMMEDIATE` du mouvement :
   `PMP = arrondi((stock_total_avant × PMP_avant + q × p) / (stock_total_avant + q))`
   (arrondi au centime le plus proche ; si `stock_total_avant = 0`, `PMP = p`). Le prix d'achat est
   obligatoire sur une entrée. Les sorties, transferts et ajustements ne modifient pas le PMP ; les
   sorties figent le PMP courant dans `intervention_parts.unit_price_cents` et
   `stock_movements.unit_price_cents`. Tests : PMP après plusieurs entrées à prix différents,
   inchangé après une sortie.
2. **Ajustements d'inventaire et rebuts** : ajouter `movement_type = 'adjustment'` avec
   `reason TEXT NULL` et `adjustment_kind TEXT NULL CHECK (adjustment_kind IN
   ('inventaire','rebut','perte','casse'))` ; `CHECK (movement_type <> 'adjustment' OR
   (reason IS NOT NULL AND adjustment_kind IS NOT NULL))`. Un ajustement porte sur un seul
   emplacement, en plus ou en moins (colonne `delta INTEGER` non nul pour ce type ; la quantité
   résultante reste `>= 0`), sans intervention. Réservé au responsable et à l'admin (nouvelle
   permission `stock.adjust`, ajoutée à `Permission` et à la matrice par défaut du responsable).
   Route `POST /api/stock/adjustments`. Audit dans la même transaction.
3. **Réservation de pièces préparées** : une pièce peut être réservée **uniquement pour une
   intervention active** (`open` ou `in_progress`) et **seulement lorsqu'elle est déclarée
   préparée** (sortie du rayon et mise de côté pour l'intervention). Table
   `stock_reservations` : `id`, `site_id`, `intervention_id NOT NULL`, `part_id NOT NULL`,
   `location_id NOT NULL`, `quantity INTEGER NOT NULL CHECK (quantity > 0)`,
   `status TEXT NOT NULL CHECK (status IN ('prepared','consumed','released'))`,
   `prepared_by_id`, `prepared_at`, `closed_at TEXT NULL`, `created_at`, `updated_at`.
   - Disponible = `stock.quantity − somme des réservations 'prepared'` de l'emplacement ; une
     préparation est refusée (409) si le disponible est insuffisant ; une sortie ordinaire ne
     peut pas consommer une quantité réservée pour une autre intervention.
   - Consommer sur l'intervention transforme la réservation en sortie (`out`), passe la
     réservation à `consumed`, dans la même transaction `BEGIN IMMEDIATE`.
   - Clôture ou annulation de l'intervention : les réservations encore `prepared` passent à
     `released` (pièces remises en rayon) ; l'IHM signale les pièces préparées non consommées
     avant la clôture.
   - Routes : `POST /api/interventions/{id}/reservations` `{ part_id, location_id, quantity }`
     (permission `stock.movements`), `POST /api/reservations/{id}/consume` `{ quantity? }`
     (consommation partielle possible : le reste est libéré), `POST /api/reservations/{id}/release`.
   - IHM : sur la fiche d'intervention, section « Pièces préparées » ; dans le stock, colonnes
     « En stock », « Réservé », « Disponible ».
   - Tests : réservation refusée sans intervention active ; disponible correct ; concurrence (deux
     préparations simultanées sur le dernier article : une seule réussit) ; libération à la
     clôture et à l'annulation.
