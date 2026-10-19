# Contrat HTTP - backend de la phase 8

Migration : `0009_exploitation.sql`. La migration `0008_pilotage.sql` de la phase 7 la précède.
Les 36 permissions existantes suffisent. Les opérations globales exigent aussi le rôle admin,
afin qu'un acteur limité à un site ne sauvegarde ni ne purge les données des autres sites.

## Représentations exactes

`ExploitationSettings` (champs toujours présents, valeurs par défaut ci-dessous) :

```json
{
  "backup.enabled": true,
  "backup.daily_hour": 2,
  "backup.dir": "",
  "backup.include_files": "folder_only",
  "backup.files_mode": "full",
  "backup.keep_daily": 14,
  "backup.keep_weekly": 8,
  "backup.include_secret_key": false,
  "backup.has_password": false,
  "purge.enabled": false,
  "purge.deleted_after_years": 5,
  "logging.retention_days": 90,
  "files.verify_weekly": false
}
```

`backup.dir` vide signifie `data/backups`. Sinon, chemin absolu.
`backup.include_files` : `folder_only`, `all`, `none`.
PUT accepte un objet partiel avec ces clés, sauf `backup.has_password` qui est en lecture seule.
Il accepte aussi `backup.password`, chaîne non vide ou `null` pour retirer le mot de passe.
Ni le mot de passe ni son chiffrement ne sont renvoyés ni audités.
Heure : 0 à 23 ; rétentions quotidiennes et hebdomadaires : 1 à 36500 ; purge : 2 à 100 ans.
Le mode incrémental facultatif n'est pas implémenté : seul `full` est accepté.

`BackupPage` :

```json
{
  "items": [{"name":"apex-maint-20261018-020000-auto-uuid.zip","size_bytes":12345}],
  "next_cursor": null
}
```

`next_cursor` est le dernier nom renvoyé quand il reste des résultats, sinon `null`.
Tri décroissant par nom ; `?cursor=<nom>&limit=50`, limite de 1 à 200.
La liste n'expose aucun chemin absolu. Les noms comprennent un UUID supplémentaire pour éviter
les collisions entre sauvegardes simultanées.

`BackupCreated` :

```json
{"archive":"apex-maint-20261018-020000-manuel-uuid.zip"}
```

`ExploitationStatus` (valeur initiale) :

```json
{"backup":null,"purge":null,"verify":null}
```

Chaque valeur non nulle a exactement cette forme :

```json
{"at":"2026-10-18T02:00:00.000Z","ok":true,"result":{},"message":null}
```

En cas d'erreur : `ok:false`, `result:null`, `message` chaîne.
Pour une sauvegarde réussie, `result` est `BackupCreated` ; pour la vérification, `FileReport` ;
pour la purge, `PurgeReport`. `at` est l'heure de fin de la tentative.

`FileReport` (CLI, résultat de vérification hebdomadaire) :

```json
{
  "checked":3,
  "migrated":0,
  "anomalies":[{"storage_id":"uuid","key":"site/uuid","message":"Fichier orphelin"}]
}
```

`PurgeReport` (CLI, résultat de purge hebdomadaire) :

```json
{
  "deleted":{"documents":1},
  "retained":["equipments/uuid : référence conservée"],
  "files_deleted":1,
  "file_errors":[],
  "logs_deleted":0,
  "dry_run":false
}
```

`deleted` est un objet table → nombre ; `retained` et `file_errors` sont des listes de chaînes.

Les erreurs HTTP suivent le contrat existant :

```json
{"error":true,"message":"Requête invalide : Paramètre d'exploitation invalide : backup.daily_hour"}
```

## Routes

| Méthode | Route | Droit | Réponse 200 |
|---|---|---|---|
| GET | `/api/exploitation/settings` | `settings.view`, admin | `ExploitationSettings` |
| PUT | `/api/exploitation/settings` | `settings.edit`, admin | `ExploitationSettings` |
| GET | `/api/exploitation/status` | `settings.view`, admin | `ExploitationStatus` |
| GET | `/api/backups` | `settings.view`, admin | `BackupPage` |
| POST | `/api/backups` | `settings.edit`, admin | `BackupCreated` |
| GET | `/api/system/exploitation/settings` | jeton local direct | `ExploitationSettings` |
| PUT | `/api/system/exploitation/settings` | jeton local direct | `ExploitationSettings` |

Les cinq premières routes exigent une session ; les mutations gardent le contrôle CSRF.
POST `/api/backups` n'attend aucun corps et utilise les réglages enregistrés.
Les deux routes `/system/` sont réservées à l'utilitaire : jeton `X-Maint-Token`, pair en boucle
locale directe, aucun en-tête de proxy. Cette exception à la session est justifiée par la phase 8
§5 ; chaque gestionnaire vérifie lui-même le jeton. Elles ne créent jamais de session.
Pas de restauration HTTP : le moteur doit être arrêté.
Codes : 400 validation, 401 session/jeton absent, 403 rôle/droit, 413 taille, 500 échec interne.

## Archives et CLI

Voir [Administration](ADMINISTRATION.md) pour les commandes et confirmations.
Les archives contiennent `manifest.json`, `apex-maint.db`, `config/apex-maint.toml`,
éventuellement `secret.key` et `files/<storage_id>/<version_id>`.
Les métadonnées viennent de la copie `VACUUM INTO`, pas de la base en cours de modification.
Chaque fichier du manifeste porte `size` (entier) et `sha256` (chaîne hexadécimale).
Le manifeste contient `version`, `schema_version`, `created_at`, `instance_name`, `options`
(`include_files`, `with_secret_key`, `automatic`), `files` (objet chemin → empreinte/taille),
`attachments` (liste de `{archive_path,storage_id,storage_key}`). Le manifeste ne s'auto-hache pas.
Toutes les empreintes sont vérifiées avant restauration ; schéma futur et doublons refusés.
AES-256 ZIP compatible 7-Zip disponible par `--password-file` ou par le réglage chiffré en base.

## Exploitation et validation

La rétention utilise les dates des manifestes, y compris chiffrés avec le mot de passe configuré.
Elle conserve séparément les quotidiennes et les dimanches ; les manuelles et les archives
illisibles sont conservées. Un ancien mot de passe doit être fourni pour traiter les archives
chiffrées avec celui-ci.

La purge calcule un plan fermé sur les références SQL et les rattachements polymorphes des
documents. Les enfants sans `deleted_at` suivent leur agrégat ; les références conservées
protègent leurs parents par propagation. Les cycles documents/versions et
occurrences/interventions, ainsi que les hiérarchies, sont traités. Les écritures sont validées
par lots de 500. Les fichiers ne sont supprimés qu'après validation et disparition de leur
dernière référence. Le dry-run exécute le même plan sur une copie `VACUUM INTO`, sans toucher
aux stockages. Les journaux courants restent protégés ; la conservation en jours remplace
le plafond historique `logging.retention_files` pour les journaux du moteur.

L'utilitaire exporte `diagnostic.json` dans un ZIP, par liste blanche : version, date et constats
(titre/niveau). Les détails libres, la base, les journaux et la configuration brute sont exclus,
car ils peuvent contenir des secrets. L'export tourne dans un fil séparé.

Les pages Inno proposent port, écoute locale/réseau et service. Une mise à jour conserve
configuration, compte et autorisations du service existant. La CI construit l'installateur et
les DEB natifs amd64/arm64 ; la recette DEB utilise un conteneur Debian avec systemd, vérifie
`/api/health` sur 18110 puis la conservation à la désinstallation et la suppression à la purge.
Ces recettes de paquets nécessitent les plateformes CI et ne sont pas exécutées dans le bac à sable.

Tests unitaires : options `none`/`folder_only`/`all` avec S3 en mémoire, sauvegarde pendant des
écritures, intégrité SQLite, rétention chiffrée à dates simulées, cycles et références vivantes,
égalité aperçu/exécution, journaux anciens/courants, fichiers modifiés/manquants/orphelins,
migration interrompue puis reprise, audit, secrets absents du diagnostic ZIP. Le test boîte
noire existant couvre les routes, la sauvegarde chiffrée et la restauration neuve ; son exécution
reste à faire hors du bac à sable, sans modifier les ACL de `secret.key`.

Limites explicites : mode incrémental facultatif absent ; archives traitées en mémoire et
limitées à 511 Mio hors manifeste (512 Mio à la lecture). La restauration multi-stockages
n'est pas atomique ; sa sauvegarde de sécurité permet une reprise. Le formulaire graphique
des réglages locaux reste en JSON. Les diagnostics affichent espace libre et version SQLite ;
le service se gère dans son onglet, avec saisie du mot de passe dans un terminal distinct ;
le contenu des archives peut être inspecté sans extraction, y compris leurs noms si chiffrés.

## Raccordements réalisés (19 octobre 2026)

L’IHM Paramètres utilise les routes d’exploitation et de sauvegarde. Les échecs de sauvegarde
alimentent l’événement SMTP `backup_failed`. La phase 7 purge le journal d’audit selon
`audit.retention_days` ; la purge métier ne touche pas à ce journal.
