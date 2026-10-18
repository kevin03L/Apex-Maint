# Contrat HTTP — backend de la phase 10

Liaison avec Apex Supervision. Deux familles de routes coexistent :
1. **Routes d'intégration Supervision** (`/api/integrations/supervision/*`) : authentification par
   clé de service (`Authorization: Bearer apx_...`), sans cookie, sans session et sans CSRF.
   Ces routes refusent les cookies de session sans clé (401). Si la liaison ou le type d'événement
   est désactivé : 404. Limitation de débit à 120 requêtes par minute par clé (429 avec `Retry-After`),
   corps limité à 1 Mo, lots limités à 1 000 éléments.
2. **Routes de gestion IHM** : authentification par session cookie habituelle (`HttpOnly`).
   Permissions RBAC : `settings.view` / `settings.edit`, `equipments.view` / `equipments.edit`.

## 1. Routes d'intégration (Supervision -> Maint)

| Méthode | Route | Accès | Corps | Réponse |
|---|---|---|---|---|
| POST | `/api/integrations/supervision/counters` | Clé Bearer (scope `counters`) | `CountersInput` (max 1 Mo, 1 000 éléments) | 200 `BatchReport` |
| POST | `/api/integrations/supervision/status` | Clé Bearer (scope `status`) | `StatusBatchInput` (max 1 Mo, 1 000 éléments) | 200 `BatchReport` |

### Représentations JSON des routes d'intégration

Corps de requête `CountersInput` :

```json
{
  "readings": [
    {
      "counter_ref": "VAR_HOURS_01",
      "value": 12345,
      "read_at": "2026-10-18T10:30:00.000Z",
      "reset": false
    }
  ]
}
```

Corps de requête `StatusBatchInput` :

```json
{
  "statuses": [
    {
      "equipment_ref": "PRS-01",
      "state": "alarm",
      "since": "2026-10-18T10:28:00.000Z",
      "message": "Surchauffe groupe hydraulique > 85°C"
    }
  ]
}
```

Valeurs admises pour `state` : `"running"`, `"stopped"`, `"degraded"`, `"alarm"`, `"unknown"`.
Le champ `message` est facultatif (texte brut, limité à 500 caractères).

Réponse `BatchReport` (200 OK pour traitement partiel ou total) :

```json
{
  "accepted": 1,
  "rejected": 1,
  "errors": [
    {
      "index": 0,
      "counter_ref": "VAR_INCONNUE",
      "message": "Ressource introuvable : Référence de compteur inconnue ou inactive"
    }
  ]
}
```

Pour les statuts, l'objet dans `errors` utilise la clé `equipment_ref` à la place de `counter_ref`.

## 2. Routes de gestion IHM

| Méthode | Route | Accès | Description |
|---|---|---|---|
| GET | `/api/integration/settings` | Session, `settings.view` | Paramètres d'intégration |
| PUT | `/api/integration/settings` | Session, `settings.edit` | Modification des paramètres |
| GET | `/api/integration/keys[?cursor=...&limit=50]` | Session, `settings.view` | Liste paginée des clés de service |
| POST | `/api/integration/keys` | Session, `settings.edit` | Création d'une clé (affiche la clé en clair une seule fois) |
| PATCH | `/api/integration/keys/:id` | Session, `settings.edit` | Modification des portées d'une clé |
| POST | `/api/integration/keys/:id/revoke` | Session, `settings.edit` | Révocation d'une clé |
| GET | `/api/integration/events[?cursor=...&limit=50]` | Session, `settings.view` | Journal des échanges |
| GET | `/api/integration/statuses[?equipment_id=...&state=...&cursor=...&limit=50]` | Session, `equipments.view` | Liste filtrée des derniers états connus |
| GET | `/api/equipments/:id/supervision-status` | Session, `equipments.view` | Dernier état connu d'un équipement (`ExternalStatus` ou `null`) |
| GET | `/api/equipments/:id/supervision-history[?cursor=...&limit=50]` | Session, `equipments.view` | Historique des changements d'état d'un équipement |
| PATCH | `/api/equipments/:id/external-ref` | Session, `equipments.edit` | Définition de `external_ref` sur un équipement |
| PATCH | `/api/counters/:id/external-ref` | Session, `equipments.edit` | Définition de `external_ref` sur un compteur |
| GET | `/api/integration/supervision-status[?cursor=...&limit=50]` | Session, `equipments.view` | Données du widget (équipements à l'arrêt ou en alarme) |

### Formes JSON exactes

`IntegrationSettings` :

```json
{
  "integration.supervision.enabled": true,
  "integration.supervision.counters_enabled": true,
  "integration.supervision.status_enabled": true,
  "integration.supervision.counter_min_interval_min": 15,
  "integration.supervision.status_stale_after_min": 30,
  "integration.supervision.status_history_days": 90
}
```

Création de clé (`POST /api/integration/keys`) :

Corps de requête :

```json
{
  "site_id": "0192e210-...",
  "label": "Supervision Atelier A",
  "scopes": ["counters", "status"],
  "expires_at": null
}
```

Réponse (201 Created) :

```json
{
  "key": "apx_abcdefghijklmnopqrstuvwxyz0123456789-_ABCDE",
  "item": {
    "id": "0192e210-...",
    "site_id": "0192e210-...",
    "label": "Supervision Atelier A",
    "key_prefix": "apx_abcd",
    "scopes": ["counters", "status"],
    "created_by_id": "0192e210-...",
    "created_at": "2026-10-18T10:00:00.000Z",
    "last_used_at": null,
    "last_used_ip": null,
    "revoked_at": null,
    "expires_at": null
  }
}
```

La clé secrète complète `key` n'est retournée **qu'à cet instant précis** et n'est jamais stockée.

`ExternalStatus` (renvoyé par `statuses`, `supervision-status` et `widget`) :

```json
{
  "equipment_id": "0192e210-...",
  "site_id": "0192e210-...",
  "equipment_code": "PRS-01",
  "equipment_name": "Presse hydraulique 1",
  "external_ref": "SUP-EQ-01",
  "source": "supervision",
  "state": "alarm",
  "original_state": "alarm",
  "since": "2026-10-18T10:00:00.000Z",
  "message": "Alarme température huile > 80°C",
  "received_at": "2026-10-18T10:05:00.000Z",
  "stale": false
}
```

Si le délai depuis `received_at` dépasse `status_stale_after_min`, `stale` vaut `true` et `state` est calculé à `"unknown"`. `original_state` conserve l'état initial brut.

`StatusHistory` (`GET /api/equipments/:id/supervision-history`) :

```json
{
  "id": "0192e210-...",
  "equipment_id": "0192e210-...",
  "site_id": "0192e210-...",
  "source": "supervision",
  "state": "alarm",
  "since": "2026-10-18T10:00:00.000Z",
  "message": "Alarme température",
  "received_at": "2026-10-18T10:05:00.000Z"
}
```

`IntegrationEvent` (`GET /api/integration/events`) :

```json
{
  "id": "0192e210-...",
  "site_id": "0192e210-...",
  "key_id": "0192e210-...",
  "kind": "counters",
  "received_at": "2026-10-18T10:05:00.000Z",
  "items_count": 2,
  "accepted": 2,
  "rejected": 0,
  "errors": [],
  "duration_ms": 12
}
```

`Reference` (`PATCH /api/equipments/:id/external-ref` et `PATCH /api/counters/:id/external-ref`) :

Corps de requête :

```json
{
  "external_ref": "VAR_SUPERVISION_ID"
}
```

Réponse (200 OK) :

```json
{
  "id": "0192e210-...",
  "external_ref": "VAR_SUPERVISION_ID"
}
```

Passer `external_ref: null` dissocie la référence. Unicité stricte par site sur les entités non supprimées (409 en cas de doublon).

## Référence Supervision dans les représentations existantes

Les représentations `Equipment` (fiches et listes) et `EquipmentCounter` gagnent le champ à plat
`"external_ref": null` (ou une chaîne), modifiable par `PATCH /api/equipments/{id}/external-ref` et
`PATCH /api/counters/{id}/external-ref`.
