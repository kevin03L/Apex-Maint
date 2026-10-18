//! Dépôts métier : filtrage des sites, contrôles d'accès et audit transactionnel.
use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{FromRow, SqliteConnection, SqlitePool};

use crate::{domain::password, Actor, MaintError, Permission, Role};

pub fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}
pub fn id() -> String {
    uuid::Uuid::now_v7().to_string()
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Site {
    pub id: String,
    pub name: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct User {
    pub id: String,
    pub site_id: Option<String>,
    pub username: String,
    pub display_name: String,
    pub role: String,
    pub must_change_password: bool,
    pub active: bool,
    pub last_login_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub locked: bool,
    #[serde(skip_serializing)]
    pub password_hash: String,
    #[serde(skip_serializing)]
    pub locked_until: Option<String>,
}

pub(crate) const USER_COLUMNS: &str = "id,site_id,username,display_name,role,must_change_password,active,last_login_at,created_at,updated_at,password_hash,locked_until,COALESCE(locked_until > strftime('%Y-%m-%dT%H:%M:%fZ','now'),0) AS locked";

pub fn require_admin(actor: &Actor) -> Result<(), MaintError> {
    if actor.is_admin() {
        Ok(())
    } else {
        Err(MaintError::Forbidden("Administrateur requis".into()))
    }
}

pub async fn sites(pool: &SqlitePool, actor: &Actor) -> Result<Vec<Site>, MaintError> {
    Ok(sqlx::query_as("SELECT id,name,created_at,updated_at FROM sites WHERE deleted_at IS NULL AND (? OR id=?) ORDER BY name")
        .bind(actor.is_admin()).bind(&actor.site_id).fetch_all(pool).await?)
}

pub async fn users(pool: &SqlitePool, actor: &Actor) -> Result<Vec<User>, MaintError> {
    actor.require_permission(Permission::UsersView)?;
    Ok(sqlx::query_as(&format!("SELECT {USER_COLUMNS} FROM users WHERE deleted_at IS NULL AND (? OR site_id=?) ORDER BY username COLLATE NOCASE"))
        .bind(actor.is_admin()).bind(&actor.site_id).fetch_all(pool).await?)
}

pub async fn user(
    conn: &mut SqliteConnection,
    actor: &Actor,
    user_id: &str,
) -> Result<User, MaintError> {
    actor.require_permission(Permission::UsersView)?;
    scoped_user(conn, actor, user_id).await
}

pub(crate) async fn scoped_user(
    conn: &mut SqliteConnection,
    actor: &Actor,
    user_id: &str,
) -> Result<User, MaintError> {
    sqlx::query_as(&format!(
        "SELECT {USER_COLUMNS} FROM users WHERE id=? AND deleted_at IS NULL AND (? OR site_id=?)"
    ))
    .bind(user_id)
    .bind(actor.is_admin())
    .bind(&actor.site_id)
    .fetch_optional(conn)
    .await?
    .ok_or_else(|| MaintError::NotFound("Utilisateur".into()))
}

pub struct Audit<'a> {
    pub action: &'a str,
    pub entity: &'a str,
    pub entity_id: Option<&'a str>,
    pub site_id: Option<&'a str>,
    pub before: Option<Value>,
    pub after: Option<Value>,
    pub ip: Option<&'a str>,
}

pub async fn audit(
    conn: &mut SqliteConnection,
    actor: &Actor,
    event: Audit<'_>,
) -> Result<(), MaintError> {
    if !actor.is_admin() && event.site_id != actor.site_id.as_deref() {
        return Err(MaintError::NotFound("Site".into()));
    }
    sqlx::query("INSERT INTO audit_log(id,site_id,user_id,action,entity,entity_id,before_json,after_json,ip,created_at) VALUES(?,?,?,?,?,?,?,?,?,?)")
        .bind(id()).bind(event.site_id).bind(if actor.user_id.is_empty() { None } else { Some(&actor.user_id) })
        .bind(event.action).bind(event.entity).bind(event.entity_id)
        .bind(event.before.map(|v| v.to_string())).bind(event.after.map(|v| v.to_string())).bind(event.ip).bind(now())
        .execute(conn).await?;
    Ok(())
}

pub async fn save_site(
    pool: &SqlitePool,
    actor: &Actor,
    site_id: Option<&str>,
    name: Option<&str>,
    delete: bool,
) -> Result<Value, MaintError> {
    require_admin(actor)?;
    let name = name.map(str::trim);
    if !delete
        && name.is_none_or(|n| {
            n.is_empty() || n.chars().count() > 128 || n.chars().any(char::is_control)
        })
    {
        return Err(MaintError::BadRequest(
            "Le nom du site doit contenir de 1 à 128 caractères".into(),
        ));
    }
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let before: Option<Site> = if let Some(sid) = site_id {
        Some(sqlx::query_as("SELECT id,name,created_at,updated_at FROM sites WHERE id=? AND deleted_at IS NULL AND (? OR id=?)")
            .bind(sid).bind(actor.is_admin()).bind(&actor.site_id).fetch_optional(&mut *tx).await?
            .ok_or_else(|| MaintError::NotFound("Site".into()))?)
    } else {
        None
    };
    let sid = site_id.map(str::to_owned).unwrap_or_else(id);
    if delete {
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM users WHERE site_id=? AND active=1 AND deleted_at IS NULL",
        )
        .bind(&sid)
        .fetch_one(&mut *tx)
        .await?;
        if count > 0 {
            return Err(MaintError::Conflict(
                "Ce site a encore des utilisateurs actifs".into(),
            ));
        }
        sqlx::query("UPDATE sites SET deleted_at=?,updated_at=? WHERE id=?")
            .bind(now())
            .bind(now())
            .bind(&sid)
            .execute(&mut *tx)
            .await?;
    } else if before.is_some() {
        sqlx::query("UPDATE sites SET name=?,updated_at=? WHERE id=?")
            .bind(name)
            .bind(now())
            .bind(&sid)
            .execute(&mut *tx)
            .await?;
    } else {
        sqlx::query("INSERT INTO sites(id,name,created_at,updated_at) VALUES(?,?,?,?)")
            .bind(&sid)
            .bind(name)
            .bind(now())
            .bind(now())
            .execute(&mut *tx)
            .await?;
    }
    let after = if delete {
        Value::Null
    } else {
        json!(
            sqlx::query_as::<_, Site>("SELECT id,name,created_at,updated_at FROM sites WHERE id=?")
                .bind(&sid)
                .fetch_one(&mut *tx)
                .await?
        )
    };
    audit(
        &mut tx,
        actor,
        Audit {
            action: if delete {
                "delete"
            } else if before.is_some() {
                "update"
            } else {
                "create"
            },
            entity: "site",
            entity_id: Some(&sid),
            site_id: Some(&sid),
            before: before.map(|s| json!(s)),
            after: Some(after.clone()),
            ip: None,
        },
    )
    .await?;
    tx.commit().await?;
    Ok(after)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateUser {
    pub username: String,
    pub display_name: String,
    pub role: Role,
    pub site_id: Option<String>,
    pub password: String,
    #[serde(default = "enabled")]
    pub active: bool,
}
fn enabled() -> bool {
    true
}

fn text(value: &str) -> Result<(), MaintError> {
    if value.trim().is_empty() || value.chars().count() > 128 || value.chars().any(char::is_control)
    {
        Err(MaintError::BadRequest(
            "Le texte doit contenir de 1 à 128 caractères".into(),
        ))
    } else {
        Ok(())
    }
}

async fn check_assignment(
    conn: &mut SqliteConnection,
    actor: &Actor,
    role: Role,
    site: Option<&str>,
) -> Result<(), MaintError> {
    if !actor.is_admin() && (role == Role::Admin || site != actor.site_id.as_deref()) {
        return Err(MaintError::Forbidden("Rôle ou site non autorisé".into()));
    }
    if role != Role::Admin && site.is_none() {
        return Err(MaintError::BadRequest("Un site est obligatoire".into()));
    }
    if let Some(sid) = site {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM sites WHERE id=? AND deleted_at IS NULL AND (? OR id=?))",
        )
        .bind(sid)
        .bind(actor.is_admin())
        .bind(&actor.site_id)
        .fetch_one(conn)
        .await?;
        if !exists {
            return Err(MaintError::NotFound("Site".into()));
        }
    }
    Ok(())
}

pub async fn create_user(
    pool: &SqlitePool,
    actor: &Actor,
    input: CreateUser,
) -> Result<User, MaintError> {
    actor.require_permission(Permission::UsersManage)?;
    text(&input.username)?;
    text(&input.display_name)?;
    password::validate_new_password(&input.password, None)?;
    let hash = password::hash(input.password).await?;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    check_assignment(&mut tx, actor, input.role, input.site_id.as_deref()).await?;
    let uid = id();
    sqlx::query("INSERT INTO users(id,site_id,username,display_name,role,password_hash,active,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?)")
        .bind(&uid).bind(&input.site_id).bind(input.username.trim()).bind(input.display_name.trim()).bind(input.role.as_str()).bind(hash).bind(input.active).bind(now()).bind(now()).execute(&mut *tx).await?;
    let saved = scoped_user(&mut tx, actor, &uid).await?;
    audit(
        &mut tx,
        actor,
        Audit {
            action: "create",
            entity: "user",
            entity_id: Some(&uid),
            site_id: saved.site_id.as_deref(),
            before: None,
            after: Some(json!(saved)),
            ip: None,
        },
    )
    .await?;
    tx.commit().await?;
    Ok(saved)
}

pub(crate) async fn revoke(
    conn: &mut SqliteConnection,
    actor: &Actor,
    uid: &str,
    except: Option<&str>,
) -> Result<(), MaintError> {
    // Le filtre est répété ici : une révocation ne traverse jamais la frontière de site.
    sqlx::query("UPDATE sessions SET revoked_at=? WHERE user_id=? AND revoked_at IS NULL AND (? IS NULL OR token_hash<>?) AND EXISTS(SELECT 1 FROM users WHERE id=sessions.user_id AND (? OR site_id=?))")
        .bind(now()).bind(uid).bind(except).bind(except).bind(actor.is_admin()).bind(&actor.site_id).execute(conn).await?;
    Ok(())
}

pub async fn update_user(
    pool: &SqlitePool,
    actor: &Actor,
    uid: &str,
    patch: Value,
    delete: bool,
) -> Result<User, MaintError> {
    actor.require_permission(Permission::UsersManage)?;
    let map = patch
        .as_object()
        .ok_or_else(|| MaintError::BadRequest("Objet JSON attendu".into()))?;
    if map
        .keys()
        .any(|k| !["username", "display_name", "role", "site_id", "active"].contains(&k.as_str()))
    {
        return Err(MaintError::BadRequest("Champ utilisateur inconnu".into()));
    }
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let before = scoped_user(&mut tx, actor, uid).await?;
    if !actor.is_admin() && before.role == "admin" {
        return Err(MaintError::Forbidden(
            "Compte administrateur protégé".into(),
        ));
    }
    let mut next = json!(before);
    for (key, value) in map {
        next[key] = value.clone();
    }
    #[derive(Deserialize)]
    struct Edit {
        username: String,
        display_name: String,
        role: Role,
        site_id: Option<String>,
        active: bool,
    }
    let edit: Edit = serde_json::from_value(next)
        .map_err(|_| MaintError::BadRequest("Champs utilisateur invalides".into()))?;
    text(&edit.username)?;
    text(&edit.display_name)?;
    if !actor.is_admin() {
        check_assignment(&mut tx, actor, edit.role, edit.site_id.as_deref()).await?;
    }
    let demote = before.role == "admin" && edit.role != Role::Admin;
    if uid == actor.user_id
        && (delete || !edit.active || demote || edit.role.as_str() != before.role)
    {
        return Err(MaintError::Conflict(
            "Impossible de se supprimer, se désactiver ou se rétrograder".into(),
        ));
    }
    if before.role == "admin" && before.active && (delete || !edit.active || demote) {
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM users WHERE role='admin' AND active=1 AND deleted_at IS NULL",
        )
        .fetch_one(&mut *tx)
        .await?;
        if count <= 1 {
            return Err(MaintError::Conflict(
                "Le dernier administrateur actif est protégé".into(),
            ));
        }
    }
    if actor.is_admin() {
        check_assignment(&mut tx, actor, edit.role, edit.site_id.as_deref()).await?;
    }
    sqlx::query("UPDATE users SET username=?,display_name=?,role=?,site_id=?,active=?,deleted_at=?,updated_at=? WHERE id=? AND (? OR site_id=?)")
        .bind(edit.username.trim()).bind(edit.display_name.trim()).bind(edit.role.as_str()).bind(&edit.site_id).bind(edit.active)
        .bind(if delete {Some(now())} else {None}).bind(now()).bind(uid).bind(actor.is_admin()).bind(&actor.site_id).execute(&mut *tx).await?;
    if delete || !edit.active || edit.role.as_str() != before.role || edit.site_id != before.site_id
    {
        revoke(&mut tx, actor, uid, None).await?;
    }
    let mut after = before.clone();
    after.username = edit.username.trim().into();
    after.display_name = edit.display_name.trim().into();
    after.role = edit.role.to_string();
    after.site_id = edit.site_id;
    after.active = edit.active;
    after.updated_at = now();
    audit(
        &mut tx,
        actor,
        Audit {
            action: if delete { "delete" } else { "update" },
            entity: "user",
            entity_id: Some(uid),
            site_id: before.site_id.as_deref(),
            before: Some(json!(before)),
            after: Some(if delete { Value::Null } else { json!(after) }),
            ip: None,
        },
    )
    .await?;
    tx.commit().await?;
    Ok(after)
}

pub async fn reset_password(
    pool: &SqlitePool,
    actor: &Actor,
    uid: &str,
    plain: String,
) -> Result<(), MaintError> {
    actor.require_permission(Permission::UsersManage)?;
    password::validate_new_password(&plain, None)?;
    let snapshot = scoped_user(&mut *pool.acquire().await?, actor, uid).await?;
    if !actor.is_admin() && snapshot.role == "admin" {
        return Err(MaintError::Forbidden(
            "Compte administrateur protégé".into(),
        ));
    }
    if password::verify(plain.clone(), Some(snapshot.password_hash.clone())).await? {
        return Err(MaintError::BadRequest(
            "Le nouveau mot de passe doit différer de l'ancien".into(),
        ));
    }
    let hash = password::hash(plain).await?;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let before = scoped_user(&mut tx, actor, uid).await?;
    if before.password_hash != snapshot.password_hash {
        return Err(MaintError::Conflict(
            "Le mot de passe vient d'être modifié, réessayez".into(),
        ));
    }
    if !actor.is_admin() && before.role == "admin" {
        return Err(MaintError::Forbidden(
            "Compte administrateur protégé".into(),
        ));
    }
    sqlx::query("UPDATE users SET password_hash=?,must_change_password=1,failed_attempts=0,locked_until=NULL,updated_at=? WHERE id=? AND (? OR site_id=?)")
        .bind(hash).bind(now()).bind(uid).bind(actor.is_admin()).bind(&actor.site_id).execute(&mut *tx).await?;
    revoke(&mut tx, actor, uid, None).await?;
    let after = scoped_user(&mut tx, actor, uid).await?;
    audit(
        &mut tx,
        actor,
        Audit {
            action: "password_reset",
            entity: "user",
            entity_id: Some(uid),
            site_id: before.site_id.as_deref(),
            before: Some(json!(before)),
            after: Some(json!(after)),
            ip: None,
        },
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

pub async fn permissions(pool: &SqlitePool, actor: &Actor) -> Result<Value, MaintError> {
    require_admin(actor)?;
    let mut matrix = json!({"responsable":[],"technicien":[],"production":[]});
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT role,permission FROM role_permissions ORDER BY role,permission")
            .fetch_all(pool)
            .await?;
    for (role, perm) in rows {
        if let Some(list) = matrix.get_mut(&role).and_then(Value::as_array_mut) {
            list.push(json!(perm));
        }
    }
    Ok(json!({"matrix":matrix,"permissions":Permission::ALL,"keys":Permission::ALL}))
}

pub async fn set_permissions(
    pool: &SqlitePool,
    actor: &Actor,
    role: Role,
    perms: Vec<Permission>,
) -> Result<(), MaintError> {
    require_admin(actor)?;
    if role == Role::Admin {
        return Err(MaintError::BadRequest(
            "Les permissions administrateur sont immuables".into(),
        ));
    }
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let before: Vec<String> = sqlx::query_scalar(
        "SELECT permission FROM role_permissions WHERE role=? ORDER BY permission",
    )
    .bind(role.as_str())
    .fetch_all(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM role_permissions WHERE role=?")
        .bind(role.as_str())
        .execute(&mut *tx)
        .await?;
    for perm in &perms {
        sqlx::query("INSERT OR IGNORE INTO role_permissions(role,permission) VALUES(?,?)")
            .bind(role.as_str())
            .bind(perm.as_str())
            .execute(&mut *tx)
            .await?;
    }
    audit(
        &mut tx,
        actor,
        Audit {
            action: "update",
            entity: "role_permissions",
            entity_id: Some(role.as_str()),
            site_id: None,
            before: Some(json!(before)),
            after: Some(json!(perms)),
            ip: None,
        },
    )
    .await?;
    tx.commit().await?;
    Ok(())
}
