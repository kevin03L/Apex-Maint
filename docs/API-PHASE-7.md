# Contrat HTTP — backend de la phase 7

Les routes exigent le cookie de session `HttpOnly`, sauf `GET /api/public/branding` et les deux
`POST /api/auth/password-reset/{request,confirm}`. Ces exceptions explicites du garde répondent
aux besoins de l'écran de connexion et du §2.8 ; les POST restent soumis au contrôle d'Origin.
Les erreurs suivent `{"error":true,"message":"…"}`. Un objet d'un autre site donne 404.
Les trois nouvelles permissions sont `dashboard.customize`, `dashboard.manage_defaults` et
`exports.run` (39 permissions au total ; admin implicite, responsable : personnalisation et exports).

Les champs montrés sont toujours présents. Les dates et identifiants sont des chaînes ; les
valeurs sans mesure sont `null`. Les tableaux de données d'un widget ou rapport sont complets,
sauf la liste de mes interventions (100 dernières). Seuls les journaux portent un curseur.

## Tableau de bord

Disposition `Layout`, corps des PUT et réponse de `GET/PUT /api/dashboard/defaults/{role}` :

```json
{"widgets":[{"id":"w1","widget":"interventions_summary","x":0,"y":0,"w":6,"h":3,"params":{"period":"30d","site_id":null,"line_id":null,"equipment_id":null,"from":null,"to":null}}]}
```

`params` est un objet dont les six clés ci-dessus sont facultatives. Les valeurs sont des chaînes
ou `null`. Les coordonnées sont des entiers positifs ou nuls ; largeur 1–12, hauteur 1–12, position
horizontale dans les 12 colonnes, 24 widgets maximum, identifiants uniques. Les paramètres sont
validés avant enregistrement. Rôles : `admin`, `responsable`, `production`, `technicien`.

`GET/PUT/DELETE /api/dashboard` :

```json
{"widgets":[{"id":"w1","widget":"interventions_summary","x":0,"y":0,"w":6,"h":3,"params":{"period":"30d"}}],"source":"user","can_customize":true}
```

`source` : `user`, `role` ou `builtin`. DELETE supprime logiquement la disposition personnelle et
renvoie la disposition résolue. Les widgets sans permission sont retirés de cette réponse.

`GET /api/dashboard/catalog` renvoie un **tableau brut** :

```json
[{"widget":"interventions_summary","title":"Synthèse des interventions","permissions":["interventions.view"],"sizes":[{"w":4,"h":2},{"w":6,"h":3},{"w":12,"h":4}],"params":{"period":"30d","site_id":null,"line_id":null,"equipment_id":null,"from":null,"to":null}}]
```

`GET /api/dashboard/widgets/{widget}` :

```json
{"widget":"interventions_summary","from":"2026-01-01T00:00:00+00:00","to":"2026-01-31T00:00:00+00:00","data":{"items":[{"key":"open","count":2}]}}
```

L'enveloppe est identique pour tous les widgets ; `data` prend exactement les formes suivantes :

| Widget | `data` |
|---|---|
| `interventions_summary`, `interventions_by_type`, `interventions_by_priority` | `{"items":[{"key":"open","count":2}]}` ; clé = statut, type ou priorité |
| `response_time` | `{"mean_minutes":30.0,"median_minutes":25.0}` |
| `mtbf_mttr`, `downtime` | forme `Reliability` ci-dessous |
| `preventive_compliance` | forme `Preventive` ci-dessous |
| `parts_cost` | forme `Costs` ci-dessous |
| `labor_time` | `{"labor":[{"equipment_id":"uuid","user_id":"uuid","duration_minutes":60}]}` |
| `stock_value`, `stock_alerts` | forme `Stock` ci-dessous |
| `my_interventions` | `{"items":[{"id":"uuid","code":"INT-001","title":"Réparation","status":"open","priority":"high"}]}` |
| `machines_down` | `{"items":[{"id":"uuid","code":"PRS-01","name":"Presse","status":"out_of_service"}]}` |
| `maintenance_overdue` | `{"items":[{"id":"uuid","code":"PREV-01","title":"Contrôle","next_due_date":"2026-01-01","next_due_counter":null,"overdue":true}]}` |
| `supervision_status` (phase 10, `equipments.view`) | `{"items":[ExternalStatus]}`, équipements à l'arrêt ou en alarme selon Supervision ; forme `ExternalStatus` dans `API-PHASE-10.md` |

Les permissions sont celles du catalogue (§2.1 de la spécification), vérifiées à chaque accès,
y compris avant lecture du cache mémoire de 60 secondes. La clé du cache inclut acteur, site,
rôle, permissions, widget et paramètres.

Toute ligne de widget ou de rapport qui porte `equipment_id` reçoit aussi `equipment_code` et
`equipment_name` (chaînes vides pour une intervention hors équipement) ; toute ligne qui porte
`user_id` reçoit `user_display_name`. Les exports n'affichent que ces libellés, sans identifiant.

## Rapports

`GET /api/reports/{report}` : `report` parmi `activity`, `reliability`, `preventive`, `costs`, `stock`.
Permission `reporting.view`, et `stock.view` pour `costs` et `stock`.

Filtres communs aux rapports et widgets : `period=7d|30d|90d|12m|month|year|custom`,
`from`, `to`, `site_id`, `line_id`, `equipment_id` ; les rapports acceptent aussi `type`.
`from/to` acceptent une date civile locale ou RFC 3339 ; une date de fin civile inclut la journée.
Les deux dates sont obligatoires en période personnalisée. Défaut : `30d` ; maximum 10 ans.

```json
{"report":"activity","from":"2026-01-01T00:00:00+00:00","to":"2026-01-31T00:00:00+00:00","data":{"by_status":[{"key":"open","count":2}],"by_type":[{"key":"corrective","count":2}],"by_priority":[{"key":"high","count":2}],"response_mean_minutes":30.0,"response_median_minutes":25.0}}
```

`data` pour `reliability` (`Reliability`) :

```json
{"items":[{"equipment_id":"uuid","failures":2,"closed_failures":1,"downtime_minutes":240.0,"mttr_minutes":240.0,"mtbf_minutes":600.0}],"failures":2,"closed_failures":1,"downtime_minutes":240.0,"mttr_minutes":240.0,"mtbf_minutes":600.0,"no_failures":false,"message":null}
```

Sans panne : `{"items":[],"failures":0,"closed_failures":0,"downtime_minutes":0.0,"mttr_minutes":null,"mtbf_minutes":null,"no_failures":true,"message":"aucune panne sur la période"}`.
Une panne encore ouverte est bornée à la fin de période ; les temps sont en minutes.

`data` pour `preventive` (`Preventive`) :

```json
{"due_count":2,"on_time_count":1,"compliance_percent":50.0,"by_type":[{"key":"preventive","count":2}]}
```

`data` pour `costs` (`Costs`) :

```json
{"parts_cost_cents":1234,"by_equipment":[{"equipment_id":"uuid","cost_cents":1234}],"labor":[{"equipment_id":"uuid","user_id":"uuid","duration_minutes":60}]}
```

`data` pour `stock` (`Stock`, valeur actuelle, sans historique de valorisation) :

```json
{"value_cents":2000,"items":[{"part_id":"uuid","reference":"REF-01","name":"Roulement","quantity":2,"unit_price_cents":1000,"value_cents":2000,"threshold":5,"low":true}]}
```

## Exports et PDF

Réponses **binaires, sans enveloppe JSON**, avec `Content-Disposition: attachment` et nom
`{liste_ou_rapport}_AAAA-MM-JJ.{format}`. Types : `text/csv; charset=utf-8`,
`application/vnd.openxmlformats-officedocument.spreadsheetml.sheet`, `application/pdf`.

| Route GET | Contenu / filtres |
|---|---|
| `/api/exports/{list}?format=csv\|xlsx\|pdf` | `interventions`, `equipments`, `parts`, `stock`, `movements`, `maintenance_plans`, `audit` ; filtres des listes existantes, sans limiter l'export à la page affichée |
| `/api/reports/{report}/export?format=csv\|xlsx\|pdf` | filtres et indicateurs du rapport |
| `/api/interventions/{id}/pdf` | fiche avec informations, techniciens, compte rendu, occurrence et pièces si `stock.view` |

Permission `exports.run` et vue du module concerné. Audit : CSV ou Excel uniquement.
Limites : 100 000 lignes, PDF de liste 5 000 ; dépassement = 413 et invitation à affiner les filtres.
CSV généré par morceaux : BOM UTF-8, `;`, guillemets doublés, dates françaises, virgule décimale.
XLSX : nombres et dates natifs, volet figé, filtre automatique, largeur ajustée, euros pour les
colonnes en centimes. Les textes à risque de formule sont préfixés par une apostrophe dans les
deux formats. PDF : Liberation Sans embarquée, paysage pour les listes. Chaque export terminé
est tracé dans l'audit (`type`, `format`, `filters`, `row_count`).

## Journal d'audit

`GET /api/audit` : filtres `from`, `to`, `site_id`, `user_id`, `entity`, `action`, `entity_id`, `q`,
`cursor`, `limit` (50 par défaut, 1–1 000). Tri décroissant `(created_at,id)` ; recherche littérale
LIKE échappée dans les contenus avant/après. `GET /api/audit/{id}` renvoie une entrée seule.

```json
{"items":[{"id":"uuid","site_id":"uuid","user_id":"uuid","user_display_name":"Administrateur","action":"update","entity":"settings","entity_id":"general","before":{"app_name":"Apex Maint"},"after":{"app_name":"Atelier"},"diff":[{"field":"app_name","before":"Apex Maint","after":"Atelier"}],"ip":null,"created_at":"2026-10-18T10:00:00.000Z"}],"next_cursor":"curseur-opaque"}
```

`before`, `after`, `site_id`, `user_id`, `user_display_name`, `entity_id`, `ip` peuvent être `null` ; dernier curseur `null`.
Le non-admin ne voit que son site. Aucun PUT, PATCH ou DELETE public. Purge quotidienne après
`audit.retention_days` (1 095 par défaut, minimum 365), tracée avec `deleted_count` et `cutoff`.
La surcharge debug `APEX_MAINT_TEST_AUDIT_PURGE_SECONDS` raccourcit la cadence, pas la rétention.

## Paramètres

GET exige `settings.view`, PUT exige `settings.edit`. PUT accepte une mise à jour partielle des
champs éditables ; sa réponse a exactement la même forme que GET.

| Routes GET/PUT | Réponse exacte |
|---|---|
| `/api/settings/general` | `{"app_name":"Apex Maint","app_subtitle":"GMAO","public_base_url":""}` |
| `/api/settings/security` | `{"session_timeout_min":480}` |
| `/api/settings/stock` | `{"stock_alert_threshold":5}` |
| `/api/settings/smtp` | `{"host":"","port":587,"security":"starttls","username":"","from_address":"","from_name":"Apex Maint","enabled":false,"has_password":false}` |
| `/api/settings/alerts` | `{"overdue_digest_hour":7,"defaults":{}}` |
| `/api/settings/audit` | `{"retention_days":1095}` |

SMTP accepte aussi `password` en écriture seule, chiffré avec `data/secret.key`. `security` vaut
`starttls`, `tls` ou `none`. `has_password` n'est pas éditable. Aucun secret ni texte chiffré dans
les réponses ou l'audit. Les paramètres de stockage, types et étiquettes de phase 6 restent disponibles.
`session_timeout_min` vaut 15–10 080 ; les sessions existantes le relisent au prochain accès.
`defaults` est un objet de rôles vers des objets de préférences, par exemple
`{"technicien":{"intervention_assigned":true},"admin":{"stock_low":false}}`.

`POST /api/settings/smtp/test` exige `settings.edit`, met en file un message à l'utilisateur courant
et renvoie **202, sans corps**. Erreur 400 si SMTP désactivé ou adresse utilisateur manquante.

`GET /api/settings/smtp/outbox` exige `settings.view`, renvoie une page des envois du périmètre (filtres `cursor`, `limit`, 100 par défaut, 1–1 000) :

```json
{"items":[{"id":"uuid","site_id":"uuid","to_address":"technicien@example.test","event":"intervention_assigned","status":"pending","attempts":0,"last_error":null,"created_at":"2026-10-18T10:00:00.000Z","sent_at":null}],"next_cursor":null}
```

`status` : `pending`, `sent`, `failed`. L'erreur peut être une chaîne ; aucun corps de message ni
lien de réinitialisation n'est exposé. Le traitement intervient toutes les 30 secondes ; reprises
à 1, 5, 30, 120 minutes, puis échec définitif. SMTP désactivé : aucune mise en file d'événement.

`GET/PUT /api/profile/notifications` renvoient un **objet à plat**, restreint aux événements
autorisés pour l'acteur ; PUT accepte les mêmes clés booléennes, partiellement :

```json
{"stock_low":true,"maintenance_generated":true,"maintenance_overdue":true,"intervention_critical":true,"intervention_assigned":true}
```

Les valeurs viennent d'abord de l'utilisateur, puis du rôle ; les admins ne sont abonnés que
sur activation explicite. Les destinataires sont actifs, avec adresse, permission et même site.
Les notifications métier sont écrites dans la transaction de l'opération ; le stock bas est
dédupliqué jusqu'à remontée au seuil, et le récapitulatif des échéances est quotidien en heure locale.

## Identité publique et récupération du compte

`GET /api/public/branding` :

```json
{"app_name":"Apex Maint","app_subtitle":"GMAO","password_reset_available":false}
```

`POST /api/auth/password-reset/request`, corps `{"identifier":"identifiant-ou-email"}` : **202**,
même réponse si compte inconnu, inactif, sans e-mail ou limité :

```json
{"message":"Si le compte le permet, un lien de réinitialisation vous sera envoyé."}
```

`POST /api/auth/password-reset/confirm`, corps `{"token":"jeton","new_password":"nouveau-secret"}` :
**204, sans corps**. Jeton invalide, expiré ou déjà utilisé : 400 générique. Ces deux routes donnent
404 si SMTP désactivé ou adresse publique absente. Jeton de 256 bits, SHA-256 en base, validité
30 minutes, maximum trois jetons par compte et dix demandes par IP et heure. La confirmation
révoque toutes les sessions, lève le verrouillage et enlève le changement imposé du mot de passe.

`POST /api/users` et `PATCH /api/users/{id}` acceptent `email` (chaîne validée ou `null`), unique
sans distinction de casse parmi les comptes non supprimés. Les représentations utilisateur
existantes gagnent exactement le champ à plat `"email":null` (ou une adresse) :

```json
{"id":"uuid","site_id":null,"username":"admin","email":null,"display_name":"Administrateur","role":"admin","must_change_password":false,"active":true,"last_login_at":null,"created_at":"2026-10-18T10:00:00.000Z","updated_at":"2026-10-18T10:00:00.000Z","locked":false}
```

Les enveloppes de connexion (`{"user":…}`) et de session existantes restent identiques.
