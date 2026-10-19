//! Documents versionnés : périmètre métier, audit et stockage historique.
pub mod storage;
pub mod types;

use std::path::Path;

use chrono::Utc;
use object_store::path::Path as ObjectPath;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{FromRow, QueryBuilder, SqliteConnection, SqlitePool};

use crate::{
    domain::sql::like_pattern,
    referentiel::Page,
    store::{self, Audit},
    Actor, Cursor, MaintError, Permission,
};
use storage::{backend, put_file, FileStore};

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Entity {
    #[serde(rename = "type")]
    pub entity_type: String,
    pub id: String,
    pub code: String,
    pub label: String,
    #[serde(skip)]
    pub site_id: String,
}
fn entity_table(
    kind: &str,
) -> Result<(&'static str, &'static str, &'static str, Permission), MaintError> {
    Ok(match kind {
        "equipment" => ("equipments", "code", "name", Permission::EquipmentsView),
        "intervention" => (
            "interventions",
            "code",
            "title",
            Permission::InterventionsView,
        ),
        "part" => ("parts", "reference", "name", Permission::PartsView),
        "maintenance_plan" => (
            "maintenance_plans",
            "code",
            "title",
            Permission::MaintenanceView,
        ),
        _ => return Err(MaintError::BadRequest("Type d'entité inconnu".into())),
    })
}
pub async fn entity(
    conn: &mut SqliteConnection,
    actor: &Actor,
    kind: &str,
    id: &str,
    require_view: bool,
) -> Result<Entity, MaintError> {
    let (table, code, label, permission) = entity_table(kind)?;
    let row: Entity = sqlx::query_as(&format!("SELECT ? AS entity_type,id,{code} AS code,{label} AS label,site_id FROM {table} WHERE id=? AND deleted_at IS NULL AND (? OR site_id=?)"))
        .bind(kind).bind(id).bind(actor.is_admin()).bind(&actor.site_id).fetch_optional(conn).await?.ok_or_else(|| MaintError::NotFound("Entité".into()))?;
    if require_view {
        actor.require_permission(permission)?;
    }
    Ok(row)
}
#[derive(Debug, Clone, Serialize, FromRow)]
pub struct UploadedBy {
    pub id: String,
    pub display_name: String,
}
#[derive(Debug, Clone, Serialize)]
pub struct Version {
    pub id: String,
    pub document_id: String,
    pub version_no: i64,
    pub original_name: String,
    pub mime_type: String,
    pub extension: String,
    pub size_bytes: i64,
    pub sha256: String,
    pub comment: Option<String>,
    pub uploaded_at: String,
    pub uploaded_by: UploadedBy,
    #[serde(skip)]
    pub storage_id: String,
    #[serde(skip)]
    pub storage_key: String,
}
#[derive(FromRow)]
struct RawVersion {
    id: String,
    document_id: String,
    version_no: i64,
    original_name: String,
    mime_type: String,
    extension: String,
    size_bytes: i64,
    sha256: String,
    comment: Option<String>,
    created_at: String,
    uploaded_by_id: String,
    display_name: String,
    storage_id: String,
    storage_key: String,
}
impl From<RawVersion> for Version {
    fn from(v: RawVersion) -> Self {
        Self {
            id: v.id,
            document_id: v.document_id,
            version_no: v.version_no,
            original_name: v.original_name,
            mime_type: v.mime_type,
            extension: v.extension,
            size_bytes: v.size_bytes,
            sha256: v.sha256,
            comment: v.comment,
            uploaded_at: v.created_at,
            uploaded_by: UploadedBy {
                id: v.uploaded_by_id,
                display_name: v.display_name,
            },
            storage_id: v.storage_id,
            storage_key: v.storage_key,
        }
    }
}
#[derive(Debug, Clone, Serialize)]
pub struct Document {
    pub id: String,
    pub site_id: String,
    pub entity_type: String,
    pub entity_id: String,
    pub title: String,
    pub category: Option<String>,
    pub description: Option<String>,
    pub versions_count: i64,
    pub current_version: Version,
    pub entity: Entity,
    pub created_at: String,
    pub updated_at: String,
}
#[derive(FromRow)]
struct RawDocument {
    id: String,
    site_id: String,
    entity_type: String,
    entity_id: String,
    title: String,
    category: Option<String>,
    description: Option<String>,
    versions_count: i64,
    created_at: String,
    updated_at: String,
}
async fn versions_conn(
    conn: &mut SqliteConnection,
    actor: &Actor,
    id: &str,
) -> Result<Vec<Version>, MaintError> {
    let versions: Vec<RawVersion> = sqlx::query_as("SELECT v.*,u.display_name FROM document_versions v JOIN users u ON u.id=v.uploaded_by_id WHERE v.document_id=? AND v.deleted_at IS NULL AND (? OR v.site_id=?) ORDER BY v.version_no DESC")
        .bind(id).bind(actor.is_admin()).bind(&actor.site_id).fetch_all(conn).await?;
    Ok(versions.into_iter().map(Into::into).collect())
}
async fn scoped(
    conn: &mut SqliteConnection,
    actor: &Actor,
    id: &str,
    view: bool,
) -> Result<Document, MaintError> {
    let raw: RawDocument = sqlx::query_as(
        "SELECT * FROM documents WHERE id=? AND deleted_at IS NULL AND (? OR site_id=?)",
    )
    .bind(id)
    .bind(actor.is_admin())
    .bind(&actor.site_id)
    .fetch_optional(&mut *conn)
    .await?
    .ok_or_else(|| MaintError::NotFound("Document".into()))?;
    let entity = entity(conn, actor, &raw.entity_type, &raw.entity_id, view).await?;
    // Conserver la règle existante : la version active au numéro le plus élevé.
    let current_version: RawVersion = sqlx::query_as("SELECT v.*,u.display_name FROM document_versions v JOIN users u ON u.id=v.uploaded_by_id WHERE v.document_id=? AND v.deleted_at IS NULL AND (? OR v.site_id=?) ORDER BY v.version_no DESC LIMIT 1")
        .bind(id).bind(actor.is_admin()).bind(&actor.site_id)
        .fetch_optional(conn).await?
        .ok_or_else(|| MaintError::Internal("Document sans version".into()))?;
    Ok(Document {
        id: raw.id,
        site_id: raw.site_id,
        entity_type: raw.entity_type,
        entity_id: raw.entity_id,
        title: raw.title,
        category: raw.category,
        description: raw.description,
        versions_count: raw.versions_count,
        current_version: current_version.into(),
        entity,
        created_at: raw.created_at,
        updated_at: raw.updated_at,
    })
}
pub async fn get_document(
    pool: &SqlitePool,
    actor: &Actor,
    id: &str,
) -> Result<Document, MaintError> {
    actor.require_permission(Permission::DocumentsView)?;
    scoped(&mut *pool.acquire().await?, actor, id, true).await
}
pub async fn entity_for_upload(
    pool: &SqlitePool,
    actor: &Actor,
    id: &str,
) -> Result<(), MaintError> {
    actor.require_permission(Permission::DocumentsCreate)?;
    scoped(&mut *pool.acquire().await?, actor, id, true).await?;
    Ok(())
}
pub async fn list_documents(
    pool: &SqlitePool,
    actor: &Actor,
    kind: &str,
    entity_id: &str,
) -> Result<Vec<Document>, MaintError> {
    actor.require_permission(Permission::DocumentsView)?;
    let mut conn = pool.acquire().await?;
    entity(&mut conn, actor, kind, entity_id, true).await?;
    let ids:Vec<String> = sqlx::query_scalar("SELECT id FROM documents WHERE entity_type=? AND entity_id=? AND deleted_at IS NULL AND (? OR site_id=?) ORDER BY category,title,id")
        .bind(kind).bind(entity_id).bind(actor.is_admin()).bind(&actor.site_id).fetch_all(&mut *conn).await?;
    let mut result = Vec::new();
    for id in ids {
        result.push(scoped(&mut conn, actor, &id, true).await?);
    }
    Ok(result)
}
#[derive(Default)]
pub struct SearchFilter {
    pub q: Option<String>,
    pub entity_type: Option<String>,
    pub category: Option<String>,
    pub site_id: Option<String>,
    pub cursor: Option<Cursor>,
    pub limit: u32,
}

pub async fn search(
    pool: &SqlitePool,
    actor: &Actor,
    filter: SearchFilter,
) -> Result<Page<Document>, MaintError> {
    actor.require_permission(Permission::DocumentsView)?;
    if let Some(kind) = &filter.entity_type {
        entity_table(kind)?;
    }
    let limit = filter.limit.clamp(1, 100) as usize;
    let mut qb = QueryBuilder::new("SELECT d.id FROM documents d WHERE d.deleted_at IS NULL");
    if !actor.is_admin() {
        qb.push(" AND d.site_id=").push_bind(&actor.site_id);
    }
    if let Some(s) = &filter.site_id {
        qb.push(" AND d.site_id=").push_bind(s);
    }
    if let Some(kind) = &filter.entity_type {
        qb.push(" AND d.entity_type=").push_bind(kind);
    }
    if let Some(category) = &filter.category {
        qb.push(" AND d.category=").push_bind(category);
    }
    if let Some(q) = &filter.q {
        qb.push(" AND d.title LIKE ")
            .push_bind(like_pattern(q))
            .push(" ESCAPE '\\'");
    }
    if let Some(c) = &filter.cursor {
        qb.push(" AND d.id>").push_bind(&c.id);
    }
    qb.push(" AND (");
    let mut first = true;
    for kind in ["equipment", "intervention", "part", "maintenance_plan"] {
        let (table, _, _, permission) = entity_table(kind)?;
        if actor.can(permission) {
            if !first {
                qb.push(" OR ");
            }
            first = false;
            qb.push("(d.entity_type=").push_bind(kind).push(format!(" AND EXISTS(SELECT 1 FROM {table} e WHERE e.id=d.entity_id AND e.site_id=d.site_id AND e.deleted_at IS NULL))"));
        }
    }
    if first {
        qb.push("0");
    }
    qb.push(") ORDER BY d.id LIMIT ")
        .push_bind((limit + 1) as i64);
    let mut ids: Vec<String> = qb.build_query_scalar().fetch_all(pool).await?;
    let next_cursor = if ids.len() > limit {
        ids.truncate(limit);
        ids.last().map(|id| Cursor::new(id).encode())
    } else {
        None
    };
    let mut conn = pool.acquire().await?;
    let mut items = Vec::new();
    for id in ids {
        items.push(scoped(&mut conn, actor, &id, true).await?);
    }
    Ok(Page { items, next_cursor })
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CreateDocument {
    pub entity_type: String,
    pub entity_id: String,
    pub title: String,
    pub category: Option<String>,
    pub description: Option<String>,
}
fn validate_metadata(
    title: &str,
    category: Option<&str>,
    description: Option<&str>,
) -> Result<(), MaintError> {
    if title.trim().is_empty()
        || title.chars().count() > 200
        || description.is_some_and(|s| s.len() > 10000)
        || category.is_some_and(|c| {
            ![
                "notice",
                "schéma",
                "certificat",
                "photo",
                "rapport",
                "gamme",
                "autre",
            ]
            .contains(&c)
        })
    {
        return Err(MaintError::BadRequest(
            "Titre, catégorie ou description invalide".into(),
        ));
    }
    Ok(())
}
/// Fichier temporaire possédé par l'appelant, retiré par sa garde même en cas d'annulation.
pub struct Upload<'a> {
    pub path: &'a Path,
    pub original_name: String,
    pub size_bytes: i64,
    pub sha256: String,
    pub comment: Option<String>,
}
pub async fn upload(
    pool: &SqlitePool,
    actor: &Actor,
    files: &FileStore,
    id: Option<&str>,
    create: Option<CreateDocument>,
    input: Upload<'_>,
) -> Result<Document, MaintError> {
    actor.require_permission(Permission::DocumentsCreate)?;
    let settings = settings_internal(pool, actor).await?;
    if input.size_bytes > i64::from(settings.max_size_mb) * 1024 * 1024 {
        return Err(MaintError::PayloadTooLarge(
            "Fichier trop volumineux".into(),
        ));
    }
    if input.size_bytes <= 0 {
        return Err(MaintError::BadRequest("Fichier vide".into()));
    }
    let name = types::clean_name(&input.original_name);
    let mut file = std::fs::File::open(input.path).map_err(types::io_error)?;
    let (extension, mime_type) = types::detect_file(&mut file, &name, &settings.allowed_types)?;
    let mut conn = pool.acquire().await?;
    if let Some(id) = id {
        scoped(&mut conn, actor, id, true).await?;
    }
    if let Some(c) = &create {
        validate_metadata(&c.title, c.category.as_deref(), c.description.as_deref())?;
        entity(&mut conn, actor, &c.entity_type, &c.entity_id, true).await?;
    }
    let target = backend(&mut conn, actor, None).await?;
    drop(conn);
    let store = files.open(&target)?;
    let key = format!("{}/{}", Utc::now().format("%Y/%m"), store::id());
    put_file(store.clone(), &key, input.path).await?;
    let result = async {
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
        let still_configured = backend(&mut tx, actor, Some(&target.id)).await?;
        if still_configured.kind != target.kind || still_configured.config != target.config {
            return Err(MaintError::Conflict("Le stockage a changé pendant l'envoi ; réessayez".into()));
        }
        let uid = id.map(str::to_string).unwrap_or_else(store::id);
        let before = if id.is_some() { Some(scoped(&mut tx, actor, &uid, true).await?) } else { None };
        if before.as_ref().is_some_and(|d| d.current_version.sha256 == input.sha256) { return Err(MaintError::Conflict("Ce fichier est identique à la version actuelle".into())); }
        let site_id = if let Some(b) = &before { b.site_id.clone() } else {
            let c = create.as_ref().ok_or_else(|| MaintError::BadRequest("Métadonnées requises".into()))?;
            let entity = entity(&mut tx,actor,&c.entity_type,&c.entity_id,true).await?;
            sqlx::query("INSERT INTO documents(id,site_id,entity_type,entity_id,title,category,description,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?)")
                .bind(&uid).bind(&entity.site_id).bind(&c.entity_type).bind(&c.entity_id).bind(c.title.trim()).bind(&c.category).bind(&c.description).bind(store::now()).bind(store::now()).execute(&mut *tx).await?;
            entity.site_id
        };
        let version = Version { id:store::id(),document_id:uid.clone(),version_no:before.as_ref().map_or(1,|d| d.versions_count+1),original_name:name,mime_type,extension,size_bytes:input.size_bytes,sha256:input.sha256,comment:input.comment,uploaded_at:store::now(),uploaded_by:UploadedBy { id:actor.user_id.clone(),display_name:String::new() },storage_id:target.id,storage_key:key.clone() };
        insert_version(&mut tx,actor,&site_id,&version).await?;
        let saved = scoped(&mut tx,actor,&uid,true).await?;
        audit_document(&mut tx,actor,if before.is_some() { "version" } else { "create" },before.as_ref(),&saved).await?;
        tx.commit().await?;
        Ok(saved)
    }.await;
    if result.is_err() {
        store
            .delete(&ObjectPath::from(key))
            .await
            .map_err(storage::storage_error)?;
    }
    result
}
async fn insert_version(
    conn: &mut SqliteConnection,
    actor: &Actor,
    site_id: &str,
    v: &Version,
) -> Result<(), MaintError> {
    sqlx::query("INSERT INTO document_versions(id,site_id,document_id,version_no,storage_id,storage_key,original_name,mime_type,extension,size_bytes,sha256,comment,uploaded_by_id,created_at,updated_at) SELECT ?,?,?,?,?,?,?,?,?,?,?,?,?,?,? WHERE (? OR ?=?)")
        .bind(&v.id).bind(site_id).bind(&v.document_id).bind(v.version_no).bind(&v.storage_id).bind(&v.storage_key).bind(&v.original_name).bind(&v.mime_type).bind(&v.extension).bind(v.size_bytes).bind(&v.sha256).bind(&v.comment).bind(&actor.user_id).bind(&v.uploaded_at).bind(&v.uploaded_at).bind(actor.is_admin()).bind(site_id).bind(&actor.site_id).execute(&mut *conn).await?;
    sqlx::query("UPDATE documents SET current_version_id=?,versions_count=?,updated_at=? WHERE id=? AND (? OR site_id=?)")
        .bind(&v.id).bind(v.version_no).bind(store::now()).bind(&v.document_id).bind(actor.is_admin()).bind(&actor.site_id).execute(conn).await?;
    Ok(())
}
async fn audit_document(
    conn: &mut SqliteConnection,
    actor: &Actor,
    action: &str,
    before: Option<&Document>,
    after: &Document,
) -> Result<(), MaintError> {
    store::audit(
        conn,
        actor,
        Audit {
            action,
            entity: "document",
            entity_id: Some(&after.id),
            site_id: Some(&after.site_id),
            before: before.map(|d| json!(d)),
            after: Some(json!(after)),
            ip: None,
        },
    )
    .await
}
pub async fn versions(
    pool: &SqlitePool,
    actor: &Actor,
    id: &str,
) -> Result<Vec<Version>, MaintError> {
    get_document(pool, actor, id).await?;
    versions_conn(&mut *pool.acquire().await?, actor, id).await
}
pub async fn restore(
    pool: &SqlitePool,
    actor: &Actor,
    id: &str,
    number: i64,
) -> Result<Version, MaintError> {
    actor.require_permission(Permission::DocumentsCreate)?;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let before = scoped(&mut tx, actor, id, true).await?;
    let mut version = versions_conn(&mut tx, actor, id)
        .await?
        .into_iter()
        .find(|v| v.version_no == number)
        .ok_or_else(|| MaintError::NotFound("Version".into()))?;
    version.id = store::id();
    version.version_no = before.versions_count + 1;
    version.uploaded_at = store::now();
    version.comment = Some(format!("Restauration de la version {number}"));
    insert_version(&mut tx, actor, &before.site_id, &version).await?;
    let after = scoped(&mut tx, actor, id, true).await?;
    audit_document(&mut tx, actor, "restore", Some(&before), &after).await?;
    tx.commit().await?;
    Ok(after.current_version)
}
pub async fn update_document(
    pool: &SqlitePool,
    actor: &Actor,
    id: &str,
    patch: Value,
    delete: bool,
) -> Result<Document, MaintError> {
    actor.require_permission(if delete {
        Permission::DocumentsDelete
    } else {
        Permission::DocumentsCreate
    })?;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let before = scoped(&mut tx, actor, id, !delete).await?;
    let mut next =
        json!({"title":before.title,"category":before.category,"description":before.description});
    for (key, value) in patch
        .as_object()
        .ok_or_else(|| MaintError::BadRequest("Objet JSON attendu".into()))?
    {
        if !["title", "category", "description"].contains(&key.as_str()) {
            return Err(MaintError::BadRequest("Champ document inconnu".into()));
        }
        next[key] = value.clone();
    }
    #[derive(Deserialize)]
    struct Metadata {
        title: String,
        category: Option<String>,
        description: Option<String>,
    }
    let m: Metadata = serde_json::from_value(next)
        .map_err(|_| MaintError::BadRequest("Métadonnées invalides".into()))?;
    validate_metadata(&m.title, m.category.as_deref(), m.description.as_deref())?;
    sqlx::query("UPDATE documents SET title=?,category=?,description=?,updated_at=? WHERE id=? AND (? OR site_id=?)")
        .bind(m.title.trim()).bind(&m.category).bind(&m.description).bind(store::now()).bind(id).bind(actor.is_admin()).bind(&actor.site_id).execute(&mut *tx).await?;
    let after = scoped(&mut tx, actor, id, !delete).await?;
    if delete {
        sqlx::query("UPDATE documents SET deleted_at=? WHERE id=? AND (? OR site_id=?)")
            .bind(store::now())
            .bind(id)
            .bind(actor.is_admin())
            .bind(&actor.site_id)
            .execute(&mut *tx)
            .await?;
    }
    audit_document(
        &mut tx,
        actor,
        if delete { "delete" } else { "update" },
        Some(&before),
        &after,
    )
    .await?;
    tx.commit().await?;
    Ok(after)
}
pub async fn download(
    pool: &SqlitePool,
    actor: &Actor,
    files: &FileStore,
    id: &str,
    number: Option<i64>,
) -> Result<(Version, object_store::GetResult), MaintError> {
    let document = get_document(pool, actor, id).await?;
    let version = if let Some(n) = number {
        versions(pool, actor, id)
            .await?
            .into_iter()
            .find(|v| v.version_no == n)
            .ok_or_else(|| MaintError::NotFound("Version".into()))?
    } else {
        document.current_version
    };
    let target = backend(
        &mut *pool.acquire().await?,
        actor,
        Some(&version.storage_id),
    )
    .await?;
    let result = files
        .open(&target)?
        .get(&ObjectPath::from(version.storage_key.as_str()))
        .await
        .map_err(storage::storage_error)?;
    Ok((version, result))
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentSettings {
    pub allowed_types: Vec<String>,
    pub max_size_mb: u32,
}
pub async fn settings_internal(
    pool: &SqlitePool,
    actor: &Actor,
) -> Result<DocumentSettings, MaintError> {
    if !actor.can(Permission::SettingsView) && !actor.can(Permission::SettingsEdit) {
        actor.require_permission(Permission::DocumentsCreate)?;
    }
    Ok(DocumentSettings {
        allowed_types: read_setting(pool, "documents.allowed_types").await?,
        max_size_mb: read_setting(pool, "documents.max_size_mb").await?,
    })
}
async fn read_setting<T: serde::de::DeserializeOwned>(
    pool: &SqlitePool,
    key: &str,
) -> Result<T, MaintError> {
    let raw: String = sqlx::query_scalar("SELECT value FROM app_settings WHERE key=?")
        .bind(key)
        .fetch_one(pool)
        .await?;
    serde_json::from_str(&raw).map_err(|_| MaintError::Internal("Paramètre stocké invalide".into()))
}
pub async fn settings(pool: &SqlitePool, actor: &Actor) -> Result<Value, MaintError> {
    actor.require_permission(Permission::SettingsView)?;
    let s = settings_internal(pool, actor).await?;
    Ok(
        json!({"allowed_types":s.allowed_types,"max_size_mb":s.max_size_mb,"catalog":types::CATALOG}),
    )
}
pub async fn set_settings(
    pool: &SqlitePool,
    actor: &Actor,
    s: DocumentSettings,
) -> Result<Value, MaintError> {
    actor.require_permission(Permission::SettingsEdit)?;
    types::validate_settings(&s.allowed_types, s.max_size_mb)?;
    set_values(
        pool,
        actor,
        vec![
            ("documents.allowed_types", json!(s.allowed_types)),
            ("documents.max_size_mb", json!(s.max_size_mb)),
        ],
    )
    .await?;
    Ok(
        json!({"allowed_types":s.allowed_types,"max_size_mb":s.max_size_mb,"catalog":types::CATALOG}),
    )
}
pub async fn labels(pool: &SqlitePool, actor: &Actor) -> Result<types::LabelFormat, MaintError> {
    actor.require_permission(Permission::SettingsView)?;
    read_setting(pool, "labels.format").await
}
pub async fn set_labels(
    pool: &SqlitePool,
    actor: &Actor,
    value: types::LabelFormat,
) -> Result<types::LabelFormat, MaintError> {
    actor.require_permission(Permission::SettingsEdit)?;
    value.validate()?;
    set_values(pool, actor, vec![("labels.format", json!(value))]).await?;
    Ok(value)
}
async fn set_values(
    pool: &SqlitePool,
    actor: &Actor,
    values: Vec<(&str, Value)>,
) -> Result<(), MaintError> {
    actor.require_permission(Permission::SettingsEdit)?;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    for (key, value) in values {
        let before: String = sqlx::query_scalar("SELECT value FROM app_settings WHERE key=?")
            .bind(key)
            .fetch_one(&mut *tx)
            .await?;
        sqlx::query("UPDATE app_settings SET value=?,updated_at=? WHERE key=?")
            .bind(value.to_string())
            .bind(store::now())
            .bind(key)
            .execute(&mut *tx)
            .await?;
        store::audit(
            &mut tx,
            actor,
            Audit {
                action: "update",
                entity: "app_setting",
                entity_id: Some(key),
                site_id: None,
                before: Some(json!(before)),
                after: Some(value),
                ip: None,
            },
        )
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests;
