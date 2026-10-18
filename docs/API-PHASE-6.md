# Contrat HTTP — backend de la phase 6

Toutes les routes ci-dessous exigent le cookie de session `HttpOnly`. Aucune nouvelle route
publique. Droits et périmètres : `specs/phase-6.md` §3. Les 36 permissions existantes comprennent
déjà toutes les clés nécessaires et leur matrice par défaut ; aucune clé n'a été ajoutée.

Les champs sont toujours présents, y compris les champs optionnels à `null`, sauf indication
contraire. Les identifiants et dates sont des chaînes ; les tailles et numéros sont des entiers.

## Représentations exactes

`Version` (également la forme exacte de `Document.current_version`) :

```json
{
  "id": "uuid-version",
  "document_id": "uuid-document",
  "version_no": 1,
  "original_name": "notice.pdf",
  "mime_type": "application/pdf",
  "extension": "pdf",
  "size_bytes": 123,
  "sha256": "empreinte-hexadecimale",
  "comment": null,
  "uploaded_at": "2026-10-18T10:00:00.000Z",
  "uploaded_by": {"id": "uuid-utilisateur", "display_name": "Responsable"}
}
```

`Document` :

```json
{
  "id": "uuid-document",
  "site_id": "uuid-site",
  "entity_type": "equipment",
  "entity_id": "uuid-entite",
  "title": "Notice",
  "category": "notice",
  "description": null,
  "versions_count": 1,
  "current_version": {
    "id": "uuid-version",
    "document_id": "uuid-document",
    "version_no": 1,
    "original_name": "notice.pdf",
    "mime_type": "application/pdf",
    "extension": "pdf",
    "size_bytes": 123,
    "sha256": "empreinte-hexadecimale",
    "comment": null,
    "uploaded_at": "2026-10-18T10:00:00.000Z",
    "uploaded_by": {"id": "uuid-utilisateur", "display_name": "Responsable"}
  },
  "entity": {"type": "equipment", "id": "uuid-entite", "code": "PRS-01", "label": "Presse"},
  "created_at": "2026-10-18T10:00:00.000Z",
  "updated_at": "2026-10-18T10:00:00.000Z"
}
```

`entity_type` et `entity.type` : `equipment`, `intervention`, `part`, `maintenance_plan`.
Pour une pièce, `entity.code` est sa référence ; `label` est le nom ou le titre de l'entité.
Catégories : `notice`, `schéma`, `certificat`, `photo`, `rapport`, `gamme`, `autre`, ou `null`.
`description` et `comment` acceptent aussi une chaîne. Les références internes de stockage ne
sont jamais exposées dans les versions.

`Storage` :

```json
{
  "id": "uuid-stockage",
  "kind": "folder",
  "label": "Dossier par défaut",
  "config": {"path": ""},
  "has_secret": false,
  "is_active": true,
  "created_at": "2026-10-18T10:00:00.000Z",
  "updated_at": "2026-10-18T10:00:00.000Z"
}
```

Pour `kind: "s3"`, la forme exacte de `config` est :

```json
{"endpoint":"https://s3.exemple.fr","region":"eu-west-1","bucket":"documents","access_key_id":"compte","path_style":true,"prefix":"maint"}
```

`secret` est accepté seulement en écriture ; ni lui ni `secret_enc` ne sont sérialisés ou audités.

`DocumentSettings` (GET et réponse PUT) :

```json
{
  "allowed_types": ["pdf", "jpeg", "png", "webp", "docx", "doc", "xlsx", "xls", "dwg", "dxf", "step"],
  "max_size_mb": 50,
  "catalog": [
    {"id":"pdf","extensions":["pdf"],"label":"PDF","disposition":"inline"},
    {"id":"jpeg","extensions":["jpg","jpeg"],"label":"Image JPEG","disposition":"inline"},
    {"id":"png","extensions":["png"],"label":"Image PNG","disposition":"inline"},
    {"id":"webp","extensions":["webp"],"label":"Image WebP","disposition":"inline"},
    {"id":"gif","extensions":["gif"],"label":"Image GIF","disposition":"inline"},
    {"id":"doc","extensions":["doc"],"label":"Word","disposition":"attachment"},
    {"id":"docx","extensions":["docx"],"label":"Word OOXML","disposition":"attachment"},
    {"id":"xls","extensions":["xls"],"label":"Excel","disposition":"attachment"},
    {"id":"xlsx","extensions":["xlsx"],"label":"Excel OOXML","disposition":"attachment"},
    {"id":"odt","extensions":["odt"],"label":"Texte OpenDocument","disposition":"attachment"},
    {"id":"ods","extensions":["ods"],"label":"Tableur OpenDocument","disposition":"attachment"},
    {"id":"csv","extensions":["csv"],"label":"CSV","disposition":"attachment"},
    {"id":"txt","extensions":["txt"],"label":"Texte","disposition":"attachment"},
    {"id":"dwg","extensions":["dwg"],"label":"Dessin DWG","disposition":"attachment"},
    {"id":"dxf","extensions":["dxf"],"label":"Dessin DXF","disposition":"attachment"},
    {"id":"step","extensions":["step","stp"],"label":"Modèle STEP","disposition":"attachment"},
    {"id":"iges","extensions":["iges","igs"],"label":"Modèle IGES","disposition":"attachment"},
    {"id":"mp4","extensions":["mp4"],"label":"Vidéo MP4","disposition":"attachment"},
    {"id":"zip","extensions":["zip"],"label":"Archive ZIP","disposition":"attachment"}
  ]
}
```

Le PUT reçoit uniquement `{"allowed_types":["pdf"],"max_size_mb":50}` ; le catalogue est en lecture seule.
Les extensions du catalogue n'ont pas de point et sont en minuscules.

`LabelFormat` (GET, entrée et réponse PUT, mêmes champs dans les deux modes) :

```json
{
  "mode": "sheet",
  "sheet": {
    "page": "A4",
    "columns": 3,
    "rows": 8,
    "margin_top_mm": 5.0,
    "margin_left_mm": 5.0,
    "gap_horizontal_mm": 0.0,
    "gap_vertical_mm": 0.0
  },
  "single": {"width_mm": 70.0, "height_mm": 37.0},
  "qr_size_mm": 20.0,
  "show_code": true,
  "show_name": true,
  "show_site": false,
  "show_production_line": false
}
```

`mode` : `sheet` ou `single`. `page` : `A4` ou `Letter`. Marges symétriques, dimensions en mm.
Colonnes 1–20, lignes 1–50, QR 10–100 mm, dimensions unitaires 10–500 mm ; rejet si le QR,
une marge de 2 mm et 3 mm par texte affiché ne tiennent pas dans la cellule calculée.

## Routes et corps retournés

Dans ce tableau, `[Document]`, `[Version]` et `[Storage]` désignent des tableaux JSON bruts,
éventuellement vides ; les noms sans crochets désignent les objets exacts ci-dessus.

| Méthode et route | Réponse |
|---|---|
| `GET /api/documents?entity_type=&entity_id=` | 200, `[Document]`, catégorie puis titre puis id ; deux paramètres obligatoires |
| `POST /api/documents` | 201, `Document` ; multipart `entity_type`, `entity_id`, `title`, `category?`, `description?`, `file` |
| `GET /api/documents/search?q=&entity_type=&category=&site_id=&cursor=&limit=` | 200, `{"items":[Document],"next_cursor":null}` ou curseur chaîne ; tri par id croissant, limite 50 par défaut, bornée à 1–100 |
| `GET /api/documents/{id}` | 200, `Document` |
| `PATCH /api/documents/{id}` | 200, `Document` ; JSON partiel `title`, `category`, `description` |
| `DELETE /api/documents/{id}` | 204, aucun corps |
| `GET /api/documents/{id}/versions` | 200, `[Version]`, numéro décroissant |
| `POST /api/documents/{id}/versions` | 201, `Version` ; multipart `file`, `comment?` |
| `POST /api/documents/{id}/versions/{n}/restore` | 201, `Version` nouvellement créée |
| `GET /api/documents/{id}/download?version=&download=` | 200, flux binaire ; sans version : courante ; `download=1` force le téléchargement |
| `GET /api/equipments/{id}/qr.svg` | 200, SVG `image/svg+xml` (aucun JSON), code seul, correction M et marge de quatre modules |
| `GET /api/settings/documents` | 200, `DocumentSettings` |
| `PUT /api/settings/documents` | 200, `DocumentSettings` |
| `GET /api/settings/labels` | 200, `LabelFormat` |
| `PUT /api/settings/labels` | 200, `LabelFormat` |
| `GET /api/settings/storage` | 200, `[Storage]` |
| `POST /api/settings/storage` | 201, `Storage` ; JSON `kind`, `label`, `config`, `secret?` |
| `PATCH /api/settings/storage/{id}` | 200, `Storage` ; mêmes champs partiels ; `config` remplace l'objet entier |
| `DELETE /api/settings/storage/{id}` | 204, aucun corps |
| `POST /api/settings/storage/{id}/activate` | 204, aucun corps |
| `POST /api/settings/storage/{id}/test` | 200, `{"ok":true,"message":"Écriture, relecture et suppression réussies"}` ou `{"ok":false,"message":"Stockage inaccessible ; vérifiez le chemin, la connexion et les droits du service"}` |

Erreurs : `{"error":true,"message":"…"}`. Fichier disparu : 410 avec
`{"error":true,"message":"Fichier introuvable dans le stockage"}`. Le contrôle de site donne 404 ;
les droits manquants 403 ; type refusé 415 ; taille dépassée 413 ; doublon SHA-256 409.

Les fiches équipement, intervention, pièce et planning ajoutent **à plat** `"documents_count":0`
(entier dynamique), sans modifier la structure des autres champs. Le compteur est également
présent dans les listes qui réutilisent ces représentations.

## Choix précisés et validation

- Les documents portent aussi leur référence d'entité dans les réponses de fiche et de liste.
  La version courante réutilise la représentation complète de version.
- La destination d'un stockage référencé est immuable ; le libellé et le secret S3 restent
  remplaçables. Créer un nouveau stockage pour changer la destination. La suppression d'un
  stockage actif est refusée, même sans version, pour conserver un stockage actif.
- Dossier : existence et écriture vérifiées à l'enregistrement, puis à l'activation. Le test
  reste disponible si le dossier devient inaccessible après son enregistrement.
- `data/secret.key` contient 32 octets, conservés entre redémarrages ; mêmes restrictions que
  le jeton de contrôle, avec mode Unix 0600. Ne pas perdre cette clé : les secrets S3 en dépendent.
- Aucun fichier de packaging n'existait ni n'est nécessaire à cette phase. L'IHM est hors périmètre.
- Nouvelles versions directes figées : `object_store` 0.12.4, `aes-gcm` 0.10.3, `infer` 0.19.0,
  `qrcode` 0.14.1, `zip` 4.6.1, `tempfile` 3.23.0 (publications antérieures de plus de deux semaines).
  Les autres dépendances directes réutilisent les familles déjà transitives du workspace.

Validation (2026-10-18) : `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`
et `cargo test --workspace` passent (129 tests, dont un test MinIO ignoré faute d'instance).

Fichiers créés : `crates/apex-core/migrations/0007_documents.sql`,
`crates/apex-core/src/documents/{mod,storage,types,tests}.rs`,
`crates/apex-server/src/http/documents.rs`, `crates/apex-server/tests/documents.rs`, ce document.

Fichiers modifiés : `Cargo.toml`, les deux manifestes de crates core/server,
`crates/apex-core/src/{lib,error,referentiel,interventions,stock,maintenance}.rs`,
`crates/apex-server/src/{app,http}.rs`, `crates/apex-server/tests/common/mod.rs`,
`README.md` et `docs/PLAN.md` (une ligne chacun). Aucun changement de cet agent dans `static/`,
aucun changement de branche ni commit.
