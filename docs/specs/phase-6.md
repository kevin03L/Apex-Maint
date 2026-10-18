# Phase 6 — Documents et QR code

Spécification de travail de la phase 6 (voir `docs/PLAN.md` §1.1 et §4, et
`docs/specs/decisions.md`, qui prime). Elle s'appuie sur les phases 1 à 5 :
- `Actor` et filtre de site, 404 hors site ;
- audit dans la même transaction, `BEGIN IMMEDIATE` ;
- pagination `{ items, next_cursor }` ;
- format d'erreur `{ "error": true, "message": "…" }`.

## Résultat attendu

- Pièces jointes **versionnées** sur les équipements, les interventions, les pièces détachées et
  les plannings préventifs.
- Envoi en flux continu, contrôle du type réel et de la taille, téléchargement réservé aux
  utilisateurs autorisés.
- Stockage **paramétrable** : dossier (local ou partage réseau) ou compatible S3.
- Liste blanche des types de fichiers **paramétrable** par l'admin, à partir d'un catalogue sûr.
- QR code par équipement (il contient le code de l'équipement seul) et étiquettes imprimables
  dont le **format est paramétrable**.

## 1. Base de données — migration `0007_documents.sql`

Conventions communes : celles des phases précédentes (UUID v7, `site_id`, `created_at`,
`updated_at`, `deleted_at`, tables `STRICT`, index commençant par `site_id`).

### `storage_backends` — emplacements de stockage

Cette table est globale (pas de `site_id`) : le stockage est un réglage de l'instance.

| Colonne | Type et contrainte |
|---|---|
| `kind` | `TEXT NOT NULL CHECK (kind IN ('folder','s3'))` |
| `label` | `TEXT NOT NULL` |
| `config` | `TEXT NOT NULL` (JSON, voir §2.1) |
| `secret_enc` | `TEXT NULL` : clé secrète S3 chiffrée (voir §2.1) |
| `is_active` | `INTEGER NOT NULL DEFAULT 0` ; index unique partiel `WHERE is_active = 1`, donc un seul stockage actif |

À la migration, un stockage `folder` actif est créé. Son chemin est vide : c'est alors le
`paths.files_dir` de la configuration qui s'applique (`data/files` par défaut).

### `documents` — document logique

| Colonne | Type et contrainte |
|---|---|
| `entity_type` | `TEXT NOT NULL CHECK (entity_type IN ('equipment','intervention','part','maintenance_plan'))` |
| `entity_id` | `TEXT NOT NULL` |
| `title` | `TEXT NOT NULL` |
| `category` | `TEXT NULL` (notice, schéma, certificat, photo, rapport, gamme, autre) |
| `description` | `TEXT NULL` |
| `current_version_id` | `TEXT NULL REFERENCES document_versions(id)` |
| `versions_count` | `INTEGER NOT NULL DEFAULT 0` |

Index : `(site_id, entity_type, entity_id, deleted_at)`.

Le `site_id` du document est celui de l'entité, vérifié par le service. L'entité doit exister,
ne pas être supprimée et appartenir au site de l'acteur.

### `document_versions` — versions

| Colonne | Type et contrainte |
|---|---|
| `document_id` | `TEXT NOT NULL REFERENCES documents(id)` |
| `version_no` | `INTEGER NOT NULL` ; `UNIQUE (document_id, version_no)` |
| `storage_id` | `TEXT NOT NULL REFERENCES storage_backends(id)` : stockage d'origine, qui ne change jamais |
| `storage_key` | `TEXT NOT NULL` : chemin relatif `AAAA/MM/<uuid>`, jamais dérivé du nom d'origine |
| `original_name` | `TEXT NOT NULL` : nom nettoyé pour l'affichage et le téléchargement |
| `mime_type` | `TEXT NOT NULL` : type détecté par le contenu |
| `extension` | `TEXT NOT NULL` |
| `size_bytes` | `INTEGER NOT NULL CHECK (size_bytes > 0)` |
| `sha256` | `TEXT NOT NULL` |
| `comment` | `TEXT NULL` (ex. « Révision C du constructeur ») |
| `uploaded_by_id` | `TEXT NOT NULL REFERENCES users(id)` |

### `app_settings`

Nouvelles clés, globales :

| Clé | Contenu |
|---|---|
| `documents.allowed_types` | Liste JSON d'identifiants du catalogue (§2.2). Par défaut : `pdf`, `jpeg`, `png`, `webp`, `docx`, `doc`, `xlsx`, `xls`, `dwg`, `dxf`, `step`. |
| `documents.max_size_mb` | Taille maximale par fichier, entre 1 et 200. Par défaut 50. |
| `labels.format` | JSON du format d'étiquette (§2.5). |

## 2. Règles

### 2.1 Stockage

- **Bibliothèque** : la crate `object_store`, avec `LocalFileSystem` pour le pilote `folder` et
  `AmazonS3` pour le pilote `s3`. Une seule interface sert à écrire, lire et vérifier les fichiers.
- **Pilote `folder`** : `config = { "path": "D:\\GMAO\\fichiers" | "\\\\serveur\\partage\\gmao" | "" }`.
  - Le chemin doit être absolu, ou vide pour utiliser `files_dir`.
  - Le dossier doit exister et être accessible en écriture : le service le vérifie en y écrivant
    puis en y effaçant un fichier témoin.
- **Pilote `s3`** : `config = { "endpoint", "region", "bucket", "access_key_id", "path_style": bool, "prefix" }`.
  - La clé secrète est chiffrée en AES-256-GCM dans `secret_enc`.
  - La clé de chiffrement est un fichier de 32 octets aléatoires, `data/secret.key`, créé au premier
    démarrage. Ses droits sont restreints au compte du service (même méthode que le jeton de
    contrôle).
  - Le secret n'est **jamais** renvoyé par l'API : on obtient seulement `has_secret: true`, et on ne
    peut que le remplacer.
- **Changement de stockage actif** : seuls les **nouveaux** fichiers vont sur le nouveau stockage.
  Chaque version garde son `storage_id` d'origine et reste lisible tant que ce stockage est
  configuré.
  - Supprimer un stockage encore référencé par une version est refusé (409).
  - La migration des fichiers entre stockages est un outil séparé (phase 8).
- **Test de connexion** : `POST /api/settings/storage/{id}/test` écrit, relit puis efface un fichier
  témoin. Il renvoie `{ ok, message }` et un message d'erreur lisible.

### 2.2 Types de fichiers autorisés

- **Catalogue fixe dans le code.** Chaque entrée associe un identifiant, ses extensions, les types
  MIME attendus, une signature de contenu et la façon de l'afficher :

  | Identifiant | Extensions | Affichage |
  |---|---|---|
  | `pdf` | `.pdf` | `inline` |
  | `jpeg` | `.jpg`, `.jpeg` | `inline` |
  | `png` | `.png` | `inline` |
  | `webp` | `.webp` | `inline` |
  | `gif` | `.gif` | `inline` |
  | `doc` | `.doc` | `attachment` |
  | `docx` | `.docx` | `attachment` |
  | `xls` | `.xls` | `attachment` |
  | `xlsx` | `.xlsx` | `attachment` |
  | `odt` | `.odt` | `attachment` |
  | `ods` | `.ods` | `attachment` |
  | `csv` | `.csv` | `attachment` |
  | `txt` | `.txt` | `attachment` |
  | `dwg` | `.dwg` | `attachment` |
  | `dxf` | `.dxf` | `attachment` |
  | `step` | `.step`, `.stp` | `attachment` |
  | `iges` | `.iges`, `.igs` | `attachment` |
  | `mp4` | `.mp4` | `attachment` |
  | `zip` | `.zip` | `attachment` |

- **Types interdits, quel que soit le réglage** : tout ce qui est hors du catalogue, en particulier
  SVG, HTML, XML, JavaScript, les exécutables et les scripts.
- **Réglage par l'admin** : il coche les identifiants autorisés dans `documents.allowed_types`
  (droit `settings.edit`).
- **Contrôle à l'envoi** : l'extension doit être dans le catalogue et autorisée, **et** la signature
  du contenu doit correspondre (crate `infer`, plus des règles propres pour DXF, STEP, IGES, CSV et
  TXT, qui sont des formats texte). Sinon, la réponse est 415 avec un message clair.
  - Un `.docx` dont le contenu n'est pas un conteneur OOXML est refusé.
  - Un fichier texte qui contient des octets nuls est refusé.

### 2.3 Envoi et téléchargement

- **Envoi** : `multipart/form-data`, lu **en flux continu** vers un fichier temporaire, avec calcul
  du SHA-256 au fil de l'eau.
  - Au-delà de `documents.max_size_mb`, l'envoi est coupé et la réponse est 413.
  - La limite de corps d'Axum est réglée sur ce maximum, plus une marge.
  - Le fichier est ensuite écrit dans le stockage actif, puis la ligne est enregistrée en base dans
    une transaction `BEGIN IMMEDIATE`, avec l'audit.
  - Si l'écriture en base échoue, le fichier écrit est effacé.
  - Le fichier temporaire est toujours supprimé.
- **Nom d'origine** : on ne garde que le nom final, sans les chemins. Les caractères de contrôle et
  `\ / : * ? " < > |` sont retirés et le nom est limité à 200 caractères. Il ne sert qu'à
  l'affichage et à `Content-Disposition`, encodé selon la RFC 5987.
- **Téléchargement** : `GET /api/documents/{id}/download?version=N`, sans `version` pour la version
  courante.
  - Il exige `documents.view` et le droit de voir l'entité. Hors site, la réponse est 404.
  - La réponse est envoyée en flux continu depuis le stockage, avec `Content-Type` égal au type
    détecté, `X-Content-Type-Options: nosniff`, `Cache-Control: private, no-store` et
    `Content-Security-Policy: sandbox; default-src 'none'`.
  - `Content-Disposition` vaut `inline` seulement pour les types marqués `inline` dans le catalogue,
    `attachment` sinon. `?download=1` force `attachment`.
- **Intégrité** : à la lecture, si le fichier est introuvable dans son stockage, la réponse est 410
  avec le message « Fichier introuvable dans le stockage ». La vérification du SHA-256 de tous les
  fichiers est un outil de la phase 8.

### 2.4 Versions et suppression

- **Création d'un document** : elle crée la version 1.
- **Nouvelle version** : `POST /api/documents/{id}/versions`, avec le fichier et un commentaire.
  Elle incrémente `version_no` et met à jour `current_version_id`.
  - Le type de fichier peut changer d'une version à l'autre, par exemple un DWG puis un PDF.
  - Un fichier identique à la version courante (même SHA-256) est refusé : 409, « Ce fichier est
    identique à la version actuelle ».
- **Revenir à une version** : `POST /api/documents/{id}/versions/{n}/restore` crée une **nouvelle**
  version qui copie la référence du fichier de la version n. On ne copie pas le fichier lui-même,
  et l'historique n'est jamais réécrit.
- **Suppression** : elle est logique et porte sur le document entier (`deleted_at`). Les fichiers
  restent dans le stockage ; leur purge relève de la phase 8.
- **Suppression d'une entité** : quand une entité est supprimée logiquement, ses documents restent
  liés mais ne sont plus listés. Ils réapparaissent si l'entité est restaurée.
- **Audit** : `entity = 'document'`. Les actions auditées sont la création, la nouvelle version, la
  restauration, la modification du titre ou de la catégorie et la suppression. Chaque ligne
  d'audit porte le nom du fichier, sa taille et son SHA-256.

### 2.5 QR code et étiquettes

- **QR code** : `GET /api/equipments/{id}/qr.svg` (droit `equipments.view`). L'image est générée
  côté serveur avec la crate `qrcode`. Elle contient **uniquement le code de l'équipement**, avec
  un niveau de correction d'erreur M et une marge de 4 modules. On peut la mettre en cache 1 heure
  (`private`).
- **Format d'étiquette** (`labels.format`), réglé par l'admin dans les Paramètres :
  - `mode` : `sheet` (planche) ou `single` (étiquette unitaire, pour une imprimante thermique ou
    d'étiquettes) ;
  - `sheet` : `page` (`A4` ou `Letter`), `columns`, `rows`, marges haute et gauche, espacements
    horizontal et vertical, en mm ;
  - `single` : `width_mm` et `height_mm` ;
  - commun : taille du QR en mm, et éléments affichés (code, nom, site, ligne de production : des
    cases à cocher) ;
  - valeurs par défaut : planche A4 de 3 × 8, QR de 20 mm, code et nom affichés.
  - Toutes les valeurs sont validées : bornes raisonnables, et une étiquette doit tenir dans la page
    (400 sinon).
- **Page d'impression** : `#/etiquettes?ids=…` dans l'IHM, ou depuis une sélection dans la liste des
  équipements, ou depuis la fiche.
  - La page est construite en HTML et CSS `@page` selon le format, sans script en ligne.
  - Une option « Commencer à la position n » réutilise une planche entamée.
  - L'impression passe par le navigateur.
- **Lecture du QR** : l'IHM le lit en phase 9. Comme le code n'est unique que par site, la recherche
  se fait dans le site de l'utilisateur ; pour l'admin, l'IHM propose un choix si plusieurs sites
  ont le même code.

## 3. Règles d'accès

| Action | Permissions |
|---|---|
| Lister les documents et télécharger | `documents.view`, plus le droit de voir l'entité (`equipments.view`, `interventions.view`, `parts.view` ou `maintenance.view`) |
| Ajouter un document ou une version, modifier le titre ou la catégorie, restaurer | `documents.create`, plus la vue de l'entité |
| Supprimer | `documents.delete` |
| Régler le stockage, les types de fichiers et les étiquettes | `settings.edit` (la lecture demande `settings.view`) |
| QR code et étiquettes | `equipments.view` |

Mêmes règles de site que d'habitude :
- un non-admin voit seulement les documents des entités de son site ;
- tout accès hors site répond 404.

Rôles par défaut :
- `documents.view` : tous les rôles ;
- `documents.create` : admin, responsable, technicien ;
- `documents.delete` : admin, responsable.

## 4. API HTTP

| Méthode et route | Remarques |
|---|---|
| `GET /api/documents?entity_type=&entity_id=` | Les deux paramètres sont obligatoires. Liste non paginée (l'entité a peu de documents), triée par catégorie puis titre. Chaque élément porte `current_version {version_no, original_name, mime_type, size_bytes, uploaded_at, uploaded_by {id, display_name}}` et `versions_count`. |
| `POST /api/documents` | Multipart : `entity_type`, `entity_id`, `title`, `category?`, `description?`, `file`. Réponse 201 avec le document. |
| `GET /api/documents/search?q=&entity_type=&category=&site_id=` | Recherche transversale paginée (`{items, next_cursor}`) pour le module Documents. Chaque élément porte `entity {type, id, code, label}`. Elle exclut les documents des entités supprimées et ceux que l'acteur n'a pas le droit de voir. |
| `GET/PATCH/DELETE /api/documents/{id}` | `PATCH` : `title`, `category`, `description`. |
| `GET /api/documents/{id}/versions` | Toutes les versions, de la plus récente à la plus ancienne. |
| `POST /api/documents/{id}/versions` | Multipart : `file`, `comment?`. Réponse 201. |
| `POST /api/documents/{id}/versions/{n}/restore` | Réponse 201 avec la nouvelle version. |
| `GET /api/documents/{id}/download?version=&download=` | Voir §2.3. |
| `GET /api/equipments/{id}/qr.svg` | `image/svg+xml`. Le SVG est produit par le serveur, sans texte venant de l'utilisateur ; on n'insère que les modules du QR. |
| `GET/PUT /api/settings/documents` | `{ allowed_types: [...], max_size_mb, catalog: [{id, extensions, label, disposition}] }`. Le catalogue est en lecture seule. |
| `GET/PUT /api/settings/labels` | Le format d'étiquette. |
| `GET/POST /api/settings/storage`, `PATCH/DELETE /api/settings/storage/{id}`, `POST /api/settings/storage/{id}/activate`, `POST /api/settings/storage/{id}/test` | Gestion des stockages. `secret` est accepté en écriture, et jamais renvoyé (`has_secret`). |

`GET /api/documents` répond 404 si l'entité est inconnue ou hors site.

Les représentations des entités gagnent un champ `documents_count` : fiches équipement,
intervention, pièce et planning.

## 5. IHM (`static/`)

`CURRENT_PHASE = 6`. Le module « Documents » devient actif. Mêmes règles que d'habitude :
`data-action`, `esc()`, `jsArg()`, pas de style ni de script en ligne, deux thèmes, 375 px.

- **Section « Documents »** commune, réutilisée dans les fiches équipement, intervention, pièce et
  planning.
  - Liste par catégorie : icône selon le type, titre, version, taille, auteur et date.
  - Actions : ouvrir (dans un nouvel onglet pour PDF et images), télécharger, nouvelle version,
    historique des versions (avec « Restaurer »), modifier, supprimer.
  - Ajout par bouton ou par glisser-déposer.
  - Envoi avec `XMLHttpRequest` pour afficher une **barre de progression**.
  - Contrôle préalable de l'extension et de la taille dans le navigateur, avec un message clair. Le
    serveur reste la référence.
- **Module « Documents »** (menu) : vue transversale du site avec tous les documents, des filtres
  (type d'entité, catégorie, recherche dans le titre) et un lien vers l'entité. Il faut pour cela
  ajouter `GET /api/documents/search?q=&entity_type=&category=` paginé (§4), avec les mêmes règles
  de droits.
- **Fiche équipement** : aperçu du QR code, bouton « Imprimer l'étiquette ».
- **Liste des équipements** : cases de sélection et action « Imprimer les étiquettes ».
- **Page d'étiquettes** : aperçu fidèle du format, choix de la position de départ, bouton
  « Imprimer ». Une feuille de style d'impression dédiée masque tout le reste.
- **Paramètres** (visibles avec `settings.view`, modifiables avec `settings.edit`). La page complète
  des Paramètres vient en phase 7 ; la phase 6 en pose le cadre avec trois onglets :
  - **Stockage** : liste des stockages, ajout d'un dossier ou d'un S3, test de connexion,
    activation ;
  - **Types de fichiers** : cases à cocher du catalogue et taille maximale ;
  - **Étiquettes** : formulaire du format avec un aperçu en direct.

## 6. Tests

### Tests unitaires

- Catalogue des types :
  - correspondance entre extension et signature ;
  - un faux `.pdf` contenant du HTML est refusé ;
  - SVG refusé même s'il est demandé dans `allowed_types` (le réglage rejette les identifiants
    inconnus).
- Nettoyage des noms de fichiers.
- Chiffrement et déchiffrement du secret S3.
- Validation des formats d'étiquette.
- Le SVG du QR contient bien le code.

### Tests boîte noire

- Envoi, puis téléchargement octet pour octet, avec les bons en-têtes (`Content-Disposition`,
  `nosniff`, CSP `sandbox`).
- 413 au-delà de la taille maximale, après avoir réglé une petite taille en test.
- 415 pour un type non autorisé, puis accepté une fois autorisé dans les réglages.
- Versions :
  - la version 2 devient courante ;
  - l'ancienne version reste téléchargeable ;
  - restaurer crée une version 3 ;
  - un fichier identique est refusé (409).
- Isolation entre sites : liste, téléchargement et nouvelle version répondent 404.
- 403 sans `documents.create` ou sans `documents.delete`.
- Une entité supprimée masque ses documents.
- Stockage :
  - un nouveau dossier activé reçoit les fichiers suivants, et les anciens restent lisibles ;
  - supprimer un stockage référencé est refusé (409) ;
  - le secret n'est jamais renvoyé ;
  - le test de connexion échoue proprement sur un chemin invalide.
- QR SVG : 200, et 404 hors site.
- Étiquettes : un format invalide est refusé (400).
- Audit écrit pour chaque opération.

Le pilote S3 est testé **unitairement** avec le stockage en mémoire d'`object_store`. Un test
d'intégration réel contre MinIO est prévu, désactivé par défaut (`#[ignore]`).
