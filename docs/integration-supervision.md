# Contrat d'intégration — Apex Supervision vers Apex Maint

Ce document décrit le contrat d'API que doit respecter le module d'export d'**Apex Supervision**
pour alimenter **Apex Maint** (GMAO).

## 1. Principes généraux

- **Architecture découplée** : Apex Supervision et Apex Maint possèdent leurs propres bases de données.
  Tous les échanges transitent exclusivement par l'API REST HTTPS d'Apex Maint.
- **Sens unique** : Les flux vont uniquement de Supervision vers Maint.
- **Authentification par clé de service** : Chaque instance de Supervision utilise une clé secrète
  dédiée générée dans les Paramètres d'Apex Maint. Cette clé est rattachée à un site géographique
  et transmise dans l'en-tête `Authorization: Bearer apx_...`.
- **Tolérance aux pannes et idempotence** :
  - Réémettre une même valeur cumulée de compteur ne crée aucun doublon.
  - Réémettre un même état d'équipement n'engendre aucune nouvelle ligne d'historique.
  - Les lots sont traités de manière unitaire : un élément rejeté (par exemple une référence
    inconnue) n'empêche pas l'enregistrement des éléments valides du même lot.
- **Informationnelle pour les statuts** : L'état Supervision est affiché à titre indicatif sur la fiche
  équipement et dans le tableau de bord. Il **ne modifie pas** le statut de maintenance de la machine
  et ne déclenche pas d'intervention corrective automatique.
- **Déclenchement automatique pour les compteurs** : Les relevés de compteurs alimentent le moteur
  de maintenance préventive d'Apex Maint. Si un seuil d'intervention est franchi, l'intervention
  préventive est automatiquement planifiée.

---

## 2. Authentification et en-têtes HTTP

Toute requête vers les routes d'intégration doit fournir :

```http
Authorization: Bearer apx_XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX
Content-Type: application/json
Accept: application/json
```

- Si la clé est absente, invalide, expirée, révoquée ou n'a pas la bonne portée : **401 Unauthorized** (sans détail technique).
- Si la liaison d'intégration ou le type d'événement est désactivé dans Apex Maint : **404 Not Found**.
- Limitation de débit : **120 requêtes par minute** par clé. Au-delà : **429 Too Many Requests** avec l'en-tête standard `Retry-After: <secondes>`.
- Taille maximale du corps : **1 Mo** (au-delà : **413 Payload Too Large**).
- Taille maximale du lot : **1 000 éléments** par lot (au-delà : **400 Bad Request**).

---

## 3. Routes d'ingestion

### 3.1 Relevés de compteurs (`POST /api/integrations/supervision/counters`)

Alimente les compteurs d'heures ou de cycles d'équipements pour le calcul des échéances préventives.

#### Rapprochement
- Par `counter_ref` correspondant à la colonne `external_ref` du compteur dans le site de la clé.
- Si le compteur est inconnu ou inactif dans Maint, l'élément est rejeté avec une erreur détaillée dans le rapport, sans bloquer les autres éléments du lot.

#### Données
- `value` : Valeur **cumulée absolue** du compteur (non un incrément).
- Une valeur inférieure à la valeur actuelle sans `"reset": true` est rejetée avec l'erreur `valeur en baisse`.
- `"reset": true` signale une remise à zéro physique ou un changement d'organe : la distance restante avant la prochaine échéance préventive est recalculée en conséquence.
- `read_at` : Horodatage ISO-8601 (RFC 3339) de la mesure. Les dates dans le futur de plus de 5 minutes sont rejetées.

#### Schéma de la requête

```json
{
  "readings": [
    {
      "counter_ref": "SUPERV_PRESS_01_HOURS",
      "value": 14250,
      "read_at": "2026-10-18T14:32:00.000Z",
      "reset": false
    },
    {
      "counter_ref": "SUPERV_ROBOT_02_CYCLES",
      "value": 850230,
      "read_at": "2026-10-18T14:32:00.000Z",
      "reset": false
    }
  ]
}
```

#### Exemple `curl`

```bash
curl -X POST "https://maint.atelier.local/api/integrations/supervision/counters" \
  -H "Authorization: Bearer apx_VOTRE_CLE_DE_SERVICE" \
  -H "Content-Type: application/json" \
  -d '{
    "readings": [
      {
        "counter_ref": "SUPERV_PRESS_01_HOURS",
        "value": 14250,
        "read_at": "2026-10-18T14:32:00.000Z",
        "reset": false
      }
    ]
  }'
```

---

### 3.2 États des équipements (`POST /api/integrations/supervision/status`)

Transmet le dernier état opérationnel des machines pour affichage sur les bandeaux de supervision
dans Maint et dans le widget d'alerte du tableau de bord.

#### Rapprochement
- En priorité par `equipment_ref` correspondant à `external_ref` de l'équipement dans le site.
- À défaut, rapprochement par le `code` officiel de l'équipement (ex. `PRS-01`) dans le site.

#### Données
- `state` : Obligatoire, parmi :
  - `"running"` : En fonctionnement nominal.
  - `"stopped"` : Machine à l'arrêt.
  - `"degraded"` : Fonctionnement dégradé.
  - `"alarm"` : Équipement en défaut / alarme.
  - `"unknown"` : État indéterminé côté supervision.
- `since` : Date et heure ISO-8601 du début de cet état.
- `message` : Message facultatif décrivant l'événement (max 500 caractères, texte brut).

#### Schéma de la requête

```json
{
  "statuses": [
    {
      "equipment_ref": "PRS-01",
      "state": "alarm",
      "since": "2026-10-18T14:30:15.000Z",
      "message": "Défaut variateur axe principal — code défaut F042"
    },
    {
      "equipment_ref": "ROB-02",
      "state": "running",
      "since": "2026-10-18T08:00:00.000Z",
      "message": null
    }
  ]
}
```

#### Exemple `curl`

```bash
curl -X POST "https://maint.atelier.local/api/integrations/supervision/status" \
  -H "Authorization: Bearer apx_VOTRE_CLE_DE_SERVICE" \
  -H "Content-Type: application/json" \
  -d '{
    "statuses": [
      {
        "equipment_ref": "PRS-01",
        "state": "alarm",
        "since": "2026-10-18T14:30:15.000Z",
        "message": "Défaut variateur axe principal"
      }
    ]
  }'
```

---

## 4. Réponses et gestion d'erreurs

Les deux routes renvoient toujours un code **200 OK** avec un compte rendu de lot (`BatchReport`) dès lors que le corps JSON est syntaxiquement correct, même si certains éléments sont rejetés :

```json
{
  "accepted": 1,
  "rejected": 1,
  "errors": [
    {
      "index": 1,
      "counter_ref": "SUPERV_INCONNU",
      "message": "Ressource introuvable : Référence de compteur inconnue ou inactive"
    }
  ]
}
```

*(Pour les statuts, le champ d'erreur s'appelle `equipment_ref`).*

### Codes HTTP de retour

| Code | Signification | Conduite à tenir côté Supervision |
|---|---|---|
| **200 OK** | Lot traité (totalement ou partiellement). | Examiner `rejected` et `errors`. Si des éléments sont rejetés pour cause de référence inconnue, consigner l'alerte pour configuration. Ne pas rejouer les rejets définitifs. |
| **400 Bad Request** | JSON invalide, champs manquants ou lot > 1 000 éléments. | Corriger le format d'envoi. Ne pas retenter sans correction. |
| **401 Unauthorized** | Clé absente, mauvaise portée, révoquée ou expirée. | Vérifier la clé de service configurée dans Supervision. Alerte administration. |
| **404 Not Found** | Liaison désactivée dans Apex Maint. | Suspendre temporairement les envois ou patienter jusqu'à activation de la liaison. |
| **413 Payload Too Large** | Corps de requête supérieur à 1 Mo. | Réduire la taille des lots (envoyer par paquets de 200 à 500 éléments). |
| **429 Too Many Requests** | Débit maximal atteint (120 req/min). | Respecter le délai indiqué par l'en-tête `Retry-After: <secondes>` avant tout nouvel appel. |
| **500 / 503** | Erreur serveur ou serveur Maint redémarrant. | Conserver le lot dans la file locale et rejouer avec backoff exponentiel. |

---

## 5. Stratégie recommandée pour l'agent d'export Supervision

1. **Fréquence d'envoi** :
   - Compteurs : envoi groupé toutes les **1 à 5 minutes**.
   - Changements d'état : envoi **immédiat** sur transition (`running` -> `stopped`, alarme, etc.), complété d'un rafraîchissement périodique de l'ensemble du parc toutes les 5 à 15 minutes.
2. **File d'attente locale (FIFO) persistante** :
   - En cas d'indisponibilité réseau ou de maintenance d'Apex Maint, stocker les lots dans une file locale sur disque.
   - À la reconnexion, vider la file par lots ordonnés dans la limite de 60 requêtes par minute pour ne pas saturer le débit.
3. **Péremption des états dans Maint** :
   - Apex Maint considère un état comme périmé après un délai configurable (`status_stale_after_min`, 30 minutes par défaut).
   - Supervision doit donc retransmettre périodiquement l'état des machines même stables pour éviter qu'elles n'apparaissent « inconnues ».
4. **Sécurité TLS** :
   - Utiliser HTTPS. Si Apex Maint utilise une autorité de certification locale auto-signée (phase 9), importer le certificat CA d'Apex Maint (`/api/tls/ca.pem`) dans le truststore de Supervision.
