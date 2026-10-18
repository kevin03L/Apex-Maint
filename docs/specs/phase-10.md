# Phase 10 — Gamme Apex : liaison avec Apex Supervision

Spécification de travail de la phase 10 (voir `docs/PLAN.md` §4 et `docs/specs/decisions.md`, qui
prime). Elle définit **le côté Apex Maint** de la liaison et le **contrat d'API** que doit respecter
Apex Supervision. Les modifications d'Apex Supervision font l'objet d'un travail séparé, dans son
propre dépôt.

## Résultat attendu

- **Liaison facultative**, désactivée par défaut et activée dans les Paramètres.
- **Authentification de Supervision** par une **clé de service** rattachée à un site, révocable et
  stockée hachée. Les bases restent séparées.
- **Deux types d'événements**, chacun activable séparément :
  - **compteurs d'heures** (et autres compteurs) : ils alimentent les compteurs d'équipement, donc
    les plannings de type `counter` (phase 5) ;
  - **état de l'équipement** : il est **affiché à titre d'information seulement**, sur la fiche et
    dans les listes. Il **ne modifie pas** le statut de l'équipement dans Maint et ne crée pas
    d'intervention.
- **Pas de sens Maint → Supervision** pour l'instant.
- Un **journal des échanges** pour diagnostiquer la liaison.

## 1. Base de données — migration `0010_integration.sql`

### `integration_keys`

| Colonne | Type et contrainte |
|---|---|
| `id` | |
| `site_id` | `NOT NULL` |
| `label` | ex. « Supervision atelier A » |
| `key_prefix` | les 8 premiers caractères, affichés pour reconnaître la clé |
| `key_hash` | `TEXT NOT NULL UNIQUE` : SHA-256 |
| `scopes` | JSON : `["counters", "status"]` |
| `created_by_id` | |
| `created_at` | |
| `last_used_at` | |
| `last_used_ip` | |
| `revoked_at` | `NULL` |
| `expires_at` | `NULL` |

### `equipment_counters`

Ajout de la colonne `external_ref TEXT NULL` : l'identifiant de la mesure côté Supervision (par
exemple le repère de la variable). Elle est unique par site parmi les compteurs non supprimés.

### `equipments`

Ajout de la colonne `external_ref TEXT NULL`. Elle est facultative : à défaut, on rapproche par le
`code` de l'équipement.

### `equipment_external_status` — dernier état connu

| Colonne | Type et contrainte |
|---|---|
| `equipment_id` | clé primaire |
| `site_id` | |
| `source` | `'supervision'` |
| `state` | `TEXT NOT NULL CHECK (state IN ('running','stopped','degraded','alarm','unknown'))` |
| `since` | |
| `message` | `NULL` |
| `received_at` | |

### `equipment_external_status_history`

Historique des changements d'état, conservé 90 jours (réglable).

### `integration_events` — journal des échanges

| Colonne | Contenu |
|---|---|
| `id` | |
| `site_id` | |
| `key_id` | |
| `kind` | `counters` ou `status` |
| `received_at` | |
| `items_count` | |
| `accepted` | |
| `rejected` | |
| `errors` | JSON, les 50 premières erreurs |
| `duration_ms` | |

Conservé 30 jours.

### `app_settings`

| Clé | Valeur par défaut |
|---|---|
| `integration.supervision.enabled` | `false` |
| `integration.supervision.counters_enabled` | `true` |
| `integration.supervision.status_enabled` | `true` |
| `integration.supervision.counter_min_interval_min` | 15 |
| `integration.supervision.status_stale_after_min` | 30 |

## 2. Règles

### 2.1 Clés de service

- **Création** : Paramètres > Intégrations (`settings.edit`).
  - La clé a la forme `apx_` suivi de 43 caractères base64url, soit 256 bits.
  - Elle est **affichée une seule fois** à la création, avec un bouton « Copier ».
  - Seuls son préfixe et son hachage sont conservés.
- **Choix à la création** : site, portée (`counters`, `status`), expiration facultative.
- **Gestion** : la clé peut être révoquée ou renouvelée (création d'une nouvelle clé, puis
  révocation de l'ancienne).
- **Audit** : création, révocation et changement de portée.
- **Utilisation** : en-tête `Authorization: Bearer apx_…`, avec une comparaison à temps constant
  sur le hachage.
  - Une clé n'ouvre **que** les routes `/api/integrations/supervision/*` de sa portée, et **jamais**
    une session ni une autre route.
  - Pas de cookie, donc **pas de contrôle CSRF** sur ces routes. En revanche, le garde refuse toute
    requête vers ces routes qui porte un cookie de session sans clé.
  - Le garde (refus par défaut) traite ces routes à part : elles sont publiques au sens de la
    session, mais exigent une clé valide. Sinon, la réponse est 401, sans détail.
- **Limitation de débit** : 120 requêtes par minute par clé, au-delà réponse 429 avec
  `Retry-After`. Corps limité à 1 Mo, 1 000 éléments au plus par requête.
- **Liaison désactivée** dans les Paramètres : toutes ces routes répondent 404.

### 2.2 Compteurs

- **Route** : `POST /api/integrations/supervision/counters`, avec le corps
  `{ "readings": [{ "counter_ref": "…", "value": 12345, "read_at": "RFC 3339", "reset": false }] }`.
- **Rapprochement** : par `equipment_counters.external_ref`, **dans le site de la clé**. Il n'y a
  aucun rapprochement par nom, pour éviter les erreurs silencieuses.
  - Une référence inconnue, ou un compteur inactif, est une erreur pour cet élément seulement. Le
    reste du lot est traité.
- **Valeur** : c'est la **valeur cumulée absolue** du compteur, et non un écart.
  - Une valeur inférieure à la valeur actuelle sans `reset: true` est rejetée, avec l'erreur
    `valeur en baisse`.
  - Une valeur avec `reset: true` est une remise à zéro (règle de la phase 5 : la distance
    restante jusqu'à l'échéance est conservée).
- **Espacement des relevés** : on n'enregistre pas plus d'un relevé par compteur toutes les
  `counter_min_interval_min` minutes. Les valeurs intermédiaires mettent à jour `current_value`
  sans créer de ligne d'historique.
  - Exception : un relevé qui **franchit un seuil** de planning est toujours enregistré.
- **Effet** : `counter_readings.source = 'supervision'`, `recorded_by_id = NULL`, mise à jour de
  `current_value`, puis **génération des plannings de compteur dus** (phase 5). Tout se fait dans la
  même transaction pour un compteur donné.
- **Dates** : `read_at` dans le futur au-delà de 5 minutes est rejeté. Un `read_at` antérieur au
  dernier relevé est accepté pour l'historique, mais ne fait pas reculer `current_value`.
- **Réponse 200** : `{ "accepted": n, "rejected": m, "errors": [{ "index", "counter_ref", "message" }] }`.

### 2.3 État de l'équipement (information seule)

- **Route** : `POST /api/integrations/supervision/status`, avec le corps
  `{ "statuses": [{ "equipment_ref": "…", "state": "running|stopped|degraded|alarm|unknown", "since": "RFC 3339", "message": "…" }] }`.
- **Rapprochement** : par `equipments.external_ref`, sinon par `equipments.code`, dans le site de
  la clé.
- **Enregistrement** : mise à jour de `equipment_external_status`, plus une ligne d'historique si
  l'état change.
- **Le statut Maint de l'équipement ne change pas**, et aucune intervention n'est créée.
- **Message** : limité à 500 caractères, texte brut. Il est toujours échappé dans l'IHM, car il
  vient d'un système externe.
- **État périmé** : sans mise à jour depuis `status_stale_after_min` minutes, l'état est affiché
  comme « inconnu (dernière information il y a X min) ».
- **Réponse** : même forme que pour les compteurs.

### 2.4 Contrat pour Apex Supervision

Le document `docs/integration-supervision.md` est à rédiger dans cette phase et à remettre à
l'équipe Supervision. Il contient :
- les deux routes, avec leurs schémas JSON et des exemples `curl` ;
- les codes de réponse : 200 avec un détail par élément, 400 si le corps est invalide, 401, 404 si
  la liaison est désactivée, 413, 429 avec `Retry-After` ;
- la stratégie recommandée côté Supervision :
  - envoi par lots toutes les 1 à 5 minutes, et immédiat sur un changement d'état ;
  - nouvelles tentatives avec un délai croissant ;
  - file locale en cas de coupure ;
  - idempotence naturelle : renvoyer une même valeur cumulée est sans effet, et un même état n'est
    pas historisé deux fois ;
- la configuration côté Supervision : adresse d'Apex Maint (HTTPS recommandé, avec l'autorité de la
  phase 9 si elle est auto-signée), clé, correspondance des variables avec les `counter_ref` et
  `equipment_ref`.

## 3. IHM (`static/`)

`CURRENT_PHASE = 10`.

- **Paramètres > Intégrations** (`settings.edit`) :
  - activation de la liaison et de chaque type d'événement ;
  - réglages d'espacement des relevés et de péremption de l'état ;
  - gestion des clés : création avec affichage unique, liste (préfixe, site, portée, dernière
    utilisation), révocation ;
  - **journal des échanges** : derniers envois, compteurs acceptés et rejetés, erreurs.
- **Fiche équipement** :
  - champ « Référence Supervision » (`external_ref`) ;
  - bandeau **« Supervision : À l'arrêt depuis 10:42 — <message> »** quand un état est connu, avec
    une couleur selon l'état et la mention « information » ;
  - historique des états sur 90 jours.
  - Compteurs :
    - champ « Référence Supervision » sur chaque compteur ;
    - pastille « alimenté par Supervision » quand des relevés arrivent ;
    - source de chaque relevé dans l'historique.
- **Liste des équipements** : colonne ou pastille d'état Supervision, facultative, et filtre « à
  l'arrêt selon Supervision ».
- **Tableau de bord** : nouveau widget `supervision_status`, qui liste les équipements à l'arrêt ou
  en alarme selon Supervision (permission `equipments.view`).

## 4. Règles d'accès

| Action | Permission |
|---|---|
| Paramètres d'intégration et clés | `settings.edit` (lecture : `settings.view`) |
| Voir l'état Supervision et la source des relevés | `equipments.view` |
| Modifier `external_ref` (équipement, compteur) | `equipments.edit` |
| Routes `/api/integrations/supervision/*` | clé de service valide, de la bonne portée et non révoquée, dans le site de la clé |

## 5. Tests

### Tests boîte noire

- **Clés** :
  - une clé valide fonctionne seulement sur sa portée, et une clé `status` reçoit 401 sur
    `/counters` ;
  - une clé révoquée ou expirée reçoit 401 ;
  - une clé ne donne accès à aucune autre route, `/api/equipments` compris ;
  - une clé n'agit **que sur son site** : la référence d'un autre site est traitée comme inconnue ;
  - la limitation de débit répond 429 ;
  - liaison désactivée : 404.
- **Compteurs** :
  - lot mixte avec des références inconnues : le traitement est partiel et les erreurs sont
    détaillées ;
  - une valeur en baisse est rejetée, et `reset` est appliqué ;
  - espacement : pas de ligne d'historique entre deux relevés rapprochés, mais `current_value`
    est à jour ;
  - le franchissement d'un seuil enregistre le relevé et **génère l'intervention préventive** ;
  - une même valeur renvoyée est sans effet.
- **État** :
  - le statut Maint de l'équipement est **inchangé** ;
  - l'historique ne garde que les changements ;
  - péremption ;
  - un message contenant du HTML est restitué tel quel en JSON, et donc échappé par l'IHM.
- **Journal des échanges** écrit, et purge à 30 jours.
- **Audit** de la gestion des clés.
