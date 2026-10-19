//! Référentiel technique d'Apex Maint (lignes de production, équipements, emplacements, fournisseurs).
//!
//! Tous les dépôts et services reçoivent un `&Actor`, appliquent le filtrage par site
//! (renvoyant 404 pour les ressources d'un autre site aux non-admins) et consignent
//! chaque opération dans `audit_log` au sein d'une transaction `BEGIN IMMEDIATE`.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{FromRow, QueryBuilder, Sqlite, SqliteConnection, SqlitePool};

use crate::{
    domain::pagination::Cursor,
    domain::sql::{ensure_site_exists, like_pattern, validate_limit},
    store::{self, Audit},
    Actor, MaintError, Permission,
};

/// Type générique pour les champs de mise à jour partielle (PATCH).
///
/// Permet de distinguer :
/// - `Absent` : le champ n'a pas été envoyé dans le JSON (valeur inchangée).
/// - `Null` : le champ a été envoyé avec la valeur `null` (remise à blanc).
/// - `Value(v)` : le champ a été envoyé avec une nouvelle valeur `v`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum PatchField<T> {
    #[default]
    Absent,
    Null,
    Value(T),
}

impl<T> PatchField<T> {
    pub fn is_absent(&self) -> bool {
        matches!(self, Self::Absent)
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for PatchField<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Option::<T>::deserialize(deserializer).map(|opt| match opt {
            Some(v) => PatchField::Value(v),
            None => PatchField::Null,
        })
    }
}

/// Page de résultats avec curseur pour la pagination keyset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
}

/// Référence nommée légère vers une entité liée (ex. ligne ou équipement parent).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, FromRow)]
pub struct NamedRef {
    pub id: String,
    pub code: String,
    pub name: String,
}

// ============================================================================
// 1. LIGNES DE PRODUCTION
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ProductionLine {
    pub id: String,
    pub site_id: String,
    pub code: String,
    pub name: String,
    pub description: Option<String>,
    pub equipment_count: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateProductionLine {
    pub site_id: Option<String>,
    pub code: String,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct UpdateProductionLine {
    pub code: Option<String>,
    pub name: Option<String>,
    #[serde(default)]
    pub description: PatchField<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ProductionLineFilter {
    pub q: Option<String>,
    pub site_id: Option<String>,
    pub cursor: Option<Cursor>,
    pub limit: u32,
}

pub async fn list_production_lines(
    pool: &SqlitePool,
    actor: &Actor,
    filter: ProductionLineFilter,
) -> Result<Page<ProductionLine>, MaintError> {
    actor.require_permission(Permission::ProductionLinesView)?;
    let limit = validate_limit(filter.limit)?;
    let site_scope = resolve_site_id_for_query(actor, filter.site_id.as_deref())?;

    let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(
        r#"
        SELECT 
            pl.id, 
            pl.site_id, 
            pl.code, 
            pl.name, 
            pl.description, 
            pl.created_at, 
            pl.updated_at,
            (SELECT count(*) FROM equipments e WHERE e.site_id = pl.site_id AND e.production_line_id = pl.id AND e.deleted_at IS NULL) AS equipment_count
        FROM production_lines pl
        WHERE pl.deleted_at IS NULL
        "#,
    );

    if let Some(sid) = site_scope {
        qb.push(" AND pl.site_id = ").push_bind(sid);
    }

    if let Some(q) = filter.q.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        let pattern = like_pattern(q);
        qb.push(" AND (pl.code LIKE ")
            .push_bind(pattern.clone())
            .push(" ESCAPE '\\' OR pl.name LIKE ")
            .push_bind(pattern)
            .push(" ESCAPE '\\')");
    }

    if let Some(cursor) = filter.cursor {
        if let Some(sort_val) = cursor.sort_value {
            qb.push(" AND (pl.code COLLATE NOCASE > ")
                .push_bind(sort_val.clone())
                .push(" OR (pl.code COLLATE NOCASE = ")
                .push_bind(sort_val)
                .push(" AND pl.id > ")
                .push_bind(cursor.id)
                .push("))");
        } else {
            qb.push(" AND pl.id > ").push_bind(cursor.id);
        }
    }

    qb.push(" ORDER BY pl.code COLLATE NOCASE ASC, pl.id ASC LIMIT ")
        .push_bind((limit + 1) as i64);

    let mut rows: Vec<ProductionLine> = qb.build_query_as().fetch_all(pool).await?;

    let next_cursor = if rows.len() > limit as usize {
        rows.truncate(limit as usize);
        rows.last()
            .map(|last| Cursor::with_sort_value(&last.id, &last.code).encode())
    } else {
        None
    };

    Ok(Page {
        items: rows,
        next_cursor,
    })
}

pub async fn get_production_line(
    pool: &SqlitePool,
    actor: &Actor,
    id: &str,
) -> Result<ProductionLine, MaintError> {
    actor.require_permission(Permission::ProductionLinesView)?;
    let mut conn = pool.acquire().await?;
    scoped_production_line(&mut conn, actor, id).await
}

async fn scoped_production_line(
    conn: &mut SqliteConnection,
    actor: &Actor,
    id: &str,
) -> Result<ProductionLine, MaintError> {
    let row: Option<ProductionLine> = sqlx::query_as(
        r#"
        SELECT 
            pl.id, 
            pl.site_id, 
            pl.code, 
            pl.name, 
            pl.description, 
            pl.created_at, 
            pl.updated_at,
            (SELECT count(*) FROM equipments e WHERE e.site_id = pl.site_id AND e.production_line_id = pl.id AND e.deleted_at IS NULL) AS equipment_count
        FROM production_lines pl
        WHERE pl.id = ? AND pl.deleted_at IS NULL AND (? OR pl.site_id = ?)
        "#,
    )
    .bind(id)
    .bind(actor.is_admin())
    .bind(&actor.site_id)
    .fetch_optional(conn)
    .await?;

    row.ok_or_else(|| MaintError::NotFound("Ligne de production".into()))
}

pub async fn create_production_line(
    pool: &SqlitePool,
    actor: &Actor,
    input: CreateProductionLine,
    ip: Option<&str>,
) -> Result<ProductionLine, MaintError> {
    actor.require_permission(Permission::ProductionLinesCreate)?;
    let site_id = resolve_site_id_for_create(actor, input.site_id.as_deref())?;
    let code = validate_code(&input.code, "Le code de ligne de production")?;
    let name = validate_name(&input.name, "Le nom de ligne de production")?;
    let description = clean_opt_string(input.description, 1024, "La description")?;

    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    ensure_site_exists(&mut tx, &site_id).await?;

    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM production_lines WHERE site_id = ? AND code = ? COLLATE NOCASE AND deleted_at IS NULL",
    )
    .bind(&site_id)
    .bind(&code)
    .fetch_one(&mut *tx)
    .await?;

    if count > 0 {
        return Err(MaintError::Conflict(
            "Une ligne de production avec ce code existe déjà sur ce site".into(),
        ));
    }

    let line_id = store::id();
    let now = store::now();

    sqlx::query(
        r#"
        INSERT INTO production_lines (id, site_id, code, name, description, created_at, updated_at, deleted_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, NULL)
        "#,
    )
    .bind(&line_id)
    .bind(&site_id)
    .bind(&code)
    .bind(&name)
    .bind(&description)
    .bind(&now)
    .bind(&now)
    .execute(&mut *tx)
    .await?;

    let line = ProductionLine {
        id: line_id.clone(),
        site_id: site_id.clone(),
        code,
        name,
        description,
        equipment_count: 0,
        created_at: now.clone(),
        updated_at: now,
    };

    store::audit(
        &mut tx,
        actor,
        Audit {
            action: "create",
            entity: "production_line",
            entity_id: Some(&line_id),
            site_id: Some(&site_id),
            before: None,
            after: Some(json!(line)),
            ip,
        },
    )
    .await?;

    tx.commit().await?;
    Ok(line)
}

pub async fn update_production_line(
    pool: &SqlitePool,
    actor: &Actor,
    id: &str,
    input: UpdateProductionLine,
    ip: Option<&str>,
) -> Result<ProductionLine, MaintError> {
    actor.require_permission(Permission::ProductionLinesEdit)?;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let current = scoped_production_line(&mut tx, actor, id).await?;

    let new_code = match input.code {
        Some(c) => Some(validate_code(&c, "Le code de ligne de production")?),
        None => None,
    };
    let new_name = match input.name {
        Some(n) => Some(validate_name(&n, "Le nom de ligne de production")?),
        None => None,
    };

    let effective_code = new_code.as_deref().unwrap_or(&current.code);
    let effective_name = new_name.as_deref().unwrap_or(&current.name);

    if effective_code != current.code {
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM production_lines WHERE site_id = ? AND code = ? COLLATE NOCASE AND id != ? AND deleted_at IS NULL",
        )
        .bind(&current.site_id)
        .bind(effective_code)
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;

        if count > 0 {
            return Err(MaintError::Conflict(
                "Une ligne de production avec ce code existe déjà sur ce site".into(),
            ));
        }
    }

    let mut effective_description = current.description.clone();
    match input.description {
        PatchField::Absent => {}
        PatchField::Null => effective_description = None,
        PatchField::Value(d) => {
            effective_description = clean_opt_string(Some(d), 1024, "La description")?;
        }
    }

    let now = store::now();

    sqlx::query(
        r#"
        UPDATE production_lines 
        SET code = ?, name = ?, description = ?, updated_at = ?
        WHERE id = ? AND deleted_at IS NULL
        "#,
    )
    .bind(effective_code)
    .bind(effective_name)
    .bind(&effective_description)
    .bind(&now)
    .bind(id)
    .execute(&mut *tx)
    .await?;

    let updated = ProductionLine {
        id: current.id.clone(),
        site_id: current.site_id.clone(),
        code: effective_code.to_string(),
        name: effective_name.to_string(),
        description: effective_description,
        equipment_count: current.equipment_count,
        created_at: current.created_at.clone(),
        updated_at: now,
    };

    store::audit(
        &mut tx,
        actor,
        Audit {
            action: "update",
            entity: "production_line",
            entity_id: Some(id),
            site_id: Some(&current.site_id),
            before: Some(json!(current)),
            after: Some(json!(updated)),
            ip,
        },
    )
    .await?;

    tx.commit().await?;
    Ok(updated)
}

pub async fn delete_production_line(
    pool: &SqlitePool,
    actor: &Actor,
    id: &str,
    ip: Option<&str>,
) -> Result<(), MaintError> {
    actor.require_permission(Permission::ProductionLinesDelete)?;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let current = scoped_production_line(&mut tx, actor, id).await?;

    let eq_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM equipments WHERE production_line_id = ? AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;

    if eq_count > 0 {
        return Err(MaintError::Conflict(
            "Impossible de supprimer une ligne de production contenant des équipements".into(),
        ));
    }

    let now = store::now();

    sqlx::query(
        "UPDATE production_lines SET deleted_at = ?, updated_at = ? WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(&now)
    .bind(&now)
    .bind(id)
    .execute(&mut *tx)
    .await?;

    store::audit(
        &mut tx,
        actor,
        Audit {
            action: "delete",
            entity: "production_line",
            entity_id: Some(id),
            site_id: Some(&current.site_id),
            before: Some(json!(current)),
            after: None,
            ip,
        },
    )
    .await?;

    tx.commit().await?;
    Ok(())
}

// ============================================================================
// 2. ÉQUIPEMENTS
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Equipment {
    pub id: String,
    pub site_id: String,
    pub code: String,
    pub name: String,
    pub status: String,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    pub serial_number: Option<String>,
    pub commissioned_on: Option<String>,
    pub description: Option<String>,
    pub production_line_id: Option<String>,
    pub parent_id: Option<String>,
    /// Référence de l'équipement côté Apex Supervision (phase 10).
    pub external_ref: Option<String>,
    pub production_line: Option<NamedRef>,
    pub parent: Option<NamedRef>,
    pub children_count: i64,
    pub documents_count: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateEquipment {
    pub site_id: Option<String>,
    pub code: String,
    pub name: String,
    pub status: Option<String>,
    pub production_line_id: Option<String>,
    pub parent_id: Option<String>,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    pub serial_number: Option<String>,
    pub commissioned_on: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct UpdateEquipment {
    pub code: Option<String>,
    pub name: Option<String>,
    pub status: Option<String>,
    #[serde(default)]
    pub production_line_id: PatchField<String>,
    #[serde(default)]
    pub parent_id: PatchField<String>,
    #[serde(default)]
    pub manufacturer: PatchField<String>,
    #[serde(default)]
    pub model: PatchField<String>,
    #[serde(default)]
    pub serial_number: PatchField<String>,
    #[serde(default)]
    pub commissioned_on: PatchField<String>,
    #[serde(default)]
    pub description: PatchField<String>,
}

#[derive(Debug, Clone, Default)]
pub struct EquipmentFilter {
    pub q: Option<String>,
    pub site_id: Option<String>,
    pub status: Option<String>,
    pub production_line_id: Option<String>,
    pub parent_id: Option<String>,
    pub cursor: Option<Cursor>,
    pub limit: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EquipmentTreeNode {
    pub id: String,
    pub site_id: String,
    pub code: String,
    pub name: String,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
    pub children: Vec<EquipmentTreeNode>,
}

#[derive(Debug, FromRow)]
struct RawEquipmentRow {
    id: String,
    site_id: String,
    code: String,
    name: String,
    status: String,
    manufacturer: Option<String>,
    model: Option<String>,
    serial_number: Option<String>,
    commissioned_on: Option<String>,
    description: Option<String>,
    production_line_id: Option<String>,
    parent_id: Option<String>,
    external_ref: Option<String>,
    created_at: String,
    updated_at: String,
    pl_id: Option<String>,
    pl_code: Option<String>,
    pl_name: Option<String>,
    pe_id: Option<String>,
    pe_code: Option<String>,
    pe_name: Option<String>,
    children_count: i64,
    documents_count: i64,
}

fn map_raw_equipment(r: RawEquipmentRow) -> Equipment {
    let production_line = match (r.pl_id, r.pl_code, r.pl_name) {
        (Some(id), Some(code), Some(name)) => Some(NamedRef { id, code, name }),
        _ => None,
    };
    let parent = match (r.pe_id, r.pe_code, r.pe_name) {
        (Some(id), Some(code), Some(name)) => Some(NamedRef { id, code, name }),
        _ => None,
    };
    Equipment {
        id: r.id,
        site_id: r.site_id,
        code: r.code,
        name: r.name,
        status: r.status,
        manufacturer: r.manufacturer,
        model: r.model,
        serial_number: r.serial_number,
        commissioned_on: r.commissioned_on,
        description: r.description,
        production_line_id: r.production_line_id,
        parent_id: r.parent_id,
        external_ref: r.external_ref,
        production_line,
        parent,
        children_count: r.children_count,
        documents_count: r.documents_count,
        created_at: r.created_at,
        updated_at: r.updated_at,
    }
}

pub async fn list_equipments(
    pool: &SqlitePool,
    actor: &Actor,
    filter: EquipmentFilter,
) -> Result<Page<Equipment>, MaintError> {
    actor.require_permission(Permission::EquipmentsView)?;
    let limit = validate_limit(filter.limit)?;
    let site_scope = resolve_site_id_for_query(actor, filter.site_id.as_deref())?;

    if let Some(st) = filter.status.as_deref() {
        validate_equipment_status(st)?;
    }

    let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(
        r#"
        SELECT 
            e.id, 
            e.site_id, 
            e.code, 
            e.name, 
            e.status, 
            e.manufacturer, 
            e.model, 
            e.serial_number, 
            e.commissioned_on, 
            e.description, 
            e.production_line_id, 
            e.parent_id, 
            e.external_ref, 
            e.created_at, 
            e.updated_at,
            pl.id AS pl_id,
            pl.code AS pl_code,
            pl.name AS pl_name,
            pe.id AS pe_id,
            pe.code AS pe_code,
            pe.name AS pe_name,
            (SELECT count(*) FROM equipments c WHERE c.site_id = e.site_id AND c.parent_id = e.id AND c.deleted_at IS NULL) AS children_count,
            (SELECT count(*) FROM documents d WHERE d.site_id=e.site_id AND d.entity_type='equipment' AND d.entity_id=e.id AND d.deleted_at IS NULL) AS documents_count
        FROM equipments e
        LEFT JOIN production_lines pl ON pl.id = e.production_line_id AND pl.deleted_at IS NULL
        LEFT JOIN equipments pe ON pe.id = e.parent_id AND pe.deleted_at IS NULL
        WHERE e.deleted_at IS NULL
        "#,
    );

    if let Some(sid) = site_scope {
        qb.push(" AND e.site_id = ").push_bind(sid);
    }

    if let Some(st) = filter.status.as_deref() {
        qb.push(" AND e.status = ").push_bind(st);
    }

    if let Some(plid) = filter
        .production_line_id
        .as_deref()
        .filter(|s| !s.is_empty())
    {
        qb.push(" AND e.production_line_id = ").push_bind(plid);
    }

    if let Some(pid) = filter.parent_id.as_deref() {
        if pid == "root" {
            qb.push(" AND e.parent_id IS NULL");
        } else if !pid.is_empty() {
            qb.push(" AND e.parent_id = ").push_bind(pid);
        }
    }

    if let Some(q) = filter.q.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        let pattern = like_pattern(q);
        qb.push(" AND (e.code LIKE ")
            .push_bind(pattern.clone())
            .push(" ESCAPE '\\' OR e.name LIKE ")
            .push_bind(pattern)
            .push(" ESCAPE '\\')");
    }

    if let Some(cursor) = filter.cursor {
        if let Some(sort_val) = cursor.sort_value {
            qb.push(" AND (e.code COLLATE NOCASE > ")
                .push_bind(sort_val.clone())
                .push(" OR (e.code COLLATE NOCASE = ")
                .push_bind(sort_val)
                .push(" AND e.id > ")
                .push_bind(cursor.id)
                .push("))");
        } else {
            qb.push(" AND e.id > ").push_bind(cursor.id);
        }
    }

    qb.push(" ORDER BY e.code COLLATE NOCASE ASC, e.id ASC LIMIT ")
        .push_bind((limit + 1) as i64);

    let raw_rows: Vec<RawEquipmentRow> = qb.build_query_as().fetch_all(pool).await?;
    let mut rows: Vec<Equipment> = raw_rows.into_iter().map(map_raw_equipment).collect();

    let next_cursor = if rows.len() > limit as usize {
        rows.truncate(limit as usize);
        rows.last()
            .map(|last| Cursor::with_sort_value(&last.id, &last.code).encode())
    } else {
        None
    };

    Ok(Page {
        items: rows,
        next_cursor,
    })
}

pub async fn get_equipment(
    pool: &SqlitePool,
    actor: &Actor,
    id: &str,
) -> Result<Equipment, MaintError> {
    actor.require_permission(Permission::EquipmentsView)?;
    let mut conn = pool.acquire().await?;
    scoped_equipment(&mut conn, actor, id).await
}

async fn scoped_equipment(
    conn: &mut SqliteConnection,
    actor: &Actor,
    id: &str,
) -> Result<Equipment, MaintError> {
    let row: Option<RawEquipmentRow> = sqlx::query_as(
        r#"
        SELECT 
            e.id, 
            e.site_id, 
            e.code, 
            e.name, 
            e.status, 
            e.manufacturer, 
            e.model, 
            e.serial_number, 
            e.commissioned_on, 
            e.description, 
            e.production_line_id, 
            e.parent_id, 
            e.external_ref, 
            e.created_at, 
            e.updated_at,
            pl.id AS pl_id,
            pl.code AS pl_code,
            pl.name AS pl_name,
            pe.id AS pe_id,
            pe.code AS pe_code,
            pe.name AS pe_name,
            (SELECT count(*) FROM equipments c WHERE c.site_id = e.site_id AND c.parent_id = e.id AND c.deleted_at IS NULL) AS children_count,
            (SELECT count(*) FROM documents d WHERE d.site_id=e.site_id AND d.entity_type='equipment' AND d.entity_id=e.id AND d.deleted_at IS NULL) AS documents_count
        FROM equipments e
        LEFT JOIN production_lines pl ON pl.id = e.production_line_id AND pl.deleted_at IS NULL
        LEFT JOIN equipments pe ON pe.id = e.parent_id AND pe.deleted_at IS NULL
        WHERE e.id = ? AND e.deleted_at IS NULL AND (? OR e.site_id = ?)
        "#,
    )
    .bind(id)
    .bind(actor.is_admin())
    .bind(&actor.site_id)
    .fetch_optional(conn)
    .await?;

    row.map(map_raw_equipment)
        .ok_or_else(|| MaintError::NotFound("Équipement".into()))
}

#[derive(Debug, FromRow)]
struct RawEquipmentTreeNode {
    id: String,
    site_id: String,
    code: String,
    name: String,
    status: String,
    parent_id: Option<String>,
    created_at: String,
    updated_at: String,
}

pub async fn get_equipments_tree(
    pool: &SqlitePool,
    actor: &Actor,
    site_id: Option<&str>,
) -> Result<Vec<EquipmentTreeNode>, MaintError> {
    actor.require_permission(Permission::EquipmentsView)?;
    let site_scope = resolve_site_id_for_query(actor, site_id)?;

    let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(
        r#"
        SELECT id, site_id, code, name, status, parent_id, created_at, updated_at
        FROM equipments
        WHERE deleted_at IS NULL
        "#,
    );

    if let Some(sid) = site_scope {
        qb.push(" AND site_id = ").push_bind(sid);
    }

    qb.push(" ORDER BY code COLLATE NOCASE ASC, id ASC");

    let raw_nodes: Vec<RawEquipmentTreeNode> = qb.build_query_as().fetch_all(pool).await?;

    Ok(build_equipment_tree(raw_nodes))
}

fn build_equipment_tree(raw_nodes: Vec<RawEquipmentTreeNode>) -> Vec<EquipmentTreeNode> {
    use std::collections::HashMap;
    let mut by_parent: HashMap<Option<String>, Vec<RawEquipmentTreeNode>> = HashMap::new();
    for node in raw_nodes {
        by_parent
            .entry(node.parent_id.clone())
            .or_default()
            .push(node);
    }

    fn assemble(
        parent_id: Option<String>,
        by_parent: &mut HashMap<Option<String>, Vec<RawEquipmentTreeNode>>,
    ) -> Vec<EquipmentTreeNode> {
        let nodes = by_parent.remove(&parent_id).unwrap_or_default();
        nodes
            .into_iter()
            .map(|n| {
                let node_id = n.id.clone();
                let children = assemble(Some(node_id), by_parent);
                EquipmentTreeNode {
                    id: n.id,
                    site_id: n.site_id,
                    code: n.code,
                    name: n.name,
                    status: n.status,
                    created_at: n.created_at,
                    updated_at: n.updated_at,
                    children,
                }
            })
            .collect()
    }

    let mut roots = assemble(None, &mut by_parent);
    // Rattachement des éventuels nœuds orphelins (ex. parent supprimé) au premier niveau
    for (_, remaining) in by_parent {
        for n in remaining {
            roots.push(EquipmentTreeNode {
                id: n.id,
                site_id: n.site_id,
                code: n.code,
                name: n.name,
                status: n.status,
                created_at: n.created_at,
                updated_at: n.updated_at,
                children: Vec::new(),
            });
        }
    }
    roots
}

pub async fn create_equipment(
    pool: &SqlitePool,
    actor: &Actor,
    input: CreateEquipment,
    ip: Option<&str>,
) -> Result<Equipment, MaintError> {
    actor.require_permission(Permission::EquipmentsCreate)?;
    let site_id = resolve_site_id_for_create(actor, input.site_id.as_deref())?;
    let code = validate_code(&input.code, "Le code de l'équipement")?;
    let name = validate_name(&input.name, "Le nom de l'équipement")?;
    let status = match input.status.as_deref() {
        Some(s) => validate_equipment_status(s)?.to_string(),
        None => "operational".to_string(),
    };

    let pl_id = clean_opt_id(input.production_line_id);
    let parent_id = clean_opt_id(input.parent_id);
    let manufacturer = clean_opt_string(input.manufacturer, 128, "Le fabricant")?;
    let model = clean_opt_string(input.model, 128, "Le modèle")?;
    let serial_number = clean_opt_string(input.serial_number, 128, "Le numéro de série")?;
    let commissioned_on = validate_commissioned_on(input.commissioned_on.as_deref())?;
    let description = clean_opt_string(input.description, 2048, "La description")?;

    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    ensure_site_exists(&mut tx, &site_id).await?;

    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM equipments WHERE site_id = ? AND code = ? COLLATE NOCASE AND deleted_at IS NULL",
    )
    .bind(&site_id)
    .bind(&code)
    .fetch_one(&mut *tx)
    .await?;

    if count > 0 {
        return Err(MaintError::Conflict(
            "Un équipement avec ce code existe déjà sur ce site".into(),
        ));
    }

    validate_equipment_parent_and_line(
        &mut tx,
        None,
        &site_id,
        pl_id.as_deref(),
        parent_id.as_deref(),
    )
    .await?;

    let equipment_id = store::id();
    let now = store::now();

    sqlx::query(
        r#"
        INSERT INTO equipments (
            id, site_id, code, name, production_line_id, parent_id, status,
            manufacturer, model, serial_number, commissioned_on, description,
            created_at, updated_at, deleted_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NULL)
        "#,
    )
    .bind(&equipment_id)
    .bind(&site_id)
    .bind(&code)
    .bind(&name)
    .bind(&pl_id)
    .bind(&parent_id)
    .bind(&status)
    .bind(&manufacturer)
    .bind(&model)
    .bind(&serial_number)
    .bind(&commissioned_on)
    .bind(&description)
    .bind(&now)
    .bind(&now)
    .execute(&mut *tx)
    .await?;

    let created = scoped_equipment(&mut tx, actor, &equipment_id).await?;

    store::audit(
        &mut tx,
        actor,
        Audit {
            action: "create",
            entity: "equipment",
            entity_id: Some(&equipment_id),
            site_id: Some(&site_id),
            before: None,
            after: Some(json!(created)),
            ip,
        },
    )
    .await?;

    tx.commit().await?;
    Ok(created)
}

pub async fn update_equipment(
    pool: &SqlitePool,
    actor: &Actor,
    id: &str,
    input: UpdateEquipment,
    ip: Option<&str>,
) -> Result<Equipment, MaintError> {
    actor.require_permission(Permission::EquipmentsEdit)?;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let current = scoped_equipment(&mut tx, actor, id).await?;

    let new_code = match input.code {
        Some(c) => Some(validate_code(&c, "Le code de l'équipement")?),
        None => None,
    };
    let new_name = match input.name {
        Some(n) => Some(validate_name(&n, "Le nom de l'équipement")?),
        None => None,
    };
    let new_status = match input.status.as_deref() {
        Some(s) => Some(validate_equipment_status(s)?.to_string()),
        None => None,
    };

    let effective_code = new_code.as_deref().unwrap_or(&current.code);
    let effective_name = new_name.as_deref().unwrap_or(&current.name);
    let effective_status = new_status.as_deref().unwrap_or(&current.status);

    if effective_code != current.code {
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM equipments WHERE site_id = ? AND code = ? COLLATE NOCASE AND id != ? AND deleted_at IS NULL",
        )
        .bind(&current.site_id)
        .bind(effective_code)
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;

        if count > 0 {
            return Err(MaintError::Conflict(
                "Un équipement avec ce code existe déjà sur ce site".into(),
            ));
        }
    }

    let mut effective_line_id = current.production_line_id.clone();
    match input.production_line_id {
        PatchField::Absent => {}
        PatchField::Null => effective_line_id = None,
        PatchField::Value(lid) => effective_line_id = clean_opt_id(Some(lid)),
    }

    let mut effective_parent_id = current.parent_id.clone();
    match input.parent_id {
        PatchField::Absent => {}
        PatchField::Null => effective_parent_id = None,
        PatchField::Value(pid) => effective_parent_id = clean_opt_id(Some(pid)),
    }

    validate_equipment_parent_and_line(
        &mut tx,
        Some(id),
        &current.site_id,
        effective_line_id.as_deref(),
        effective_parent_id.as_deref(),
    )
    .await?;

    let mut effective_mfr = current.manufacturer.clone();
    match input.manufacturer {
        PatchField::Absent => {}
        PatchField::Null => effective_mfr = None,
        PatchField::Value(v) => effective_mfr = clean_opt_string(Some(v), 128, "Le fabricant")?,
    }

    let mut effective_model = current.model.clone();
    match input.model {
        PatchField::Absent => {}
        PatchField::Null => effective_model = None,
        PatchField::Value(v) => effective_model = clean_opt_string(Some(v), 128, "Le modèle")?,
    }

    let mut effective_sn = current.serial_number.clone();
    match input.serial_number {
        PatchField::Absent => {}
        PatchField::Null => effective_sn = None,
        PatchField::Value(v) => {
            effective_sn = clean_opt_string(Some(v), 128, "Le numéro de série")?;
        }
    }

    let mut effective_comm = current.commissioned_on.clone();
    match input.commissioned_on {
        PatchField::Absent => {}
        PatchField::Null => effective_comm = None,
        PatchField::Value(v) => {
            effective_comm = validate_commissioned_on(Some(&v))?;
        }
    }

    let mut effective_desc = current.description.clone();
    match input.description {
        PatchField::Absent => {}
        PatchField::Null => effective_desc = None,
        PatchField::Value(v) => {
            effective_desc = clean_opt_string(Some(v), 2048, "La description")?;
        }
    }

    let now = store::now();

    sqlx::query(
        r#"
        UPDATE equipments 
        SET code = ?, name = ?, status = ?, production_line_id = ?, parent_id = ?,
            manufacturer = ?, model = ?, serial_number = ?, commissioned_on = ?,
            description = ?, updated_at = ?
        WHERE id = ? AND deleted_at IS NULL
        "#,
    )
    .bind(effective_code)
    .bind(effective_name)
    .bind(effective_status)
    .bind(&effective_line_id)
    .bind(&effective_parent_id)
    .bind(&effective_mfr)
    .bind(&effective_model)
    .bind(&effective_sn)
    .bind(&effective_comm)
    .bind(&effective_desc)
    .bind(&now)
    .bind(id)
    .execute(&mut *tx)
    .await?;

    let updated = scoped_equipment(&mut tx, actor, id).await?;

    store::audit(
        &mut tx,
        actor,
        Audit {
            action: "update",
            entity: "equipment",
            entity_id: Some(id),
            site_id: Some(&current.site_id),
            before: Some(json!(current)),
            after: Some(json!(updated)),
            ip,
        },
    )
    .await?;

    tx.commit().await?;
    Ok(updated)
}

pub async fn delete_equipment(
    pool: &SqlitePool,
    actor: &Actor,
    id: &str,
    ip: Option<&str>,
) -> Result<(), MaintError> {
    actor.require_permission(Permission::EquipmentsDelete)?;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let current = scoped_equipment(&mut tx, actor, id).await?;

    let children_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM equipments WHERE parent_id = ? AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;

    if children_count > 0 {
        return Err(MaintError::Conflict(
            "Impossible de supprimer un équipement ayant des sous-équipements".into(),
        ));
    }

    let active_plans: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM maintenance_plans WHERE equipment_id = ? AND deleted_at IS NULL AND active = 1",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;

    if active_plans > 0 {
        return Err(MaintError::Conflict(
            "Impossible de supprimer un équipement ayant des plannings de maintenance actifs"
                .into(),
        ));
    }

    let now = store::now();

    sqlx::query(
        "UPDATE equipments SET deleted_at = ?, updated_at = ? WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(&now)
    .bind(&now)
    .bind(id)
    .execute(&mut *tx)
    .await?;

    store::audit(
        &mut tx,
        actor,
        Audit {
            action: "delete",
            entity: "equipment",
            entity_id: Some(id),
            site_id: Some(&current.site_id),
            before: Some(json!(current)),
            after: None,
            ip,
        },
    )
    .await?;

    tx.commit().await?;
    Ok(())
}

async fn validate_equipment_parent_and_line(
    tx: &mut SqliteConnection,
    equipment_id: Option<&str>,
    site_id: &str,
    production_line_id: Option<&str>,
    parent_id: Option<&str>,
) -> Result<(), MaintError> {
    if let Some(line_id) = production_line_id {
        let line_site: Option<String> = sqlx::query_scalar(
            "SELECT site_id FROM production_lines WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(line_id)
        .fetch_optional(&mut *tx)
        .await?;

        match line_site {
            Some(s) if s == site_id => {}
            _ => {
                return Err(MaintError::BadRequest(
                    "La ligne de production doit appartenir au même site".into(),
                ))
            }
        }
    }

    if let Some(pid) = parent_id {
        if let Some(eid) = equipment_id {
            if pid == eid {
                return Err(MaintError::BadRequest(
                    "Un équipement ne peut pas être son propre parent".into(),
                ));
            }
        }

        let parent_site: Option<String> = sqlx::query_scalar(
            "SELECT site_id FROM equipments WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(pid)
        .fetch_optional(&mut *tx)
        .await?;

        match parent_site {
            Some(s) if s == site_id => {}
            _ => {
                return Err(MaintError::BadRequest(
                    "L'équipement parent doit appartenir au même site".into(),
                ))
            }
        }

        // Vérification de cycle
        if let Some(eid) = equipment_id {
            let cycle_count: i64 = sqlx::query_scalar(
                r#"
                WITH RECURSIVE ancestors(id, parent_id, depth) AS (
                    SELECT id, parent_id, 1 FROM equipments WHERE id = ? AND deleted_at IS NULL
                    UNION ALL
                    SELECT e.id, e.parent_id, a.depth + 1
                    FROM equipments e
                    JOIN ancestors a ON e.id = a.parent_id
                    WHERE e.deleted_at IS NULL AND a.depth < 20
                )
                SELECT count(*) FROM ancestors WHERE id = ?
                "#,
            )
            .bind(pid)
            .bind(eid)
            .fetch_one(&mut *tx)
            .await?;

            if cycle_count > 0 {
                return Err(MaintError::BadRequest(
                    "Cycle détecté dans l'arborescence des équipements".into(),
                ));
            }
        }

        // Vérification de la profondeur maximale (8 niveaux)
        let parent_depth: i64 = sqlx::query_scalar(
            r#"
            WITH RECURSIVE ancestors(id, parent_id, depth) AS (
                SELECT id, parent_id, 1 FROM equipments WHERE id = ? AND deleted_at IS NULL
                UNION ALL
                SELECT e.id, e.parent_id, a.depth + 1
                FROM equipments e
                JOIN ancestors a ON e.id = a.parent_id
                WHERE e.deleted_at IS NULL AND a.depth < 20
            )
            SELECT max(depth) FROM ancestors
            "#,
        )
        .bind(pid)
        .fetch_one(&mut *tx)
        .await?;

        let subtree_height: i64 = if let Some(eid) = equipment_id {
            sqlx::query_scalar(
                r#"
                WITH RECURSIVE descendants(id, height) AS (
                    SELECT id, 1 FROM equipments WHERE id = ? AND deleted_at IS NULL
                    UNION ALL
                    SELECT e.id, d.height + 1
                    FROM equipments e
                    JOIN descendants d ON e.parent_id = d.id
                    WHERE e.deleted_at IS NULL AND d.height < 20
                )
                SELECT COALESCE(max(height), 1) FROM descendants
                "#,
            )
            .bind(eid)
            .fetch_one(&mut *tx)
            .await?
        } else {
            1
        };

        if parent_depth + subtree_height > 8 {
            return Err(MaintError::BadRequest(
                "La profondeur maximale de l'arborescence des équipements est de 8 niveaux".into(),
            ));
        }
    }

    Ok(())
}

fn validate_equipment_status(status: &str) -> Result<&str, MaintError> {
    match status.trim() {
        "operational" | "degraded" | "out_of_service" => Ok(status.trim()),
        _ => Err(MaintError::BadRequest(format!(
            "Statut d'équipement invalide : « {status} » (attendu: operational, degraded, out_of_service)"
        ))),
    }
}

fn validate_commissioned_on(value: Option<&str>) -> Result<Option<String>, MaintError> {
    match value.map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => {
            NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| {
                MaintError::BadRequest(
                    "Format de date de mise en service invalide (attendu: AAAA-MM-JJ)".into(),
                )
            })?;
            Ok(Some(s.to_string()))
        }
        None => Ok(None),
    }
}

// ============================================================================
// 3. EMPLACEMENTS
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Location {
    pub id: String,
    pub site_id: String,
    pub code: String,
    pub name: String,
    pub kind: String,
    pub parent_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateLocation {
    pub site_id: Option<String>,
    pub code: String,
    pub name: String,
    pub kind: String,
    pub parent_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct UpdateLocation {
    pub code: Option<String>,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct LocationFilter {
    pub q: Option<String>,
    pub site_id: Option<String>,
    pub kind: Option<String>,
    pub parent_id: Option<String>,
    pub cursor: Option<Cursor>,
    pub limit: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocationTreeNode {
    pub id: String,
    pub site_id: String,
    pub code: String,
    pub name: String,
    pub kind: String,
    pub created_at: String,
    pub updated_at: String,
    pub children: Vec<LocationTreeNode>,
}

pub async fn list_locations(
    pool: &SqlitePool,
    actor: &Actor,
    filter: LocationFilter,
) -> Result<Page<Location>, MaintError> {
    actor.require_permission(Permission::LocationsView)?;
    let limit = validate_limit(filter.limit)?;
    let site_scope = resolve_site_id_for_query(actor, filter.site_id.as_deref())?;

    if let Some(k) = filter.kind.as_deref() {
        validate_location_kind(k)?;
    }

    let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(
        r#"
        SELECT id, site_id, code, name, kind, parent_id, created_at, updated_at
        FROM locations
        WHERE deleted_at IS NULL
        "#,
    );

    if let Some(sid) = site_scope {
        qb.push(" AND site_id = ").push_bind(sid);
    }

    if let Some(k) = filter.kind.as_deref() {
        qb.push(" AND kind = ").push_bind(k);
    }

    if let Some(pid) = filter.parent_id.as_deref() {
        if pid == "root" {
            qb.push(" AND parent_id IS NULL");
        } else if !pid.is_empty() {
            qb.push(" AND parent_id = ").push_bind(pid);
        }
    }

    if let Some(q) = filter.q.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        let pattern = like_pattern(q);
        qb.push(" AND (code LIKE ")
            .push_bind(pattern.clone())
            .push(" ESCAPE '\\' OR name LIKE ")
            .push_bind(pattern)
            .push(" ESCAPE '\\')");
    }

    if let Some(cursor) = filter.cursor {
        if let Some(sort_val) = cursor.sort_value {
            qb.push(" AND (code COLLATE NOCASE > ")
                .push_bind(sort_val.clone())
                .push(" OR (code COLLATE NOCASE = ")
                .push_bind(sort_val)
                .push(" AND id > ")
                .push_bind(cursor.id)
                .push("))");
        } else {
            qb.push(" AND id > ").push_bind(cursor.id);
        }
    }

    qb.push(" ORDER BY code COLLATE NOCASE ASC, id ASC LIMIT ")
        .push_bind((limit + 1) as i64);

    let mut rows: Vec<Location> = qb.build_query_as().fetch_all(pool).await?;

    let next_cursor = if rows.len() > limit as usize {
        rows.truncate(limit as usize);
        rows.last()
            .map(|last| Cursor::with_sort_value(&last.id, &last.code).encode())
    } else {
        None
    };

    Ok(Page {
        items: rows,
        next_cursor,
    })
}

pub async fn get_location(
    pool: &SqlitePool,
    actor: &Actor,
    id: &str,
) -> Result<Location, MaintError> {
    actor.require_permission(Permission::LocationsView)?;
    let mut conn = pool.acquire().await?;
    scoped_location(&mut conn, actor, id).await
}

async fn scoped_location(
    conn: &mut SqliteConnection,
    actor: &Actor,
    id: &str,
) -> Result<Location, MaintError> {
    let row: Option<Location> = sqlx::query_as(
        r#"
        SELECT id, site_id, code, name, kind, parent_id, created_at, updated_at
        FROM locations
        WHERE id = ? AND deleted_at IS NULL AND (? OR site_id = ?)
        "#,
    )
    .bind(id)
    .bind(actor.is_admin())
    .bind(&actor.site_id)
    .fetch_optional(conn)
    .await?;

    row.ok_or_else(|| MaintError::NotFound("Emplacement".into()))
}

pub async fn get_locations_tree(
    pool: &SqlitePool,
    actor: &Actor,
    site_id: Option<&str>,
) -> Result<Vec<LocationTreeNode>, MaintError> {
    actor.require_permission(Permission::LocationsView)?;
    let site_scope = resolve_site_id_for_query(actor, site_id)?;

    let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(
        r#"
        SELECT id, site_id, code, name, kind, parent_id, created_at, updated_at
        FROM locations
        WHERE deleted_at IS NULL
        "#,
    );

    if let Some(sid) = site_scope {
        qb.push(" AND site_id = ").push_bind(sid);
    }

    qb.push(" ORDER BY code COLLATE NOCASE ASC, id ASC");

    let raw_nodes: Vec<Location> = qb.build_query_as().fetch_all(pool).await?;

    Ok(build_locations_tree(raw_nodes))
}

fn build_locations_tree(raw_nodes: Vec<Location>) -> Vec<LocationTreeNode> {
    use std::collections::HashMap;
    let mut by_parent: HashMap<Option<String>, Vec<Location>> = HashMap::new();
    for node in raw_nodes {
        by_parent
            .entry(node.parent_id.clone())
            .or_default()
            .push(node);
    }

    fn assemble(
        parent_id: Option<String>,
        by_parent: &mut HashMap<Option<String>, Vec<Location>>,
    ) -> Vec<LocationTreeNode> {
        let nodes = by_parent.remove(&parent_id).unwrap_or_default();
        nodes
            .into_iter()
            .map(|n| {
                let node_id = n.id.clone();
                let children = assemble(Some(node_id), by_parent);
                LocationTreeNode {
                    id: n.id,
                    site_id: n.site_id,
                    code: n.code,
                    name: n.name,
                    kind: n.kind,
                    created_at: n.created_at,
                    updated_at: n.updated_at,
                    children,
                }
            })
            .collect()
    }

    let mut roots = assemble(None, &mut by_parent);
    for (_, remaining) in by_parent {
        for n in remaining {
            roots.push(LocationTreeNode {
                id: n.id,
                site_id: n.site_id,
                code: n.code,
                name: n.name,
                kind: n.kind,
                created_at: n.created_at,
                updated_at: n.updated_at,
                children: Vec::new(),
            });
        }
    }
    roots
}

pub async fn create_location(
    pool: &SqlitePool,
    actor: &Actor,
    input: CreateLocation,
    ip: Option<&str>,
) -> Result<Location, MaintError> {
    actor.require_permission(Permission::LocationsManage)?;
    let site_id = resolve_site_id_for_create(actor, input.site_id.as_deref())?;
    let code = validate_code(&input.code, "Le code d'emplacement")?;
    let name = validate_name(&input.name, "Le nom d'emplacement")?;
    let kind = validate_location_kind(&input.kind)?.to_string();
    let parent_id = clean_opt_id(input.parent_id);

    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    ensure_site_exists(&mut tx, &site_id).await?;

    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM locations WHERE site_id = ? AND code = ? COLLATE NOCASE AND deleted_at IS NULL",
    )
    .bind(&site_id)
    .bind(&code)
    .fetch_one(&mut *tx)
    .await?;

    if count > 0 {
        return Err(MaintError::Conflict(
            "Un emplacement avec ce code existe déjà sur ce site".into(),
        ));
    }

    validate_location_hierarchy(&mut tx, &site_id, &kind, parent_id.as_deref()).await?;

    let location_id = store::id();
    let now = store::now();

    sqlx::query(
        r#"
        INSERT INTO locations (id, site_id, code, name, kind, parent_id, created_at, updated_at, deleted_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, NULL)
        "#,
    )
    .bind(&location_id)
    .bind(&site_id)
    .bind(&code)
    .bind(&name)
    .bind(&kind)
    .bind(&parent_id)
    .bind(&now)
    .bind(&now)
    .execute(&mut *tx)
    .await?;

    let location = Location {
        id: location_id.clone(),
        site_id: site_id.clone(),
        code,
        name,
        kind,
        parent_id,
        created_at: now.clone(),
        updated_at: now,
    };

    store::audit(
        &mut tx,
        actor,
        Audit {
            action: "create",
            entity: "location",
            entity_id: Some(&location_id),
            site_id: Some(&site_id),
            before: None,
            after: Some(json!(location)),
            ip,
        },
    )
    .await?;

    tx.commit().await?;
    Ok(location)
}

pub async fn update_location(
    pool: &SqlitePool,
    actor: &Actor,
    id: &str,
    input: UpdateLocation,
    ip: Option<&str>,
) -> Result<Location, MaintError> {
    actor.require_permission(Permission::LocationsManage)?;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let current = scoped_location(&mut tx, actor, id).await?;

    let new_code = match input.code {
        Some(c) => Some(validate_code(&c, "Le code d'emplacement")?),
        None => None,
    };
    let new_name = match input.name {
        Some(n) => Some(validate_name(&n, "Le nom d'emplacement")?),
        None => None,
    };

    let effective_code = new_code.as_deref().unwrap_or(&current.code);
    let effective_name = new_name.as_deref().unwrap_or(&current.name);

    if effective_code != current.code {
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM locations WHERE site_id = ? AND code = ? COLLATE NOCASE AND id != ? AND deleted_at IS NULL",
        )
        .bind(&current.site_id)
        .bind(effective_code)
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;

        if count > 0 {
            return Err(MaintError::Conflict(
                "Un emplacement avec ce code existe déjà sur ce site".into(),
            ));
        }
    }

    let now = store::now();

    sqlx::query(
        "UPDATE locations SET code = ?, name = ?, updated_at = ? WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(effective_code)
    .bind(effective_name)
    .bind(&now)
    .bind(id)
    .execute(&mut *tx)
    .await?;

    let updated = Location {
        id: current.id.clone(),
        site_id: current.site_id.clone(),
        code: effective_code.to_string(),
        name: effective_name.to_string(),
        kind: current.kind.clone(),
        parent_id: current.parent_id.clone(),
        created_at: current.created_at.clone(),
        updated_at: now,
    };

    store::audit(
        &mut tx,
        actor,
        Audit {
            action: "update",
            entity: "location",
            entity_id: Some(id),
            site_id: Some(&current.site_id),
            before: Some(json!(current)),
            after: Some(json!(updated)),
            ip,
        },
    )
    .await?;

    tx.commit().await?;
    Ok(updated)
}

pub async fn delete_location(
    pool: &SqlitePool,
    actor: &Actor,
    id: &str,
    ip: Option<&str>,
) -> Result<(), MaintError> {
    actor.require_permission(Permission::LocationsManage)?;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let current = scoped_location(&mut tx, actor, id).await?;

    let children_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM locations WHERE parent_id = ? AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;

    if children_count > 0 {
        return Err(MaintError::Conflict(
            "Impossible de supprimer un emplacement ayant des sous-emplacements".into(),
        ));
    }

    let stock_count: i64 =
        sqlx::query_scalar("SELECT COALESCE(SUM(quantity), 0) FROM stock WHERE location_id = ?")
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;

    if stock_count > 0 {
        return Err(MaintError::Conflict(
            "Impossible de supprimer un emplacement contenant du stock".into(),
        ));
    }

    let now = store::now();

    sqlx::query(
        "UPDATE locations SET deleted_at = ?, updated_at = ? WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(&now)
    .bind(&now)
    .bind(id)
    .execute(&mut *tx)
    .await?;

    store::audit(
        &mut tx,
        actor,
        Audit {
            action: "delete",
            entity: "location",
            entity_id: Some(id),
            site_id: Some(&current.site_id),
            before: Some(json!(current)),
            after: None,
            ip,
        },
    )
    .await?;

    tx.commit().await?;
    Ok(())
}

fn validate_location_kind(kind: &str) -> Result<&str, MaintError> {
    match kind.trim() {
        "magasin" | "allee" | "rack" | "bac" => Ok(kind.trim()),
        _ => Err(MaintError::BadRequest(format!(
            "Type d'emplacement invalide : « {kind} » (attendu: magasin, allee, rack, bac)"
        ))),
    }
}

async fn validate_location_hierarchy(
    tx: &mut SqliteConnection,
    site_id: &str,
    kind: &str,
    parent_id: Option<&str>,
) -> Result<(), MaintError> {
    match kind {
        "magasin" => {
            if parent_id.is_some() {
                return Err(MaintError::BadRequest(
                    "Un magasin ne peut pas avoir d'emplacement parent".into(),
                ));
            }
        }
        "allee" => {
            let pid = parent_id.ok_or_else(|| {
                MaintError::BadRequest("Une allée doit avoir un magasin pour parent".into())
            })?;
            let parent_info: Option<(String, String)> = sqlx::query_as(
                "SELECT site_id, kind FROM locations WHERE id = ? AND deleted_at IS NULL",
            )
            .bind(pid)
            .fetch_optional(&mut *tx)
            .await?;

            match parent_info {
                Some((s, k)) if s == site_id && k == "magasin" => {}
                Some((s, _)) if s != site_id => {
                    return Err(MaintError::BadRequest(
                        "L'emplacement parent doit appartenir au même site".into(),
                    ));
                }
                _ => {
                    return Err(MaintError::BadRequest(
                        "Une allée doit avoir un magasin pour parent".into(),
                    ));
                }
            }
        }
        "rack" => {
            let pid = parent_id.ok_or_else(|| {
                MaintError::BadRequest("Un rack doit avoir une allée pour parent".into())
            })?;
            let parent_info: Option<(String, String)> = sqlx::query_as(
                "SELECT site_id, kind FROM locations WHERE id = ? AND deleted_at IS NULL",
            )
            .bind(pid)
            .fetch_optional(&mut *tx)
            .await?;

            match parent_info {
                Some((s, k)) if s == site_id && k == "allee" => {}
                Some((s, _)) if s != site_id => {
                    return Err(MaintError::BadRequest(
                        "L'emplacement parent doit appartenir au même site".into(),
                    ));
                }
                _ => {
                    return Err(MaintError::BadRequest(
                        "Un rack doit avoir une allée pour parent".into(),
                    ));
                }
            }
        }
        "bac" => {
            let pid = parent_id.ok_or_else(|| {
                MaintError::BadRequest("Un bac doit avoir un rack pour parent".into())
            })?;
            let parent_info: Option<(String, String)> = sqlx::query_as(
                "SELECT site_id, kind FROM locations WHERE id = ? AND deleted_at IS NULL",
            )
            .bind(pid)
            .fetch_optional(&mut *tx)
            .await?;

            match parent_info {
                Some((s, k)) if s == site_id && k == "rack" => {}
                Some((s, _)) if s != site_id => {
                    return Err(MaintError::BadRequest(
                        "L'emplacement parent doit appartenir au même site".into(),
                    ));
                }
                _ => {
                    return Err(MaintError::BadRequest(
                        "Un bac doit avoir un rack pour parent".into(),
                    ));
                }
            }
        }
        _ => {
            return Err(MaintError::BadRequest(format!(
                "Type d'emplacement invalide : « {kind} »"
            )));
        }
    }
    Ok(())
}

// ============================================================================
// 4. FOURNISSEURS
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Supplier {
    pub id: String,
    pub site_id: String,
    pub name: String,
    pub contact_name: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub address: Option<String>,
    pub website: Option<String>,
    pub notes: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateSupplier {
    pub site_id: Option<String>,
    pub name: String,
    pub contact_name: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub address: Option<String>,
    pub website: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct UpdateSupplier {
    pub name: Option<String>,
    #[serde(default)]
    pub contact_name: PatchField<String>,
    #[serde(default)]
    pub email: PatchField<String>,
    #[serde(default)]
    pub phone: PatchField<String>,
    #[serde(default)]
    pub address: PatchField<String>,
    #[serde(default)]
    pub website: PatchField<String>,
    #[serde(default)]
    pub notes: PatchField<String>,
}

#[derive(Debug, Clone, Default)]
pub struct SupplierFilter {
    pub q: Option<String>,
    pub site_id: Option<String>,
    pub cursor: Option<Cursor>,
    pub limit: u32,
}

pub async fn list_suppliers(
    pool: &SqlitePool,
    actor: &Actor,
    filter: SupplierFilter,
) -> Result<Page<Supplier>, MaintError> {
    actor.require_permission(Permission::SuppliersView)?;
    let limit = validate_limit(filter.limit)?;
    let site_scope = resolve_site_id_for_query(actor, filter.site_id.as_deref())?;

    let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(
        r#"
        SELECT id, site_id, name, contact_name, email, phone, address, website, notes, created_at, updated_at
        FROM suppliers
        WHERE deleted_at IS NULL
        "#,
    );

    if let Some(sid) = site_scope {
        qb.push(" AND site_id = ").push_bind(sid);
    }

    if let Some(q) = filter.q.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        let pattern = like_pattern(q);
        qb.push(" AND (name LIKE ")
            .push_bind(pattern.clone())
            .push(" ESCAPE '\\' OR contact_name LIKE ")
            .push_bind(pattern)
            .push(" ESCAPE '\\')");
    }

    if let Some(cursor) = filter.cursor {
        if let Some(sort_val) = cursor.sort_value {
            qb.push(" AND (name COLLATE NOCASE > ")
                .push_bind(sort_val.clone())
                .push(" OR (name COLLATE NOCASE = ")
                .push_bind(sort_val)
                .push(" AND id > ")
                .push_bind(cursor.id)
                .push("))");
        } else {
            qb.push(" AND id > ").push_bind(cursor.id);
        }
    }

    qb.push(" ORDER BY name COLLATE NOCASE ASC, id ASC LIMIT ")
        .push_bind((limit + 1) as i64);

    let mut rows: Vec<Supplier> = qb.build_query_as().fetch_all(pool).await?;

    let next_cursor = if rows.len() > limit as usize {
        rows.truncate(limit as usize);
        rows.last()
            .map(|last| Cursor::with_sort_value(&last.id, &last.name).encode())
    } else {
        None
    };

    Ok(Page {
        items: rows,
        next_cursor,
    })
}

pub async fn get_supplier(
    pool: &SqlitePool,
    actor: &Actor,
    id: &str,
) -> Result<Supplier, MaintError> {
    actor.require_permission(Permission::SuppliersView)?;
    let mut conn = pool.acquire().await?;
    scoped_supplier(&mut conn, actor, id).await
}

async fn scoped_supplier(
    conn: &mut SqliteConnection,
    actor: &Actor,
    id: &str,
) -> Result<Supplier, MaintError> {
    let row: Option<Supplier> = sqlx::query_as(
        r#"
        SELECT id, site_id, name, contact_name, email, phone, address, website, notes, created_at, updated_at
        FROM suppliers
        WHERE id = ? AND deleted_at IS NULL AND (? OR site_id = ?)
        "#,
    )
    .bind(id)
    .bind(actor.is_admin())
    .bind(&actor.site_id)
    .fetch_optional(conn)
    .await?;

    row.ok_or_else(|| MaintError::NotFound("Fournisseur".into()))
}

pub async fn create_supplier(
    pool: &SqlitePool,
    actor: &Actor,
    input: CreateSupplier,
    ip: Option<&str>,
) -> Result<Supplier, MaintError> {
    actor.require_permission(Permission::SuppliersManage)?;
    let site_id = resolve_site_id_for_create(actor, input.site_id.as_deref())?;
    let name = validate_name(&input.name, "Le nom du fournisseur")?;
    let contact_name = clean_opt_string(input.contact_name, 128, "Le nom du contact")?;
    let email = clean_opt_string(input.email, 256, "L'adresse email")?;
    let phone = clean_opt_string(input.phone, 64, "Le téléphone")?;
    let address = clean_opt_string(input.address, 512, "L'adresse")?;
    let website = clean_opt_string(input.website, 256, "Le site web")?;
    let notes = clean_opt_string(input.notes, 2048, "Les notes")?;

    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    ensure_site_exists(&mut tx, &site_id).await?;

    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM suppliers WHERE site_id = ? AND name = ? COLLATE NOCASE AND deleted_at IS NULL",
    )
    .bind(&site_id)
    .bind(&name)
    .fetch_one(&mut *tx)
    .await?;

    if count > 0 {
        return Err(MaintError::Conflict(
            "Un fournisseur avec ce nom existe déjà sur ce site".into(),
        ));
    }

    let supplier_id = store::id();
    let now = store::now();

    sqlx::query(
        r#"
        INSERT INTO suppliers (
            id, site_id, name, contact_name, email, phone, address, website, notes,
            created_at, updated_at, deleted_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NULL)
        "#,
    )
    .bind(&supplier_id)
    .bind(&site_id)
    .bind(&name)
    .bind(&contact_name)
    .bind(&email)
    .bind(&phone)
    .bind(&address)
    .bind(&website)
    .bind(&notes)
    .bind(&now)
    .bind(&now)
    .execute(&mut *tx)
    .await?;

    let supplier = Supplier {
        id: supplier_id.clone(),
        site_id: site_id.clone(),
        name,
        contact_name,
        email,
        phone,
        address,
        website,
        notes,
        created_at: now.clone(),
        updated_at: now,
    };

    store::audit(
        &mut tx,
        actor,
        Audit {
            action: "create",
            entity: "supplier",
            entity_id: Some(&supplier_id),
            site_id: Some(&site_id),
            before: None,
            after: Some(json!(supplier)),
            ip,
        },
    )
    .await?;

    tx.commit().await?;
    Ok(supplier)
}

pub async fn update_supplier(
    pool: &SqlitePool,
    actor: &Actor,
    id: &str,
    input: UpdateSupplier,
    ip: Option<&str>,
) -> Result<Supplier, MaintError> {
    actor.require_permission(Permission::SuppliersManage)?;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let current = scoped_supplier(&mut tx, actor, id).await?;

    let new_name = match input.name {
        Some(n) => Some(validate_name(&n, "Le nom du fournisseur")?),
        None => None,
    };

    let effective_name = new_name.as_deref().unwrap_or(&current.name);

    if effective_name != current.name {
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM suppliers WHERE site_id = ? AND name = ? COLLATE NOCASE AND id != ? AND deleted_at IS NULL",
        )
        .bind(&current.site_id)
        .bind(effective_name)
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;

        if count > 0 {
            return Err(MaintError::Conflict(
                "Un fournisseur avec ce nom existe déjà sur ce site".into(),
            ));
        }
    }

    let mut effective_contact = current.contact_name.clone();
    match input.contact_name {
        PatchField::Absent => {}
        PatchField::Null => effective_contact = None,
        PatchField::Value(v) => {
            effective_contact = clean_opt_string(Some(v), 128, "Le nom du contact")?;
        }
    }

    let mut effective_email = current.email.clone();
    match input.email {
        PatchField::Absent => {}
        PatchField::Null => effective_email = None,
        PatchField::Value(v) => {
            effective_email = clean_opt_string(Some(v), 256, "L'adresse email")?;
        }
    }

    let mut effective_phone = current.phone.clone();
    match input.phone {
        PatchField::Absent => {}
        PatchField::Null => effective_phone = None,
        PatchField::Value(v) => effective_phone = clean_opt_string(Some(v), 64, "Le téléphone")?,
    }

    let mut effective_address = current.address.clone();
    match input.address {
        PatchField::Absent => {}
        PatchField::Null => effective_address = None,
        PatchField::Value(v) => effective_address = clean_opt_string(Some(v), 512, "L'adresse")?,
    }

    let mut effective_website = current.website.clone();
    match input.website {
        PatchField::Absent => {}
        PatchField::Null => effective_website = None,
        PatchField::Value(v) => effective_website = clean_opt_string(Some(v), 256, "Le site web")?,
    }

    let mut effective_notes = current.notes.clone();
    match input.notes {
        PatchField::Absent => {}
        PatchField::Null => effective_notes = None,
        PatchField::Value(v) => effective_notes = clean_opt_string(Some(v), 2048, "Les notes")?,
    }

    let now = store::now();

    sqlx::query(
        r#"
        UPDATE suppliers 
        SET name = ?, contact_name = ?, email = ?, phone = ?, address = ?,
            website = ?, notes = ?, updated_at = ?
        WHERE id = ? AND deleted_at IS NULL
        "#,
    )
    .bind(effective_name)
    .bind(&effective_contact)
    .bind(&effective_email)
    .bind(&effective_phone)
    .bind(&effective_address)
    .bind(&effective_website)
    .bind(&effective_notes)
    .bind(&now)
    .bind(id)
    .execute(&mut *tx)
    .await?;

    let updated = Supplier {
        id: current.id.clone(),
        site_id: current.site_id.clone(),
        name: effective_name.to_string(),
        contact_name: effective_contact,
        email: effective_email,
        phone: effective_phone,
        address: effective_address,
        website: effective_website,
        notes: effective_notes,
        created_at: current.created_at.clone(),
        updated_at: now,
    };

    store::audit(
        &mut tx,
        actor,
        Audit {
            action: "update",
            entity: "supplier",
            entity_id: Some(id),
            site_id: Some(&current.site_id),
            before: Some(json!(current)),
            after: Some(json!(updated)),
            ip,
        },
    )
    .await?;

    tx.commit().await?;
    Ok(updated)
}

pub async fn delete_supplier(
    pool: &SqlitePool,
    actor: &Actor,
    id: &str,
    ip: Option<&str>,
) -> Result<(), MaintError> {
    actor.require_permission(Permission::SuppliersManage)?;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let current = scoped_supplier(&mut tx, actor, id).await?;

    let now = store::now();

    sqlx::query(
        "UPDATE suppliers SET deleted_at = ?, updated_at = ? WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(&now)
    .bind(&now)
    .bind(id)
    .execute(&mut *tx)
    .await?;

    store::audit(
        &mut tx,
        actor,
        Audit {
            action: "delete",
            entity: "supplier",
            entity_id: Some(id),
            site_id: Some(&current.site_id),
            before: Some(json!(current)),
            after: None,
            ip,
        },
    )
    .await?;

    tx.commit().await?;
    Ok(())
}

// ============================================================================
// FONCTIONS UTILITAIRES DE VALIDATION ET DE GESTION DES SITES
// ============================================================================

fn resolve_site_id_for_create(
    actor: &Actor,
    requested_site_id: Option<&str>,
) -> Result<String, MaintError> {
    if actor.is_admin() {
        let sid = requested_site_id
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| MaintError::BadRequest("Le site est obligatoire".into()))?;
        Ok(sid.to_string())
    } else {
        if let Some(sid) = requested_site_id {
            if sid != actor.site_id.as_deref().unwrap_or_default() {
                return Err(MaintError::Forbidden(
                    "Accès refusé : vous ne pouvez pas créer de ressource sur un autre site".into(),
                ));
            }
        }
        actor
            .site_id
            .clone()
            .ok_or_else(|| MaintError::Forbidden("Aucun site rattaché".into()))
    }
}

fn resolve_site_id_for_query<'a>(
    actor: &'a Actor,
    requested_site_id: Option<&'a str>,
) -> Result<Option<&'a str>, MaintError> {
    if actor.is_admin() {
        Ok(requested_site_id.filter(|s| !s.is_empty()))
    } else {
        let actor_site = actor.site_id.as_deref().unwrap_or_default();
        if let Some(req_sid) = requested_site_id {
            if req_sid != actor_site {
                // Règle de la spec : ressource d'un autre site répond 404
                return Err(MaintError::NotFound("Site".into()));
            }
        }
        Ok(Some(actor_site))
    }
}

fn validate_code(code: &str, field_name: &str) -> Result<String, MaintError> {
    let trimmed = code.trim();
    if trimmed.is_empty() || trimmed.chars().count() > 64 || trimmed.chars().any(char::is_control) {
        return Err(MaintError::BadRequest(format!(
            "{field_name} doit contenir entre 1 et 64 caractères valides"
        )));
    }
    Ok(trimmed.to_string())
}

fn validate_name(name: &str, field_name: &str) -> Result<String, MaintError> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.chars().count() > 128 || trimmed.chars().any(char::is_control)
    {
        return Err(MaintError::BadRequest(format!(
            "{field_name} doit contenir entre 1 et 128 caractères valides"
        )));
    }
    Ok(trimmed.to_string())
}

fn clean_opt_id(id: Option<String>) -> Option<String> {
    id.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn clean_opt_string(
    s: Option<String>,
    max_len: usize,
    field_name: &str,
) -> Result<Option<String>, MaintError> {
    match s.map(|v| v.trim().to_string()).filter(|v| !v.is_empty()) {
        Some(v) => {
            if v.chars().count() > max_len || v.chars().any(char::is_control) {
                return Err(MaintError::BadRequest(format!(
                    "{field_name} ne peut pas dépasser {max_len} caractères valides"
                )));
            }
            Ok(Some(v))
        }
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::like_pattern;

    #[test]
    fn motif_like_echappe_les_jokers() {
        assert_eq!(like_pattern("abc"), "%abc%");
        assert_eq!(like_pattern(r"50%_a\b"), r"%50\%\_a\\b%");
    }
}
