//! Stockages historiques : dossier et S3, secrets chiffrés au repos.
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use object_store::{
    aws::AmazonS3Builder, local::LocalFileSystem, path::Path as ObjectPath, prefix::PrefixStore,
    ObjectStore,
};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{FromRow, SqliteConnection, SqlitePool};
use tokio::io::AsyncReadExt;

use super::types::io_error;
use crate::{
    store::{self, Audit},
    Actor, MaintError, Permission,
};

#[derive(Clone)]
pub struct FileStore {
    pub files_dir: PathBuf,
    key: [u8; 32],
}
impl FileStore {
    pub fn new(files_dir: PathBuf, key: [u8; 32]) -> Self {
        Self { files_dir, key }
    }
    pub fn encrypt(&self, plain: &str) -> Result<String, MaintError> {
        let mut nonce = [0; 12];
        rand::rngs::OsRng.fill_bytes(&mut nonce);
        let cipher = Aes256Gcm::new_from_slice(&self.key)
            .map_err(|_| MaintError::Internal("Clé de chiffrement invalide".into()))?;
        let encrypted = cipher
            .encrypt(Nonce::from_slice(&nonce), plain.as_bytes())
            .map_err(|_| MaintError::Internal("Chiffrement impossible".into()))?;
        Ok(nonce
            .into_iter()
            .chain(encrypted)
            .map(|b| format!("{b:02x}"))
            .collect())
    }
    pub fn decrypt(&self, encrypted: &str) -> Result<String, MaintError> {
        let invalid = || MaintError::Internal("Secret chiffré invalide".into());
        if encrypted.len() < 56 || !encrypted.len().is_multiple_of(2) || !encrypted.is_ascii() {
            return Err(invalid());
        }
        let bytes = (0..encrypted.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&encrypted[i..i + 2], 16).map_err(|_| invalid()))
            .collect::<Result<Vec<_>, _>>()?;
        let cipher = Aes256Gcm::new_from_slice(&self.key).map_err(|_| invalid())?;
        let plain = cipher
            .decrypt(Nonce::from_slice(&bytes[..12]), &bytes[12..])
            .map_err(|_| invalid())?;
        String::from_utf8(plain).map_err(|_| invalid())
    }
    pub fn open(&self, backend: &StorageBackend) -> Result<Arc<dyn ObjectStore>, MaintError> {
        match backend.kind.as_str() {
            "folder" => {
                let path = backend.config["path"].as_str().unwrap_or("");
                let path = if path.is_empty() {
                    self.files_dir.as_path()
                } else {
                    Path::new(path)
                };
                Ok(Arc::new(
                    LocalFileSystem::new_with_prefix(path).map_err(storage_error)?,
                ))
            }
            "s3" => {
                let c: S3Config = serde_json::from_value(backend.config.clone())
                    .map_err(|_| MaintError::BadRequest("Configuration S3 invalide".into()))?;
                let secret = self.decrypt(
                    backend
                        .secret_enc
                        .as_deref()
                        .ok_or_else(|| MaintError::BadRequest("Clé secrète S3 requise".into()))?,
                )?;
                let store = AmazonS3Builder::new()
                    .with_endpoint(&c.endpoint)
                    .with_region(&c.region)
                    .with_bucket_name(&c.bucket)
                    .with_access_key_id(&c.access_key_id)
                    .with_secret_access_key(secret)
                    .with_virtual_hosted_style_request(!c.path_style)
                    .with_allow_http(c.endpoint.starts_with("http://"))
                    .build()
                    .map_err(storage_error)?;
                Ok(Arc::new(PrefixStore::new(
                    store,
                    ObjectPath::from(c.prefix),
                )))
            }
            _ => Err(MaintError::BadRequest("Pilote de stockage inconnu".into())),
        }
    }
}
pub fn storage_error(e: object_store::Error) -> MaintError {
    // Une erreur du pilote peut contenir des informations d'authentification : jamais dans l'API.
    match e {
        object_store::Error::NotFound { .. } => {
            MaintError::Gone("Fichier introuvable dans le stockage".into())
        }
        _ => MaintError::Unavailable(
            "Stockage inaccessible ; vérifiez le chemin, la connexion et les droits du service"
                .into(),
        ),
    }
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct StorageBackend {
    pub id: String,
    pub kind: String,
    pub label: String,
    #[sqlx(json)]
    pub config: Value,
    #[serde(skip)]
    pub secret_enc: Option<String>,
    pub has_secret: bool,
    pub is_active: bool,
    pub created_at: String,
    pub updated_at: String,
}
const COLUMNS: &str = "id,kind,label,config,secret_enc,secret_enc IS NOT NULL AS has_secret,is_active,created_at,updated_at";
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageInput {
    pub kind: String,
    pub label: String,
    pub config: Value,
    pub secret: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FolderConfig {
    path: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct S3Config {
    endpoint: String,
    region: String,
    bucket: String,
    access_key_id: String,
    path_style: bool,
    prefix: String,
}
fn validate(input: &StorageInput) -> Result<(), MaintError> {
    let invalid = || MaintError::BadRequest("Configuration de stockage invalide".into());
    if input.label.trim().is_empty() || input.label.len() > 200 {
        return Err(invalid());
    }
    match input.kind.as_str() {
        "folder" => {
            let c: FolderConfig =
                serde_json::from_value(input.config.clone()).map_err(|_| invalid())?;
            if !c.path.is_empty() && !Path::new(&c.path).is_absolute() {
                return Err(invalid());
            }
            if input.secret.is_some() {
                return Err(invalid());
            }
        }
        "s3" => {
            let c: S3Config =
                serde_json::from_value(input.config.clone()).map_err(|_| invalid())?;
            if !(c.endpoint.starts_with("https://") || c.endpoint.starts_with("http://"))
                || c.endpoint.contains(['@', '?', '#'])
                || c.region.is_empty()
                || c.bucket.is_empty()
                || c.access_key_id.is_empty()
                || c.prefix.split('/').any(|p| p == "..")
                || input.secret.as_ref().is_some_and(|s| s.is_empty())
            {
                return Err(invalid());
            }
        }
        _ => return Err(invalid()),
    }
    Ok(())
}
pub(super) async fn backend(
    conn: &mut SqliteConnection,
    actor: &Actor,
    id: Option<&str>,
) -> Result<StorageBackend, MaintError> {
    // Réglage global ; l'appelant métier a déjà contrôlé la permission et le site du document.
    actor.require_permission(if actor.can(Permission::DocumentsCreate) {
        Permission::DocumentsCreate
    } else {
        Permission::DocumentsView
    })?;
    read_backend(conn, id).await
}
async fn read_backend(
    conn: &mut SqliteConnection,
    id: Option<&str>,
) -> Result<StorageBackend, MaintError> {
    sqlx::query_as(&format!("SELECT {COLUMNS} FROM storage_backends WHERE deleted_at IS NULL AND ((? IS NULL AND is_active=1) OR id=?)"))
        .bind(id).bind(id).fetch_optional(conn).await?.ok_or_else(|| MaintError::NotFound("Stockage".into()))
}
pub async fn list_storage(
    pool: &SqlitePool,
    actor: &Actor,
) -> Result<Vec<StorageBackend>, MaintError> {
    actor.require_permission(Permission::SettingsView)?;
    Ok(sqlx::query_as(&format!(
        "SELECT {COLUMNS} FROM storage_backends WHERE deleted_at IS NULL ORDER BY created_at,id"
    ))
    .fetch_all(pool)
    .await?)
}
pub async fn save_storage(
    pool: &SqlitePool,
    actor: &Actor,
    files: &FileStore,
    id: Option<&str>,
    patch: Value,
) -> Result<StorageBackend, MaintError> {
    actor.require_permission(Permission::SettingsEdit)?;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let before = if let Some(id) = id {
        Some(read_backend(&mut tx, Some(id)).await?)
    } else {
        None
    };
    let mut value = before
        .as_ref()
        .map(|b| json!({"kind":b.kind,"label":b.label,"config":b.config}))
        .unwrap_or_else(|| json!({}));
    let map = patch
        .as_object()
        .ok_or_else(|| MaintError::BadRequest("Objet JSON attendu".into()))?;
    for (key, v) in map {
        if !["kind", "label", "config", "secret"].contains(&key.as_str()) {
            return Err(MaintError::BadRequest("Champ stockage inconnu".into()));
        }
        value[key] = v.clone();
    }
    let input: StorageInput = serde_json::from_value(value)
        .map_err(|_| MaintError::BadRequest("Stockage incomplet".into()))?;
    validate(&input)?;
    let mut secret = before.as_ref().and_then(|b| b.secret_enc.clone());
    if let Some(ref s) = input.secret {
        secret = Some(files.encrypt(s)?);
    }
    if input.kind == "folder" {
        secret = None;
    }
    if input.kind == "s3" && secret.is_none() {
        return Err(MaintError::BadRequest("Clé secrète S3 requise".into()));
    }
    // Les références historiques doivent garder leur destination et leur pilote.
    if let Some(b) = &before {
        let refs: i64 =
            sqlx::query_scalar("SELECT count(*) FROM document_versions WHERE storage_id=?")
                .bind(&b.id)
                .fetch_one(&mut *tx)
                .await?;
        if refs > 0 && (b.kind != input.kind || b.config != input.config) {
            return Err(MaintError::Conflict(
                "Destination d'un stockage référencé immuable ; créez un nouveau stockage".into(),
            ));
        }
    }
    let uid = id.map(str::to_string).unwrap_or_else(store::id);
    if before.is_none() {
        sqlx::query("INSERT INTO storage_backends(id,kind,label,config,secret_enc,created_at,updated_at) VALUES(?,?,?,?,?,?,?)")
            .bind(&uid).bind(&input.kind).bind(input.label.trim()).bind(input.config.to_string()).bind(secret).bind(store::now()).bind(store::now()).execute(&mut *tx).await?;
    } else {
        sqlx::query("UPDATE storage_backends SET kind=?,label=?,config=?,secret_enc=?,updated_at=? WHERE id=? AND deleted_at IS NULL")
            .bind(&input.kind).bind(input.label.trim()).bind(input.config.to_string()).bind(secret).bind(store::now()).bind(&uid).execute(&mut *tx).await?;
    }
    let saved = read_backend(&mut tx, Some(&uid)).await?;
    if saved.kind == "folder" {
        let result = match files.open(&saved) {
            Ok(store) => probe(store).await,
            Err(e) => Err(e),
        };
        if result.is_err() {
            return Err(MaintError::BadRequest(
                "Le dossier doit exister et être accessible en écriture par le service".into(),
            ));
        }
    }
    store::audit(
        &mut tx,
        actor,
        Audit {
            action: if before.is_none() { "create" } else { "update" },
            entity: "storage_backend",
            entity_id: Some(&uid),
            site_id: None,
            before: before.map(|b| json!(b)),
            after: Some(json!(saved)),
            ip: None,
        },
    )
    .await?;
    tx.commit().await?;
    Ok(saved)
}
pub async fn change_storage(
    pool: &SqlitePool,
    actor: &Actor,
    files: &FileStore,
    id: &str,
    delete: bool,
) -> Result<(), MaintError> {
    actor.require_permission(Permission::SettingsEdit)?;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let before = read_backend(&mut tx, Some(id)).await?;
    if delete {
        let count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM document_versions WHERE storage_id=?")
                .bind(id)
                .fetch_one(&mut *tx)
                .await?;
        if count > 0 || before.is_active {
            return Err(MaintError::Conflict(
                "Stockage actif ou encore référencé".into(),
            ));
        }
        sqlx::query("UPDATE storage_backends SET deleted_at=?,updated_at=? WHERE id=?")
            .bind(store::now())
            .bind(store::now())
            .bind(id)
            .execute(&mut *tx)
            .await?;
    } else {
        probe(files.open(&before)?).await?;
        sqlx::query("UPDATE storage_backends SET is_active=0,updated_at=? WHERE is_active=1")
            .bind(store::now())
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE storage_backends SET is_active=1,updated_at=? WHERE id=? AND deleted_at IS NULL").bind(store::now()).bind(id).execute(&mut *tx).await?;
    }
    let after = if delete {
        None
    } else {
        Some(json!(read_backend(&mut tx, Some(id)).await?))
    };
    store::audit(
        &mut tx,
        actor,
        Audit {
            action: if delete { "delete" } else { "activate" },
            entity: "storage_backend",
            entity_id: Some(id),
            site_id: None,
            before: Some(json!(before)),
            after,
            ip: None,
        },
    )
    .await?;
    tx.commit().await?;
    Ok(())
}
pub async fn test_storage(
    pool: &SqlitePool,
    actor: &Actor,
    files: &FileStore,
    id: &str,
) -> Result<Value, MaintError> {
    actor.require_permission(Permission::SettingsEdit)?;
    let backend = read_backend(&mut *pool.acquire().await?, Some(id)).await?;
    let result = match files.open(&backend) {
        Ok(store) => probe(store).await,
        Err(e) => Err(e),
    };
    Ok(match result {
        Ok(()) => json!({"ok":true,"message":"Écriture, relecture et suppression réussies"}),
        Err(_) => {
            json!({"ok":false,"message":"Stockage inaccessible ; vérifiez le chemin, la connexion et les droits du service"})
        }
    })
}
pub async fn probe(store: Arc<dyn ObjectStore>) -> Result<(), MaintError> {
    let key = ObjectPath::from(format!(".apex-test-{}", store::id()));
    let data = bytes::Bytes::from_static(b"Apex Maint : test de stockage");
    store
        .put(&key, data.clone().into())
        .await
        .map_err(storage_error)?;
    let result = async {
        let read = store
            .get(&key)
            .await
            .map_err(storage_error)?
            .bytes()
            .await
            .map_err(storage_error)?;
        if read != data {
            return Err(MaintError::Unavailable(
                "Relecture du témoin incorrecte".into(),
            ));
        }
        Ok(())
    }
    .await;
    let cleanup = store.delete(&key).await.map_err(storage_error);
    result.and(cleanup)
}

pub(super) async fn put_file(
    store: Arc<dyn ObjectStore>,
    key: &str,
    path: &Path,
) -> Result<(), MaintError> {
    let key = ObjectPath::from(key);
    let mut file = tokio::fs::File::open(path).await.map_err(io_error)?;
    let mut upload = store.put_multipart(&key).await.map_err(storage_error)?;
    let result = async {
        let mut buffer = vec![0; 8 * 1024 * 1024];
        loop {
            let mut len = 0;
            while len < buffer.len() {
                let n = file.read(&mut buffer[len..]).await.map_err(io_error)?;
                if n == 0 {
                    break;
                }
                len += n;
            }
            if len == 0 {
                break;
            }
            upload
                .put_part(bytes::Bytes::copy_from_slice(&buffer[..len]).into())
                .await
                .map_err(storage_error)?;
        }
        upload.complete().await.map_err(storage_error)?;
        Ok(())
    }
    .await;
    if result.is_err() {
        let _ = upload.abort().await;
        let _ = store.delete(&key).await;
    }
    result
}
