# Phase 7 — Pilotage et paramètres

Spécification de travail de la phase 7 (voir `docs/PLAN.md` §1.1 et §4, et
`docs/specs/decisions.md`, qui prime). Elle s'appuie sur les phases 1 à 6 (mêmes conventions :
`Actor`, filtre de site, 404 hors site, audit dans la même transaction, pagination
`{ items, next_cursor }`, erreurs `{ "error": true, "message": "…" }`).

## Résultat attendu

- **Tableau de bord paramétrable** :
  - un catalogue de widgets ;
  - une disposition par défaut pour chaque rôle, fixée par l'admin ;
  - une disposition personnelle pour les utilisateurs qui en ont le droit ;
  - des widgets filtrés par permission.
- **Reporting** : activité des interventions, fiabilité (MTBF, MTTR, arrêts), préventif, coûts et
  stock, par période, site, ligne et équipement.
- **Exports** CSV, Excel (`.xlsx`) et PDF pour les listes et les rapports.
- **Journal d'audit** consultable, filtrable et exportable, avec une durée de conservation
  paramétrable (3 ans par défaut).
- **Paramètres** complets : général, sessions, stock, SMTP, alertes, audit, plus les onglets de la
  phase 6 (stockage, types de fichiers, étiquettes).
- **Alertes par e-mail** : stock bas, échéances préventives, intervention critique, affectation.
- **Mot de passe oublié** : réinitialisation par l'admin (déjà en place) et lien par e-mail si le
  SMTP est configuré.
- **Parité fonctionnelle** avec l'ancienne application.

## 1. Base de données — migration `0008_pilotage.sql`

### `dashboard_layouts`

| Colonne | Type et contrainte |
|---|---|
| `scope` | `TEXT NOT NULL CHECK (scope IN ('role','user'))` |
| `role` | `TEXT NULL` : renseigné si `scope = 'role'` |
| `user_id` | `TEXT NULL REFERENCES users(id)` : renseigné si `scope = 'user'` |
| `layout` | `TEXT NOT NULL` : JSON, voir §2.1 |
| `updated_at` | |
| `updated_by_id` | |

Index uniques : `(role) WHERE scope = 'role'` et `(user_id) WHERE scope = 'user'`.

### `notification_preferences`

| Colonne | Type et contrainte |
|---|---|
| `user_id` | `TEXT NOT NULL REFERENCES users(id)` |
| `event` | `TEXT NOT NULL CHECK (event IN ('stock_low','maintenance_generated','maintenance_overdue','intervention_critical','intervention_assigned'))` |
| `enabled` | `INTEGER NOT NULL` |

Clé primaire : `(user_id, event)`.

### `outbox_emails` — file d'envoi

| Colonne | Type et contrainte |
|---|---|
| `id` | |
| `site_id` | `NULL` |
| `to_address` | |
| `subject` | |
| `body_text` | |
| `body_html` | |
| `event` | |
| `dedup_key` | `TEXT NULL`, avec un index unique partiel `WHERE status IN ('pending','sent') AND dedup_key IS NOT NULL` |
| `status` | `TEXT NOT NULL CHECK (status IN ('pending','sent','failed'))` |
| `attempts` | `INTEGER NOT NULL DEFAULT 0` |
| `last_error` | `TEXT NULL` |
| `created_at` | |
| `sent_at` | |
| `next_attempt_at` | |

### `password_reset_tokens`

| Colonne | Type et contrainte |
|---|---|
| `id` | |
| `user_id` | |
| `token_hash` | `TEXT NOT NULL UNIQUE` : SHA-256 du jeton |
| `expires_at` | |
| `used_at` | `NULL` |
| `created_at` | |
| `requested_ip` | `NULL` |

### `users`

Ajout de la colonne `email TEXT NULL`, unique parmi les comptes non supprimés.

### `app_settings`

Nouvelles clés :

| Clé | Valeur par défaut |
|---|---|
| `app_name` | « Apex Maint » |
| `app_subtitle` | « GMAO » |
| `session_timeout_min` | 480 (entre 15 et 10 080) |
| `stock_alert_threshold` | existe déjà |
| `smtp.host` | |
| `smtp.port` | |
| `smtp.security` | `starttls`, `tls` ou `none` |
| `smtp.username` | |
| `smtp.password_enc` | chiffré avec la clé de la phase 6 (`data/secret.key`), jamais renvoyé |
| `smtp.from_address` | |
| `smtp.from_name` | |
| `smtp.enabled` | |
| `public_base_url` | adresse publique de l'IHM, pour construire les liens des e-mails |
| `alerts.overdue_digest_hour` | 7 (heure locale de l'envoi du récapitulatif quotidien) |
| `audit.retention_days` | 1 095 (au moins 365) |

### Permissions

Nouvelles permissions :

| Permission | Rôles par défaut |
|---|---|
| `dashboard.customize` | admin, responsable |
| `dashboard.manage_defaults` | admin |
| `exports.run` | admin, responsable |

`reporting.view`, `audit.view`, `settings.view` et `settings.edit` existent déjà.

## 2. Règles

### 2.1 Tableau de bord

- **Catalogue de widgets fixe dans le code.** Chaque widget a un identifiant, un titre, une
  permission requise, des tailles possibles et des paramètres.

  | Widget | Contenu | Permission requise |
  |---|---|---|
  | `interventions_summary` | Interventions ouvertes, en cours et clôturées sur la période | `interventions.view` |
  | `interventions_by_type` | Répartition par type | `interventions.view` |
  | `interventions_by_priority` | Répartition par priorité | `interventions.view` |
  | `my_interventions` | Interventions assignées à l'utilisateur | `interventions.view` |
  | `machines_down` | Équipements à l'arrêt ou dégradés | `equipments.view` |
  | `response_time` | Délai moyen de prise en charge | `interventions.view` |
  | `mtbf_mttr` | MTBF et MTTR par ligne ou par équipement | `reporting.view` |
  | `downtime` | Temps d'arrêt cumulé, top 10 des équipements | `reporting.view` |
  | `preventive_compliance` | Taux de réalisation à l'heure, part préventif / correctif | `maintenance.view` |
  | `maintenance_overdue` | Échéances en retard et à venir | `maintenance.view` |
  | `parts_cost` | Coût des pièces consommées par période et par équipement | `stock.view` et `reporting.view` |
  | `labor_time` | Temps passé par équipement et par technicien | `reporting.view` |
  | `stock_value` | Valeur du stock au prix moyen pondéré | `stock.view` |
  | `stock_alerts` | Pièces sous le seuil | `stock.view` |

- **Disposition** : `{ "widgets": [{ "id", "widget", "x", "y", "w", "h", "params": { "period": "30d", "line_id": null, … } }] }`.
  - La grille fait 12 colonnes, et une seule colonne à 375 px.
  - Au plus 24 widgets par disposition.
  - Les `params` sont validés selon le widget.
- **Ordre de résolution** pour un utilisateur :
  1. sa disposition personnelle, s'il a `dashboard.customize` et qu'elle existe ;
  2. sinon, la disposition par défaut de son rôle ;
  3. sinon, la disposition intégrée du rôle, définie dans le code.
  Dans tous les cas, **les widgets dont la permission manque sont retirés** à l'affichage, et le
  serveur refuse leurs données (403).
- **Droits** :
  - avec `dashboard.customize`, l'utilisateur ajoute, retire, déplace et redimensionne ses widgets,
    et peut revenir à la disposition de son rôle ;
  - avec `dashboard.manage_defaults` (l'admin), on édite la disposition par défaut de chaque rôle.
- **Données** : un appel par widget, `GET /api/dashboard/widgets/{widget}?params…`, qui respecte le
  site. L'admin peut choisir « Tous les sites » ou un site. Le résultat est mis en cache 60 s en
  mémoire, par paire acteur et paramètres.

### 2.2 Définitions des indicateurs (reporting)

- **Périodes** : `7d`, `30d`, `90d`, `12m`, mois civil, année civile, ou dates personnalisées.
  Elles sont calculées en heure locale du serveur.
- **Délai de prise en charge** : `started_at − reported_at`, en moyenne et en médiane.
- **Panne** : intervention `corrective` avec `machine_state = 'ARRET'`. Les interventions annulées
  sont exclues.
- **Temps d'arrêt d'une panne** : `completed_at − reported_at`. Une panne encore ouverte compte
  jusqu'à la fin de la période.
- **MTTR** = somme des temps d'arrêt / nombre de pannes clôturées sur la période.
- **MTBF** = (temps calendaire de la période − temps d'arrêt) / nombre de pannes. Si aucune panne :
  « — », avec la mention « aucune panne sur la période ».
- **Taux de réalisation préventive à l'heure** = occurrences `done` dont l'intervention est
  clôturée au plus tard à `due_date`, divisées par les occurrences échues sur la période (`done`,
  `skipped`, `missed` et en retard).
- **Coût des pièces** : somme `quantité × prix unitaire` des pièces consommées (prix figé à la
  consommation). La valeur du stock est la somme `quantité × PMP` actuelle.
- **Temps passé** : somme de `intervention_technicians.duration_minutes`.
- **Requêtes** : agrégats SQL indexés par `site_id`. Un test de performance vise moins de 500 ms
  sur 50 000 interventions, en mode release.

### 2.3 Reporting

- **Page Reporting** (`reporting.view`) : un onglet par famille d'indicateurs (activité, fiabilité,
  préventif, coûts et stock).
- **Filtres** : période, site (admin), ligne, équipement, type.
- **Présentation** : tableaux et graphiques, en SVG généré côté client sans bibliothèque externe et
  conforme à la CSP.
- **Exports** de chaque rapport.

### 2.4 Exports

- **Exports de listes** (interventions, équipements, pièces, stock, mouvements, plannings, audit) :
  - ils reprennent **les filtres de la liste affichée** ;
  - ils sont plafonnés à 100 000 lignes : au-delà, réponse 413 avec un message invitant à affiner
    les filtres ;
  - permission : `exports.run` en plus de la permission de vue du module.
- **Formats** :
  - **CSV** : séparateur `;`, UTF-8 avec BOM, dates `JJ/MM/AAAA HH:MM`, montants en euros avec la
    virgule décimale.
  - **Excel** : crate `rust_xlsxwriter` ; vraies dates et nombres, en-têtes figés, filtre
    automatique, largeurs ajustées, montants au format `# ##0,00 €`.
  - **PDF** : crate `genpdf` avec une police embarquée (Liberation Sans, licence libre). Trois
    modèles :
    - **fiche intervention** : informations, intervenants et temps, pièces consommées et coût,
      compte rendu, lien vers le planning ;
    - **rapport de période** : indicateurs clés et tableaux de la page Reporting ;
    - **liste filtrée** : en paysage, limitée à 5 000 lignes.
- **Protection contre l'injection de formules** : en CSV et en Excel, toute valeur texte qui
  commence par `=`, `+`, `-`, `@`, une tabulation ou un retour chariot est préfixée par `'`.
- **Téléchargement** :
  - `Content-Disposition: attachment` ;
  - nom de fichier du type `interventions_2026-10-18.xlsx` ;
  - génération en flux continu pour le CSV ;
  - l'export est audité (`entity = 'export'` : type, format, filtres, nombre de lignes).

### 2.5 Journal d'audit

- **Page « Journal d'audit »** (`audit.view`) : liste paginée, du plus récent au plus ancien.
  - **Filtres** : période, site (admin), utilisateur, entité, action, identifiant de l'entité,
    recherche dans le contenu.
  - **Détail** : avant et après, sous forme de différences champ par champ.
  - **Exports** : CSV et Excel.
- **Conservation** : une tâche de fond quotidienne supprime **physiquement** les lignes plus
  anciennes que `audit.retention_days`. C'est la seule suppression physique de l'application ; elle
  est justifiée par la durée de conservation. La purge est elle-même tracée par une ligne d'audit
  qui donne le nombre de lignes supprimées et la date limite.
- L'audit n'est jamais modifiable par l'API.

### 2.6 Paramètres

- **Page « Paramètres »** : modification avec `settings.edit`, consultation seule avec
  `settings.view`. Onglets :
  1. **Général** : nom et sous-titre de l'application (affichés dans l'en-tête et le titre),
     adresse publique de l'IHM.
  2. **Sécurité et sessions** : durée d'inactivité avant déconnexion. Elle s'applique aux nouvelles
     sessions ; les sessions existantes la reprennent à leur prochain accès.
  3. **Stock** : seuil d'alerte global.
  4. **E-mail (SMTP)** : serveur, port, sécurité, identifiants, expéditeur, activation, et bouton
     « Envoyer un e-mail de test » à l'adresse de l'utilisateur.
  5. **Alertes** : préférences par défaut de chaque rôle, heure du récapitulatif quotidien.
  6. **Journal d'audit** : durée de conservation (au moins 1 an).
  7. Les onglets **Stockage**, **Types de fichiers** et **Étiquettes** de la phase 6.
  8. **Tableau de bord** : dispositions par défaut des rôles, avec `dashboard.manage_defaults`.
- **Format et audit** : chaque modification est validée par le serveur (types, bornes, adresse
  e-mail, URL `http` ou `https`) et auditée, avant et après. Les secrets sont masqués dans l'audit.

### 2.7 Alertes par e-mail

Les alertes ne partent que si le SMTP est activé. Sinon, rien n'est mis en file et rien ne
s'affiche comme erreur.

- **Destinataires** : les utilisateurs actifs **du site concerné** qui ont une adresse e-mail, la
  permission correspondante et la préférence activée. Les admins ne reçoivent que s'ils l'ont
  activé.

| Événement | Envoi | Destinataires (permission) |
|---|---|---|
| `stock_low` : une pièce passe sous son seuil (front descendant) | immédiat ; une seule fois par pièce tant que le stock ne remonte pas au-dessus du seuil (`dedup_key`) | `stock.view` |
| `maintenance_generated` : intervention préventive générée | immédiat | technicien par défaut du planning, sinon `maintenance.manage` |
| `maintenance_overdue` : échéances en retard | récapitulatif quotidien à l'heure réglée | `maintenance.manage` |
| `intervention_critical` : création d'une intervention `critical` ou `machine_state = 'ARRET'` | immédiat | `interventions.edit` |
| `intervention_assigned` : assignation d'un technicien | immédiat | le technicien assigné |

- **Mise en file** : les événements écrivent dans `outbox_emails` **dans la même transaction** que
  l'opération métier. Ainsi, aucun e-mail n'est perdu, et aucun n'est envoyé pour une opération
  annulée.
- **Envoi** : un envoyeur en tâche de fond (crate `lettre`, avec rustls) traite la file toutes les
  30 s.
  - Nouvelles tentatives à 1, 5, 30 puis 120 minutes, puis statut `failed`.
  - Les échecs sont visibles dans Paramètres > E-mail, avec la dernière erreur.
- **Contenu** :
  - format texte et HTML simple, en français ;
  - lien vers l'objet construit avec `public_base_url` ;
  - jamais de données d'un autre site ;
  - toute donnée insérée dans le HTML est échappée.
- **Préférences de l'utilisateur** : chacun règle ses alertes dans « Mon profil », parmi celles que
  ses permissions autorisent. Les valeurs par défaut viennent du rôle.

### 2.8 Mot de passe oublié par e-mail

- L'écran de connexion propose « Mot de passe oublié ? » seulement si le SMTP est activé et que
  `public_base_url` est renseigné.
- `POST /api/auth/password-reset/request { identifier }` (identifiant ou e-mail, route publique) :
  - il répond **toujours 202** avec le même message, que le compte existe ou non ;
  - si le compte est actif, avec une adresse e-mail, il crée un jeton de 256 bits (stocké en
    SHA-256) valable 30 minutes et envoie le lien `…/#/reinitialiser?jeton=…` ;
  - il est limité à 3 demandes par compte et par heure, et à 10 par adresse IP et par heure ;
  - les demandes précédentes encore valides sont invalidées.
- `POST /api/auth/password-reset/confirm { token, new_password }` (route publique) :
  - le jeton doit être à usage unique, non expiré, et le compte actif ;
  - le nouveau mot de passe suit les règles de la phase 1 ;
  - la confirmation **révoque toutes les sessions** du compte, lève le verrouillage et efface
    `must_change_password` ;
  - elle est auditée ;
  - un jeton invalide répond 400 avec un message générique.
- La réinitialisation par l'admin (phase 1) est inchangée.
- Les nouvelles routes publiques s'ajoutent explicitement à la liste blanche du garde (refus par
  défaut) et restent soumises au contrôle CSRF d'`Origin`.

## 3. Règles d'accès

| Action | Permission |
|---|---|
| Voir le tableau de bord | `dashboard.view`, puis la permission de chaque widget |
| Personnaliser son tableau de bord | `dashboard.customize` |
| Dispositions par défaut des rôles | `dashboard.manage_defaults` |
| Reporting et indicateurs | `reporting.view` (plus `stock.view` pour les coûts et le stock) |
| Exports | `exports.run`, plus la vue du module concerné |
| Journal d'audit | `audit.view` ; le non-admin ne voit que les lignes de son site |
| Paramètres | `settings.view` / `settings.edit` |
| Préférences d'alertes | chaque utilisateur, pour lui-même |

## 4. API HTTP

| Méthode et route | Remarques |
|---|---|
| `GET /api/dashboard` | Disposition résolue, déjà filtrée par permission, avec `source` (`user`, `role` ou `builtin`) et `can_customize`. |
| `PUT /api/dashboard` | Enregistre la disposition personnelle (`dashboard.customize`). |
| `DELETE /api/dashboard` | Revient à la disposition du rôle. |
| `GET /api/dashboard/catalog` | Widgets disponibles pour l'acteur. |
| `GET /api/dashboard/widgets/{widget}` | Données du widget. Paramètres : `period`, `site_id`, `line_id`, `equipment_id`, `from`, `to`. |
| `GET/PUT /api/dashboard/defaults/{role}` | Dispositions par défaut des rôles (`dashboard.manage_defaults`). |
| `GET /api/reports/{report}` | `report` parmi `activity`, `reliability`, `preventive`, `costs`, `stock`. Mêmes filtres. |
| `GET /api/reports/{report}/export?format=csv\|xlsx\|pdf` | Export d'un rapport. |
| `GET /api/exports/{list}?format=csv\|xlsx\|pdf&<filtres de la liste>` | `list` parmi `interventions`, `equipments`, `parts`, `stock`, `movements`, `maintenance_plans`, `audit`. |
| `GET /api/interventions/{id}/pdf` | Fiche intervention en PDF. |
| `GET /api/audit` | Paginé et filtrable. |
| `GET /api/audit/{id}` | Détail d'une ligne. |
| `GET/PUT /api/settings/general` | Paramètres généraux. |
| `GET/PUT /api/settings/security` | Sécurité et sessions. |
| `GET/PUT /api/settings/stock` | Seuil d'alerte global. |
| `GET/PUT /api/settings/smtp` | Le mot de passe est en écriture seule (`has_password`). |
| `POST /api/settings/smtp/test` | E-mail de test. |
| `GET /api/settings/smtp/outbox` | Derniers envois et échecs. |
| `GET/PUT /api/settings/alerts` | Préférences par défaut des rôles, heure du récapitulatif. |
| `GET/PUT /api/settings/audit` | Durée de conservation. |
| `GET/PUT /api/profile/notifications` | Préférences de l'utilisateur courant. |
| `GET /api/public/branding` | Route publique : `app_name`, `app_subtitle`, `password_reset_available`, pour l'écran de connexion. |
| `POST /api/auth/password-reset/request` | Voir §2.8. |
| `POST /api/auth/password-reset/confirm` | Voir §2.8. |

`PATCH /api/users/{id}` accepte maintenant `email`.

## 5. IHM (`static/`)

`CURRENT_PHASE = 7`. Les modules Tableau de bord, Reporting, Journal d'audit et Paramètres
deviennent actifs. Mêmes règles que d'habitude : `data-action`, `esc()`, `jsArg()`, pas de style ni
de script en ligne, deux thèmes, 375 px.

- **Tableau de bord** :
  - grille de widgets, et une seule colonne à 375 px ;
  - mode « Personnaliser », visible avec `dashboard.customize` : ajout depuis le catalogue,
    déplacement, redimensionnement, paramètres et suppression d'un widget, puis « Enregistrer » ou
    « Revenir à la disposition du rôle ». Le déplacement se fait par glisser-déposer **et** par
    boutons, pour l'accessibilité au clavier ;
  - éditeur des dispositions par rôle dans Paramètres > Tableau de bord.
- **Graphiques** :
  - petite bibliothèque interne de graphiques SVG : barres, barres empilées, courbe, anneau et
    jauge ;
  - couleurs reprises des variables CSS, valides dans les deux thèmes ;
  - info-bulles accessibles et tableau de données équivalent pour chaque graphique.
- **Reporting** : onglets, filtres et boutons d'export (CSV, Excel, PDF).
- **Listes existantes** : interventions, équipements, pièces, stock, mouvements et plannings gagnent
  un menu « Exporter » qui reprend les filtres affichés. La fiche intervention gagne « PDF ».
- **Journal d'audit** : liste filtrable, et détail avec les différences avant/après.
- **Paramètres** : les onglets du §2.6.
- **Mon profil** : adresse e-mail (si l'admin l'autorise : sinon, l'admin la saisit) et cases des
  alertes.
- **Écran de connexion** : nom et sous-titre de l'application, lien « Mot de passe oublié ? »,
  écran de réinitialisation `#/reinitialiser`.

## 6. Tests

### Tests unitaires

- Calcul des indicateurs sur des jeux de données construits à la main : MTBF, MTTR, panne encore
  ouverte en fin de période, aucune panne, taux de réalisation préventive.
- Validation des dispositions : widget inconnu, `params` invalides, plus de 24 widgets.
- Résolution de la disposition et filtrage par permission.
- Neutralisation des formules dans les exports.
- Format CSV : BOM, `;`, virgule décimale.
- Chiffrement du mot de passe SMTP.
- Construction des e-mails : échappement, lien.

### Tests boîte noire

- **Tableau de bord** :
  - un technicien sans `stock.view` ne reçoit pas les widgets de coûts, et leur route répond 403 ;
  - personnalisation enregistrée puis retour au rôle ;
  - un utilisateur sans `dashboard.customize` reçoit 403 sur `PUT`.
- **Indicateurs** : ils correspondent aux valeurs attendues sur un jeu de données fixe. Isolation
  entre sites.
- **Exports** :
  - CSV et XLSX lisibles (le XLSX est relu avec la crate `calamine` en dépendance de
    développement) ;
  - le PDF commence par `%PDF` ;
  - les filtres sont respectés, la limite de lignes est appliquée, l'export est audité ;
  - 403 sans `exports.run`.
- **Audit** :
  - filtres et pagination ;
  - le non-admin ne voit que son site ;
  - la purge supprime au-delà de la durée de conservation, à l'aide d'une variable de test pour
    raccourcir le délai, et trace son exécution.
- **Paramètres** : validation, secrets jamais renvoyés, audit masqué.
- **Alertes** :
  - les e-mails sont mis en file dans la même transaction, et aucun n'est mis en file si
    l'opération est annulée ;
  - déduplication du stock bas ;
  - destinataires filtrés par site et par permission.
  Le SMTP est simulé par un petit serveur SMTP de test écrit dans les tests, qui capture les
  messages.
- **Mot de passe oublié** :
  - même réponse 202 pour un compte inconnu ;
  - jeton à usage unique et expiré ;
  - les sessions sont révoquées ;
  - limitation du nombre de demandes ;
  - route indisponible (404) si le SMTP est désactivé.
