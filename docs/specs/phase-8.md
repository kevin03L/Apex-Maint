# Phase 8 — Exploitation

Spécification de travail de la phase 8 (voir `docs/PLAN.md` §4 et `docs/specs/decisions.md`, qui
prime). Elle **reprend l'outillage d'Apex Supervision** (`D:\Vs-code\Apex-Supervision`, en lecture
seule) : on copie et on adapte, on ne réinvente pas.

| Élément d'Apex Supervision | Équivalent dans Apex Maint |
|---|---|
| `crates/apex-server/src/service.rs` (SCM Windows et systemd) | identique, avec les noms d'Apex Maint |
| `crates/apex-admin` (utilitaire graphique eframe : état, configuration, journaux, diagnostics, sauvegardes, assistant) | `crates/apex-maint-admin` |
| `packaging/windows` (Inno Setup, script de construction, signature) | identique |
| `packaging/linux` (paquet `debian/`, unité systemd, polkit, raccourci `.desktop`) | identique |
| `tests/exploitation.rs` | même approche |

Il n'y a **pas d'`import-legacy`** (décision du 2026-10-18) : on démarre à vide.

## Résultat attendu

- Première **version alpha installable** sous Windows (Inno Setup) et sous Debian / Ubuntu (DEB).
- Moteur en **service** (Windows et systemd) et utilitaire d'administration graphique.
- **Sauvegardes** automatiques et manuelles, avec rétention et restauration.
- **Purge** des données supprimées anciennes et des journaux.
- Contrôle d'**intégrité** des fichiers et **migration** entre stockages, deux outils annoncés en
  phase 6.

## 1. Service

- **Commandes** (droits administrateur) :
  - `apex-maint service install|uninstall|start|stop|restart|status|enable|disable` ;
  - `apex-maint service run`, le point d'entrée du gestionnaire de services Windows.
  Comportement repris de Supervision.
- **Windows** :
  - le service tourne sous le compte **LocalService** ;
  - l'installation n'accorde à ce compte que l'écriture sur `data`, `logs`, `run` et, s'il est
    local, le dossier de fichiers joints ;
  - `run` (jeton de contrôle) et `data` (base, `secret.key`) sont réservés aux administrateurs et au
    service ;
  - redémarrage automatique en cas d'échec : 3 tentatives, puis arrêt ;
  - **partage réseau** : un stockage de fichiers sur un partage UNC exige que le service tourne sous
    un compte de domaine ou un compte dédié ayant accès au partage.
    - L'assistant de l'utilitaire le signale.
    - `service install --account <compte>` permet de choisir le compte. Le mot de passe est saisi de
      façon interactive et n'est jamais stocké.
- **Linux** :
  - unité systemd `apex-maint.service`, avec l'utilisateur système `apex-maint` ;
  - durcissement : `ProtectSystem=strict`, `ReadWritePaths` limité aux dossiers de données,
    `NoNewPrivileges`, `PrivateTmp` ;
  - règle polkit pour l'utilitaire d'administration.

## 2. Sauvegardes

### 2.1 Contenu et format

- **Archive** : un fichier `apex-maint-AAAAMMJJ-HHMMSS-<auto|manuel>.zip`. Il contient :
  - `manifest.json` : version d'Apex Maint, version du schéma (dernière migration), date, nom de
    l'instance, liste des fichiers avec leur taille et leur SHA-256, options utilisées ;
  - `apex-maint.db` : **copie à chaud cohérente**, obtenue avec `VACUUM INTO` vers un fichier
    temporaire. Le moteur continue de tourner ;
  - `config/apex-maint.toml` : la configuration ;
  - `secret.key`, **seulement si l'option est cochée**. Sans elle, les secrets chiffrés (S3, SMTP)
    devront être ressaisis après une restauration. L'utilitaire le signale clairement ;
  - `files/…` : les fichiers joints, selon les options du §2.2.
- **Chiffrement de l'archive** : facultatif, par un mot de passe (AES-256, ZIP compatible
  7-Zip). Il est recommandé quand `secret.key` est inclus.
- **Fin de la sauvegarde** : l'archive est écrite sous un nom temporaire puis renommée. La
  sauvegarde est tracée dans le journal d'audit et dans `logs/`.

### 2.2 Fichiers joints

Le réglage `backup.include_files` prend l'une de ces valeurs :

| Valeur | Effet |
|---|---|
| `folder_only` (**par défaut**) | Inclut les fichiers des stockages `folder`. Pour les stockages S3, seule la référence (clé, empreinte) est gardée : ils sont supposés sauvegardés par leur fournisseur. |
| `all` | Rapatrie aussi les fichiers S3 dans l'archive. C'est plus long et plus volumineux. |
| `none` | Base seule. Les fichiers sont sauvegardés à part, par l'informaticien. |

**Mode incrémental des fichiers** (facultatif, `backup.files_mode = full|incremental`) :
- l'archive quotidienne ne contient que les fichiers ajoutés depuis la dernière sauvegarde
  complète ;
- une sauvegarde complète est faite chaque semaine.
Ce mode réduit la taille quand il y a beaucoup de pièces jointes. La restauration recompose
l'ensemble à partir de la chaîne d'archives.

### 2.3 Planification et rétention

- **Sauvegarde automatique** : quotidienne, à l'heure `backup.daily_hour` (2 h par défaut),
  exécutée par le moteur lui-même (tâche de fond). Elle est désactivable.
- **Sauvegarde manuelle** :
  - bouton dans l'utilitaire d'administration ;
  - bouton dans Paramètres > Sauvegardes de l'IHM (`settings.edit`) ;
  - commande `apex-maint backup [--output <dossier>] [--include-files folder_only|all|none] [--with-secret-key] [--password-file <f>]`.
- **Destination** : `backup.dir`, `data/backups` par défaut. Ce peut être un partage réseau ; dans
  ce cas, mêmes remarques sur le compte du service qu'au §1. L'écriture est vérifiée avant chaque
  sauvegarde.
- **Rétention** :
  - on garde les `backup.keep_daily` dernières sauvegardes quotidiennes (14 par défaut) et les
    `backup.keep_weekly` dernières hebdomadaires (8 par défaut ; celle du dimanche) ;
  - les sauvegardes manuelles ne sont **jamais** supprimées automatiquement ;
  - une archive incrémentale n'est supprimée qu'avec la complète dont elle dépend.
- **Échec d'une sauvegarde** :
  - une alerte par e-mail part si le SMTP est configuré (nouvel événement `backup_failed`, envoyé
    aux comptes qui ont `settings.edit`) ;
  - l'échec s'affiche en bandeau dans l'IHM pour les administrateurs ;
  - l'état est visible dans l'utilitaire.

### 2.4 Restauration

- **Commande** : `apex-maint restore <archive> [--password-file <f>] [--force]`, **moteur arrêté**
  (sinon refus, code de sortie 4).
- **Déroulement** :
  1. Vérifier l'archive : manifeste et SHA-256 de chaque fichier. Si une seule empreinte est
     fausse, la restauration est refusée.
  2. Vérifier la compatibilité : un schéma plus récent que le binaire est refusé ; un schéma plus
     ancien est accepté, et les migrations seront appliquées au prochain démarrage.
  3. Faire **une sauvegarde de sécurité de l'état actuel** avant d'écraser quoi que ce soit, sauf
     avec `--force`.
  4. Remplacer la base, la configuration (après confirmation), `secret.key` s'il est présent et les
     fichiers du stockage `folder`.
- **Interface** : l'utilitaire propose la même restauration, avec assistant et confirmation.
  L'IHM web **ne restaure pas**, puisque le moteur doit être arrêté.
- **Tests** : une sauvegarde suivie d'une restauration sur une instance neuve redonne les mêmes
  données et les mêmes fichiers, octet pour octet. Une archive corrompue est refusée.

## 3. Purge

- **Commande** : `apex-maint purge [--dry-run] [--older-than <années>]`. Elle tourne aussi
  automatiquement chaque semaine si `purge.enabled` (désactivé par défaut).
- **Données supprimées logiquement depuis plus de `purge.deleted_after_years`** (5 ans par défaut,
  2 au minimum) : elles sont supprimées **physiquement**, avec les fichiers de leurs documents.
  - **Ordre** : des enfants vers les parents (versions de documents, documents, occurrences,
    plannings, réservations, mouvements, pièces consommées, interventions, compteurs et relevés,
    équipements…).
  - **Garde-fou** : un élément encore référencé par une donnée **non supprimée** n'est pas purgé.
    Par exemple, un équipement supprimé dont les interventions sont conservées. Il est signalé dans
    le rapport.
  - **Historique** : le journal d'audit n'est pas touché par cette purge ; il suit sa propre durée
    de conservation (phase 7).
  - **Transactions** : par lots de 500 éléments, en `BEGIN IMMEDIATE`, pour ne pas bloquer le
    moteur longtemps.
  - **Fichiers** : un fichier n'est effacé du stockage qu'après la validation de la transaction qui
    supprime sa dernière référence. S'il ne peut pas être effacé, il est noté dans le rapport, et le
    contrôle d'intégrité (§4) le détectera comme orphelin.
- **Journaux** : les fichiers de `logs/` plus anciens que `logging.retention_days` (90 jours par
  défaut) sont supprimés. Le journal en cours n'est jamais touché.
- **Rapport** : nombre d'éléments par table, fichiers effacés, éléments conservés et pourquoi. Il est
  écrit dans `logs/purge-AAAAMMJJ.log`, et la purge est elle-même tracée dans l'audit.
- **`--dry-run`** : montre le rapport sans rien supprimer.
- **Sauvegarde préalable** : une sauvegarde automatique est faite juste avant une purge réelle.

## 4. Outils sur les fichiers joints

Ces deux outils sont annoncés en phase 6.

- `apex-maint files verify [--storage <id>] [--sample <n>]` :
  - vérifie que chaque version de document existe dans son stockage et que son SHA-256 correspond ;
  - liste aussi les **fichiers orphelins**, présents dans le stockage sans aucune référence, sans
    les supprimer ;
  - produit un rapport. Une exécution hebdomadaire facultative signale les anomalies dans l'IHM
    pour les administrateurs.
- `apex-maint files migrate --from <id> --to <id> [--dry-run]` :
  - copie les fichiers d'un stockage vers un autre, vérifie le SHA-256 après copie, puis met à jour
    `storage_id` version par version ;
  - peut reprendre là où il s'est arrêté ;
  - ne supprime **pas** la source : on la supprime à la main une fois la migration vérifiée ;
  - peut tourner moteur démarré ; dans ce cas, les nouveaux envois vont déjà vers le stockage
    actif.

## 5. Utilitaire d'administration `apex-maint-admin`

C'est une adaptation de `apex-admin` de Supervision (eframe), en français, avec les deux thèmes.
Onglets :

1. **État** : moteur démarré ou arrêté (sonde `/api/health` avec le jeton de contrôle), version,
   adresse de l'IHM (bouton « Ouvrir »), base et taille, dernière sauvegarde, alertes (sauvegarde
   en échec, intégrité, mot de passe d'usine encore actif).
2. **Service** : installer, désinstaller, démarrer, arrêter, redémarrer, compte du service.
3. **Configuration** : formulaire de `apex-maint.toml` (instance, adresse d'écoute, port, chemins,
   journaux), validation par `config check` avant d'enregistrer, et redémarrage proposé.
4. **Sauvegardes** :
   - liste des archives (date, type, taille, contenu) ;
   - « Sauvegarder maintenant », « Restaurer… », « Ouvrir le dossier » ;
   - réglages de planification et de rétention, qui écrivent les clés `backup.*` par l'API locale
     protégée par le jeton de contrôle.
5. **Maintenance** : purge (aperçu puis exécution), vérification des fichiers, migration entre
   stockages, avec leurs rapports.
6. **Journaux** : lecture des journaux du moteur, avec filtres par niveau et recherche.
7. **Diagnostics** : port, droits sur les dossiers, accès au partage réseau, espace disque, version
   de SQLite, export d'un paquet de diagnostic (ZIP sans secrets).
8. **À propos**.

Les opérations lentes tournent dans un fil séparé, comme dans Supervision.

## 6. Paquets

### Windows : Inno Setup

- Installation dans `Program Files\Apex Maint`, données dans `ProgramData\Apex Maint`.
- Pages :
  - choix du port, 3100 par défaut ;
  - écoute locale ou réseau ;
  - installation du service ;
  - création d'un raccourci vers l'utilitaire.
- **Mise à jour sur place** : le service est arrêté, une sauvegarde automatique est faite, les
  fichiers sont remplacés, puis le service redémarre. Les migrations s'appliquent au démarrage.
- **Désinstallation** : elle garde les données par défaut ; une case permet de tout supprimer, avec
  confirmation.
- **Signature** : reprise de `signer.ps1`. Elle est désactivée tant que la décision sur la
  signature n'est pas prise (dépôt public et SignPath : point ouvert).

### Debian / Ubuntu : DEB

- Fichiers installés :
  - `/usr/bin/apex-maint` et `/usr/bin/apex-maint-admin` ;
  - IHM dans `/usr/share/apex-maint/static` ;
  - configuration dans `/etc/apex-maint/` ;
  - données dans `/var/lib/apex-maint/` ;
  - journaux dans `/var/log/apex-maint/`.
- `postinst` : crée l'utilisateur système, fixe les droits, active le service sans le démarrer si
  la configuration est absente, puis affiche l'adresse et le mot de passe d'usine à changer.
- `prerm` et `postrm` : `purge` supprime les données seulement avec la purge du paquet
  (`apt purge`).
- Architectures : `amd64` et `arm64`.

### Construction

- Un script pour chaque plateforme, repris de Supervision.
- La CI construit les deux paquets et les publie comme artefacts.
- La version vient du workspace : la phase 8 publie `0.8.0-alpha.1`.

## 7. Paramètres ajoutés

Clés de `app_settings`, modifiables dans Paramètres > Sauvegardes (IHM) et dans l'utilitaire :

| Clé | Valeur par défaut |
|---|---|
| `backup.enabled` | `true` |
| `backup.daily_hour` | 2 |
| `backup.dir` | `""`, c'est-à-dire `data/backups` |
| `backup.include_files` | `folder_only` |
| `backup.files_mode` | `full` |
| `backup.keep_daily` | 14 |
| `backup.keep_weekly` | 8 |
| `backup.include_secret_key` | `false` |
| `purge.enabled` | `false` |
| `purge.deleted_after_years` | 5 |
| `logging.retention_days` | 90 |
| `files.verify_weekly` | `false` |

Le mot de passe d'archive des sauvegardes automatiques est facultatif. Il est chiffré avec
`secret.key` et n'est jamais renvoyé.

## 8. Tests

### Tests d'exploitation

Même approche que `tests/exploitation.rs` de Supervision : binaire réel, instance portable, ports
libres.

- **Sauvegarde à chaud pendant des écritures** : l'archive reste cohérente (`PRAGMA
  integrity_check`), le manifeste est juste.
- **Restauration** :
  - sur une instance neuve, les données et les fichiers sont identiques ;
  - une archive corrompue ou un schéma futur sont refusés ;
  - la restauration est refusée si le moteur tourne ;
  - une sauvegarde de sécurité est faite avant d'écraser.
- **Options de fichiers** :
  - `folder_only` : un stockage S3 en mémoire n'est pas rapatrié ;
  - `all` : il l'est ;
  - `none` : base seule.
- **Rétention** : avec des dates simulées, le compte des quotidiennes et des hebdomadaires est
  juste, et les sauvegardes manuelles sont conservées.
- **Purge** :
  - `--dry-run` ne modifie rien ;
  - un élément ancien non référencé est supprimé avec ses fichiers ;
  - un élément référencé est conservé et signalé ;
  - les journaux anciens sont supprimés ;
  - l'audit est écrit.
- **Fichiers** :
  - `files verify` détecte un fichier modifié, un fichier manquant et un orphelin ;
  - `files migrate` copie, vérifie et bascule, et peut reprendre après une interruption.
- **Service** : tests repris de Supervision, ignorés hors de la CI Windows ou Linux avec droits
  administrateur.
- **Paquets** : la CI installe le DEB dans un conteneur Debian, démarre le service, interroge
  `/api/health` puis désinstalle. La construction de l'installateur Windows est vérifiée en CI.
