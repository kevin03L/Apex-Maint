# Contrat HTTP — backend de la phase 9

Migration `0010_terrain.sql`, après `0008_pilotage.sql` et `0009_exploitation.sql`. Aucune permission nouvelle : les droits
des interventions, équipements et paramètres existants s'appliquent. `static/` n'est pas modifié.

## Routes et représentations

| Méthode | Route | Accès | Réponse |
|---|---|---|---|
| GET | `/api/offline/snapshot[?since=RFC3339]` | session, `interventions.view` | `Snapshot` |
| GET | `/api/equipments/by-code/:code` | session, `equipments.view` | équipement de phase 2 ; liste si plusieurs sites correspondent pour l'admin |
| GET | `/api/tls/ca.pem` | publique, seulement `self_signed` | PEM de l'autorité, jamais sa clé ; sinon 404 |

`Snapshot` possède exactement ces collections, même si elles sont vides :

```json
{
  "generated_at":"2026-10-18T12:00:00.000Z",
  "full":true,
  "equipments":[],
  "production_lines":[],
  "counters":[],
  "parts":[],
  "users":[],
  "interventions":[],
  "technicians":[],
  "intervention_ids":[]
}
```

Chaque élément est un objet avec les champs exacts suivants (les champs optionnels sont
présents à `null`, les dates et identifiants sont des chaînes) :

| Collection | Champs |
|---|---|
| `equipments` | `id, site_id, code, name, status, production_line_id, parent_id, updated_at, deleted_at` |
| `production_lines` | `id, site_id, code, name, updated_at, deleted_at` |
| `counters` | `id, site_id, equipment_id, name, unit, current_value, active, updated_at, deleted_at` |
| `parts` | `id, site_id, reference, name, available, updated_at, deleted_at` |
| `users` | `id, display_name, updated_at, deleted_at` |
| `interventions` | `id, site_id, code, title, description, type, priority, status, machine_state, equipment_id, reported_at, started_at, completed_at, solution_report, duration_minutes, updated_at, deleted_at` |
| `technicians` | `intervention_id, user_id, is_lead, assigned_at, duration_minutes` |

`current_value`, `available` et les durées sont des entiers ; `active` et `is_lead` sont des
booléens. Aucune session, aucun hachage, document, mot de passe ou secret n'est exporté.
Les non-admins ne reçoivent que leur site et leurs interventions assignées (ouvertes/en cours,
ou mises à jour depuis sept jours). L'admin conserve le périmètre global du backend existant.
`intervention_ids` est la liste complète des identifiants encore visibles, y compris en delta :
retirer du cache les interventions absentes de cette liste, notamment après désassignation.

`since` valide datant de moins de 24 h active le delta ; une date trop ancienne ou future
redonne un instantané complet, une date mal formée répond 400. Les suppressions sont des
objets avec `deleted_at` non nul. Remplacer la liste des techniciens pour chaque intervention
renvoyée. Les stocks et réservations modifiés rendent également leur pièce éligible au delta.
La réponse est gzip si `Accept-Encoding: gzip` ; `Cache-Control: no-store` reste présent.

## Synchronisation des interventions

Les cinq routes de la spécification conservent leurs corps/réponses de phase 3 : création
(201, `Intervention`), prise en charge (200, `Intervention`), temps (200,
`AssignedTechnician`), PATCH et clôture (200, `Intervention`). PATCH accepte aussi
`solution_report` (chaîne ou null).

- `Idempotency-Key: UUID` : réponse réussie stockée dans la même transaction que l'écriture et
  l'audit. Une répétition renvoie le même code et le même corps, même après redémarrage.
  Les refus métier 4xx sont également mémorisés après annulation de l'opération.
- Réutilisation par un autre utilisateur, un autre site actuel ou une autre route : 422.
  Les permissions actuelles sont contrôlées avant le rejeu. Conservation : trente jours.
- `X-Occurred-At: RFC3339` : date d'action entre réception −7 jours et réception +5 minutes,
  jamais antérieure à l'étape précédente. Sinon, date de réception et audit
  `offline_date_replaced`, dans la transaction. Sans cet en-tête, comportement habituel.
- `If-Unmodified-Since: <updated_at>` sur PATCH : comparaison exacte dans la transaction,
  refus 412 si l'intervention a changé. Les erreurs gardent la forme
  `{"error":true,"message":"…"}`.

## Configuration TLS et proxies

```toml
[server]
bind = "0.0.0.0"
port = 8443
trusted_proxies = []
hsts_max_age = 86400

[server.tls]
mode = "self_signed" # off, files, self_signed
cert_file = ""
key_file = ""
self_signed_names = ["maint.atelier.local", "192.168.1.20"]
http_redirect_port = 8080
```

En mode `files`, chemins absolus ou relatifs à `data_dir`. `config check` et le démarrage
refusent fichiers absents, certificat expiré/pas encore valide et clé incompatible.
La vérification/relecture est horaire ; une paire invalide conserve le certificat actif.
En mode `self_signed`, `data/tls/` est réservé au compte exécutant et aux administrateurs ;
`ca.key` n'est jamais exposée. Le serveur est signé pour 730 jours et renouvelé à moins de
trente jours de l'expiration. L'autorité publique se télécharge par la route ci-dessus.
Les noms automatiques comprennent localhost, les boucles locales, le nom de machine et
ses adresses résolues ; déclarer explicitement les noms/adresses utilisés par les appareils.

Android : installer le PEM comme certificat d'autorité dans les paramètres de sécurité.
iOS : installer le profil téléchargé, puis activer sa confiance dans Réglages > Général >
Informations > Réglages des certificats. Ces opérations restent à recetter sur appareils.

Avec un proxy inverse, utiliser `mode="off"` et déclarer son adresse IP dans
`trusted_proxies`. Les autres pairs ne peuvent imposer ni IP client ni schéma HTTPS.
Le cookie devient `Secure` en TLS direct ou avec `X-Forwarded-Proto: https` venant d'un pair
de confiance. HSTS n'est émis qu'en HTTPS. Les contrôles locaux et leurs détails restent
refusés dès qu'un en-tête de proxy est présent, même pour un proxy de confiance.

L'utilitaire vérifie les connexions TLS avec les autorités système et l'autorité/certificat
de l'instance locale. Les noms fournis doivent couvrir l'adresse utilisée par la sonde.

## Raccordement IHM réalisé (19 octobre 2026)

L’IHM terrain fournit le manifeste, le service worker, le cache IndexedDB, la file hors ligne,
la résolution des conflits, l’effacement à la déconnexion, les QR et le téléchargement de l’autorité.
