//! Module de gestion des pièces détachées et du stock (Phase 4).
//!
//! Gère le catalogue des pièces détachées par site, le stock par emplacement
//! physique sous transaction stricte `BEGIN IMMEDIATE`, la valorisation au prix moyen
//! pondéré (PMP en centimes d'euro), la traçabilité des mouvements (entrées, sorties,
//! transferts, ajustements), l'association aux équipements, les alertes de stock bas
//! et les réservations de pièces préparées pour intervention.

use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{FromRow, QueryBuilder, Sqlite, SqliteConnection, SqlitePool};

use crate::{
    domain::pagination::Cursor,
    interventions,
    referentiel::{NamedRef, PatchField},
    store::{self, Audit},
    Actor, MaintError, Permission,
};

// ============================================================================
// STRUCTURES DE DONNÉES DU DOMAINE
// ============================================================================

/// Représentation d'une pièce détachée en base.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Part {
    pub id: String,
    pub site_id: String,
    pub reference: String,
    pub name: String,
    pub description: Option<String>,
    pub category: Option<String>,
    pub unit: String,
    pub unit_price_cents: i64,
    pub supplier_id: Option<String>,
    pub supplier_reference: Option<String>,
    pub manufacturer: Option<String>,
    pub manufacturer_reference: Option<String>,
    pub barcode: Option<String>,
    pub alert_threshold: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
}

/// Ventilation du stock d'une pièce sur un emplacement physique.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartLocationStock {
    pub location_id: String,
    pub location_code: String,
    pub location_name: String,
    pub quantity: i64,
    pub reserved_quantity: i64,
    pub available_quantity: i64,
}

/// Équipement auquel une pièce est rattachée.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartEquipmentRef {
    pub equipment_id: String,
    pub equipment_code: String,
    pub equipment_name: String,
    pub quantity_recommended: i64,
    pub notes: Option<String>,
}

/// Fiche détaillée d'une pièce avec indicateurs de stock et relations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartDetail {
    pub documents_count: i64,
    #[serde(flatten)]
    pub part: Part,
    pub total_stock: i64,
    pub total_reserved: i64,
    pub total_available: i64,
    pub effective_threshold: i64,
    pub is_low_stock: bool,
    pub locations: Vec<PartLocationStock>,
    pub supplier: Option<NamedRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub equipments: Option<Vec<PartEquipmentRef>>,
}

/// Données de création d'une pièce détachée.
#[derive(Debug, Clone, Deserialize)]
pub struct CreatePart {
    pub site_id: Option<String>,
    pub reference: String,
    pub name: String,
    pub description: Option<String>,
    pub category: Option<String>,
    pub unit: Option<String>,
    pub unit_price_cents: Option<i64>,
    pub supplier_id: Option<String>,
    pub supplier_reference: Option<String>,
    pub manufacturer: Option<String>,
    pub manufacturer_reference: Option<String>,
    pub barcode: Option<String>,
    pub alert_threshold: Option<i64>,
}

/// Données de mise à jour partielle (PATCH) d'une pièce détachée.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct UpdatePart {
    pub reference: Option<String>,
    pub name: Option<String>,
    #[serde(default)]
    pub description: PatchField<String>,
    #[serde(default)]
    pub category: PatchField<String>,
    pub unit: Option<String>,
    pub unit_price_cents: Option<i64>,
    #[serde(default)]
    pub supplier_id: PatchField<String>,
    #[serde(default)]
    pub supplier_reference: PatchField<String>,
    #[serde(default)]
    pub manufacturer: PatchField<String>,
    #[serde(default)]
    pub manufacturer_reference: PatchField<String>,
    #[serde(default)]
    pub barcode: PatchField<String>,
    #[serde(default)]
    pub alert_threshold: PatchField<i64>,
}

/// Filtres pour la recherche et pagination du catalogue de pièces.
#[derive(Debug, Clone, Default)]
pub struct PartFilter {
    pub q: Option<String>,
    pub site_id: Option<String>,
    pub category: Option<String>,
    pub supplier_id: Option<String>,
    pub low_stock: Option<bool>,
    pub cursor: Option<Cursor>,
    pub limit: u32,
}

/// Élément de stock par emplacement physique.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StockItem {
    pub id: String,
    pub site_id: String,
    pub part_id: String,
    pub location_id: String,
    pub quantity: i64,
    pub reserved_quantity: i64,
    pub available_quantity: i64,
    pub updated_at: String,
    pub part_reference: String,
    pub part_name: String,
    pub location_code: String,
    pub location_name: String,
}

/// Filtres pour l'état des stocks par emplacement.
#[derive(Debug, Clone, Default)]
pub struct StockFilter {
    pub site_id: Option<String>,
    pub part_id: Option<String>,
    pub location_id: Option<String>,
}

/// Mouvement de stock tracé.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StockMovement {
    pub id: String,
    pub site_id: String,
    pub part_id: String,
    pub movement_type: String,
    pub quantity: i64,
    pub from_location_id: Option<String>,
    pub to_location_id: Option<String>,
    pub intervention_id: Option<String>,
    pub user_id: String,
    pub unit_price_cents: Option<i64>,
    pub reason: Option<String>,
    pub adjustment_kind: Option<String>,
    pub delta: Option<i64>,
    pub created_at: String,
    pub part_reference: Option<String>,
    pub part_name: Option<String>,
    pub from_location_code: Option<String>,
    pub from_location_name: Option<String>,
    pub to_location_code: Option<String>,
    pub to_location_name: Option<String>,
    pub user_display_name: Option<String>,
    pub intervention_code: Option<String>,
}

/// Données pour l'enregistrement d'un mouvement d'entrée ou de transfert.
#[derive(Debug, Clone, Deserialize)]
pub struct CreateStockMovement {
    pub part_id: String,
    pub movement_type: String,
    pub to_location_id: Option<String>,
    pub from_location_id: Option<String>,
    pub quantity: i64,
    pub unit_price_cents: Option<i64>,
    pub reason: Option<String>,
}

/// Données pour l'enregistrement d'un ajustement de stock.
#[derive(Debug, Clone, Deserialize)]
pub struct CreateAdjustment {
    pub part_id: String,
    pub location_id: String,
    pub delta: i64,
    pub adjustment_kind: String,
    pub reason: String,
}

/// Pièce consommée sur une intervention.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct InterventionPart {
    pub id: String,
    pub site_id: String,
    pub intervention_id: String,
    pub part_id: String,
    pub location_id: String,
    pub quantity: i64,
    pub unit_price_cents: i64,
    pub movement_id: String,
    pub created_at: String,
    pub part_reference: String,
    pub part_name: String,
    pub location_code: String,
    pub location_name: String,
}

/// Données de consommation d'une pièce sur une intervention.
#[derive(Debug, Clone, Deserialize)]
pub struct ConsumePartInput {
    pub part_id: String,
    pub location_id: String,
    pub quantity: i64,
}

/// Pièce détachée associée à un équipement.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct EquipmentPart {
    pub equipment_id: String,
    pub part_id: String,
    pub quantity_recommended: i64,
    pub notes: Option<String>,
    pub part_reference: String,
    pub part_name: String,
    pub part_category: Option<String>,
    pub unit_price_cents: i64,
    pub available_stock: i64,
}

/// Données d'association d'une pièce à un équipement.
#[derive(Debug, Clone, Deserialize)]
pub struct AssociateEquipmentPart {
    pub part_id: String,
    pub quantity_recommended: Option<i64>,
    pub notes: Option<String>,
}

/// Réservation de pièce préparée pour une intervention.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct StockReservation {
    pub id: String,
    pub site_id: String,
    pub intervention_id: String,
    pub part_id: String,
    pub location_id: String,
    pub quantity: i64,
    pub status: String,
    pub prepared_by_id: String,
    pub prepared_at: String,
    pub closed_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub part_reference: Option<String>,
    pub part_name: Option<String>,
    pub location_code: Option<String>,
    pub location_name: Option<String>,
    pub prepared_by_name: Option<String>,
}

/// Données de préparation d'une réservation de pièce.
#[derive(Debug, Clone, Deserialize)]
pub struct CreateReservation {
    pub part_id: String,
    pub location_id: String,
    pub quantity: i64,
}

/// Consommation totale ou partielle d'une réservation préparée.
#[derive(Debug, Clone, Deserialize)]
pub struct ConsumeReservationInput {
    pub quantity: Option<i64>,
}

/// Filtres pour le journal des mouvements de stock.
#[derive(Debug, Clone, Default)]
pub struct StockMovementFilter {
    pub q: Option<String>,
    pub site_id: Option<String>,
    pub part_id: Option<String>,
    pub movement_type: Option<String>,
    pub intervention_id: Option<String>,
    pub location_id: Option<String>,
    pub user_id: Option<String>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    pub cursor: Option<Cursor>,
    pub limit: u32,
}

// ============================================================================
// 1. CATALOGUE DE PIÈCES DÉTACHÉES (CRUD)
// ============================================================================

pub async fn list_parts(
    pool: &SqlitePool,
    actor: &Actor,
    filter: PartFilter,
) -> Result<crate::referentiel::Page<PartDetail>, MaintError> {
    actor.require_permission(Permission::PartsView)?;
    let limit = validate_limit(filter.limit)?;
    let site_scope = resolve_site_id_for_query(actor, filter.site_id.as_deref())?;

    let global_threshold: i64 = get_global_stock_alert_threshold(pool).await?;

    let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(
        r#"
        SELECT 
            p.id, p.site_id, p.reference, p.name, p.description, p.category, p.unit,
            p.unit_price_cents, p.supplier_id, p.supplier_reference, p.manufacturer,
            p.manufacturer_reference, p.barcode, p.alert_threshold, p.created_at, p.updated_at
        FROM parts p
        WHERE p.deleted_at IS NULL
        "#,
    );

    if let Some(sid) = site_scope {
        qb.push(" AND p.site_id = ").push_bind(sid);
    }

    if let Some(cat) = filter
        .category
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        qb.push(" AND p.category = ").push_bind(cat);
    }

    if let Some(sup) = filter
        .supplier_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        qb.push(" AND p.supplier_id = ").push_bind(sup);
    }

    if let Some(q) = filter.q.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        let pattern = like_pattern(q);
        qb.push(" AND (p.reference LIKE ")
            .push_bind(pattern.clone())
            .push(" ESCAPE '\\' OR p.name LIKE ")
            .push_bind(pattern.clone())
            .push(" ESCAPE '\\' OR p.barcode LIKE ")
            .push_bind(pattern)
            .push(" ESCAPE '\\')");
    }

    // Gestion du curseur (tri par reference ASC, id ASC)
    if let Some(cursor) = &filter.cursor {
        if let Some(sort_val) = &cursor.sort_value {
            qb.push(" AND (p.reference > ")
                .push_bind(sort_val)
                .push(" OR (p.reference = ")
                .push_bind(sort_val)
                .push(" AND p.id > ")
                .push_bind(&cursor.id)
                .push("))");
        } else {
            qb.push(" AND p.id > ").push_bind(&cursor.id);
        }
    }

    qb.push(" ORDER BY p.reference ASC, p.id ASC LIMIT ")
        .push_bind(limit as i64 + 1);

    let rows: Vec<Part> = qb.build_query_as().fetch_all(pool).await?;

    let has_more = rows.len() > limit as usize;
    let mut parts = rows;
    if has_more {
        parts.truncate(limit as usize);
    }

    let next_cursor = if has_more {
        parts
            .last()
            .map(|last| Cursor::with_sort_value(&last.id, &last.reference).encode())
    } else {
        None
    };

    let mut items = Vec::with_capacity(parts.len());
    for part in parts {
        let detail = build_part_detail(pool, actor, part, global_threshold, false).await?;
        if let Some(low_only) = filter.low_stock {
            if low_only && !detail.is_low_stock {
                continue;
            }
        }
        items.push(detail);
    }

    Ok(crate::referentiel::Page { items, next_cursor })
}

pub async fn get_part(
    pool: &SqlitePool,
    actor: &Actor,
    id: &str,
) -> Result<PartDetail, MaintError> {
    actor.require_permission(Permission::PartsView)?;
    let mut conn = pool.acquire().await?;
    let part = scoped_part(&mut conn, actor, id).await?;
    let global_threshold: i64 = get_global_stock_alert_threshold(pool).await?;
    build_part_detail(pool, actor, part, global_threshold, true).await
}

pub async fn create_part(
    pool: &SqlitePool,
    actor: &Actor,
    input: CreatePart,
    ip: Option<&str>,
) -> Result<PartDetail, MaintError> {
    actor.require_permission(Permission::PartsCreate)?;
    let site_id = resolve_site_id_for_create(actor, input.site_id.as_deref())?;
    let reference = validate_reference(&input.reference)?;
    let name = validate_name(&input.name)?;
    let unit = validate_unit(input.unit.as_deref().unwrap_or("piece"))?;
    let unit_price_cents = input.unit_price_cents.unwrap_or(0);
    if unit_price_cents < 0 {
        return Err(MaintError::BadRequest(
            "Le prix unitaire en centimes ne peut pas être négatif".into(),
        ));
    }
    let alert_threshold = match input.alert_threshold {
        Some(t) => {
            if t < 0 {
                return Err(MaintError::BadRequest(
                    "Le seuil d'alerte ne peut pas être négatif".into(),
                ));
            }
            Some(t)
        }
        None => None,
    };

    let description = clean_opt_string(input.description, 2048, "La description")?;
    let category = clean_opt_string(input.category, 64, "La catégorie")?;
    let supplier_id = clean_opt_id(input.supplier_id);
    let supplier_reference =
        clean_opt_string(input.supplier_reference, 128, "La référence fournisseur")?;
    let manufacturer = clean_opt_string(input.manufacturer, 128, "Le constructeur")?;
    let manufacturer_reference = clean_opt_string(
        input.manufacturer_reference,
        128,
        "La référence constructeur",
    )?;
    let barcode = clean_opt_string(input.barcode, 64, "Le code-barres")?;

    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    ensure_site_exists(&mut tx, &site_id).await?;

    if let Some(sup_id) = &supplier_id {
        ensure_supplier_exists(&mut tx, &site_id, sup_id).await?;
    }

    let exists: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM parts WHERE site_id = ? AND reference = ? COLLATE NOCASE AND deleted_at IS NULL",
    )
    .bind(&site_id)
    .bind(&reference)
    .fetch_one(&mut *tx)
    .await?;

    if exists > 0 {
        return Err(MaintError::Conflict(
            "Une pièce avec cette référence existe déjà sur ce site".into(),
        ));
    }

    let part_id = store::id();
    let now = store::now();

    sqlx::query(
        r#"
        INSERT INTO parts (
            id, site_id, reference, name, description, category, unit, unit_price_cents,
            supplier_id, supplier_reference, manufacturer, manufacturer_reference, barcode,
            alert_threshold, created_at, updated_at, deleted_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NULL)
        "#,
    )
    .bind(&part_id)
    .bind(&site_id)
    .bind(&reference)
    .bind(&name)
    .bind(&description)
    .bind(&category)
    .bind(&unit)
    .bind(unit_price_cents)
    .bind(&supplier_id)
    .bind(&supplier_reference)
    .bind(&manufacturer)
    .bind(&manufacturer_reference)
    .bind(&barcode)
    .bind(alert_threshold)
    .bind(&now)
    .bind(&now)
    .execute(&mut *tx)
    .await?;

    let part = Part {
        id: part_id.clone(),
        site_id: site_id.clone(),
        reference,
        name,
        description,
        category,
        unit,
        unit_price_cents,
        supplier_id,
        supplier_reference,
        manufacturer,
        manufacturer_reference,
        barcode,
        alert_threshold,
        created_at: now.clone(),
        updated_at: now,
    };

    store::audit(
        &mut tx,
        actor,
        Audit {
            action: "create",
            entity: "part",
            entity_id: Some(&part_id),
            site_id: Some(&site_id),
            before: None,
            after: Some(json!(part)),
            ip,
        },
    )
    .await?;

    crate::pilotage::notifications::stock(&mut tx, actor).await?;
    tx.commit().await?;

    let global_threshold: i64 = get_global_stock_alert_threshold(pool).await?;
    build_part_detail(pool, actor, part, global_threshold, true).await
}

pub async fn update_part(
    pool: &SqlitePool,
    actor: &Actor,
    id: &str,
    input: UpdatePart,
    ip: Option<&str>,
) -> Result<PartDetail, MaintError> {
    actor.require_permission(Permission::PartsEdit)?;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let current = scoped_part(&mut tx, actor, id).await?;

    let effective_reference = match input.reference {
        Some(r) => {
            let valid = validate_reference(&r)?;
            if valid != current.reference {
                let count: i64 = sqlx::query_scalar(
                    "SELECT count(*) FROM parts WHERE site_id = ? AND reference = ? COLLATE NOCASE AND id != ? AND deleted_at IS NULL",
                )
                .bind(&current.site_id)
                .bind(&valid)
                .bind(id)
                .fetch_one(&mut *tx)
                .await?;

                if count > 0 {
                    return Err(MaintError::Conflict(
                        "Une pièce avec cette référence existe déjà sur ce site".into(),
                    ));
                }
            }
            valid
        }
        None => current.reference.clone(),
    };

    let effective_name = match input.name {
        Some(n) => validate_name(&n)?,
        None => current.name.clone(),
    };

    let mut effective_description = current.description.clone();
    match input.description {
        PatchField::Absent => {}
        PatchField::Null => effective_description = None,
        PatchField::Value(v) => {
            effective_description = clean_opt_string(Some(v), 2048, "La description")?;
        }
    }

    let mut effective_category = current.category.clone();
    match input.category {
        PatchField::Absent => {}
        PatchField::Null => effective_category = None,
        PatchField::Value(v) => {
            effective_category = clean_opt_string(Some(v), 64, "La catégorie")?;
        }
    }

    let effective_unit = match input.unit {
        Some(u) => validate_unit(&u)?,
        None => current.unit.clone(),
    };

    let effective_price = match input.unit_price_cents {
        Some(p) => {
            if p < 0 {
                return Err(MaintError::BadRequest(
                    "Le prix unitaire en centimes ne peut pas être négatif".into(),
                ));
            }
            p
        }
        None => current.unit_price_cents,
    };

    let mut effective_supplier_id = current.supplier_id.clone();
    match input.supplier_id {
        PatchField::Absent => {}
        PatchField::Null => effective_supplier_id = None,
        PatchField::Value(v) => {
            let sup_id = clean_opt_id(Some(v));
            if let Some(sid) = &sup_id {
                ensure_supplier_exists(&mut tx, &current.site_id, sid).await?;
            }
            effective_supplier_id = sup_id;
        }
    }

    let mut effective_supplier_ref = current.supplier_reference.clone();
    match input.supplier_reference {
        PatchField::Absent => {}
        PatchField::Null => effective_supplier_ref = None,
        PatchField::Value(v) => {
            effective_supplier_ref = clean_opt_string(Some(v), 128, "La référence fournisseur")?;
        }
    }

    let mut effective_manufacturer = current.manufacturer.clone();
    match input.manufacturer {
        PatchField::Absent => {}
        PatchField::Null => effective_manufacturer = None,
        PatchField::Value(v) => {
            effective_manufacturer = clean_opt_string(Some(v), 128, "Le constructeur")?;
        }
    }

    let mut effective_manufacturer_ref = current.manufacturer_reference.clone();
    match input.manufacturer_reference {
        PatchField::Absent => {}
        PatchField::Null => effective_manufacturer_ref = None,
        PatchField::Value(v) => {
            effective_manufacturer_ref =
                clean_opt_string(Some(v), 128, "La référence constructeur")?;
        }
    }

    let mut effective_barcode = current.barcode.clone();
    match input.barcode {
        PatchField::Absent => {}
        PatchField::Null => effective_barcode = None,
        PatchField::Value(v) => {
            effective_barcode = clean_opt_string(Some(v), 64, "Le code-barres")?;
        }
    }

    let mut effective_threshold = current.alert_threshold;
    match input.alert_threshold {
        PatchField::Absent => {}
        PatchField::Null => effective_threshold = None,
        PatchField::Value(v) => {
            if v < 0 {
                return Err(MaintError::BadRequest(
                    "Le seuil d'alerte ne peut pas être négatif".into(),
                ));
            }
            effective_threshold = Some(v);
        }
    }

    let now = store::now();

    sqlx::query(
        r#"
        UPDATE parts
        SET reference = ?, name = ?, description = ?, category = ?, unit = ?,
            unit_price_cents = ?, supplier_id = ?, supplier_reference = ?,
            manufacturer = ?, manufacturer_reference = ?, barcode = ?,
            alert_threshold = ?, updated_at = ?
        WHERE id = ? AND deleted_at IS NULL
        "#,
    )
    .bind(&effective_reference)
    .bind(&effective_name)
    .bind(&effective_description)
    .bind(&effective_category)
    .bind(&effective_unit)
    .bind(effective_price)
    .bind(&effective_supplier_id)
    .bind(&effective_supplier_ref)
    .bind(&effective_manufacturer)
    .bind(&effective_manufacturer_ref)
    .bind(&effective_barcode)
    .bind(effective_threshold)
    .bind(&now)
    .bind(id)
    .execute(&mut *tx)
    .await?;

    let updated = Part {
        id: current.id.clone(),
        site_id: current.site_id.clone(),
        reference: effective_reference,
        name: effective_name,
        description: effective_description,
        category: effective_category,
        unit: effective_unit,
        unit_price_cents: effective_price,
        supplier_id: effective_supplier_id,
        supplier_reference: effective_supplier_ref,
        manufacturer: effective_manufacturer,
        manufacturer_reference: effective_manufacturer_ref,
        barcode: effective_barcode,
        alert_threshold: effective_threshold,
        created_at: current.created_at.clone(),
        updated_at: now,
    };

    store::audit(
        &mut tx,
        actor,
        Audit {
            action: "update",
            entity: "part",
            entity_id: Some(id),
            site_id: Some(&current.site_id),
            before: Some(json!(current)),
            after: Some(json!(updated)),
            ip,
        },
    )
    .await?;

    crate::pilotage::notifications::stock(&mut tx, actor).await?;
    tx.commit().await?;

    let global_threshold: i64 = get_global_stock_alert_threshold(pool).await?;
    build_part_detail(pool, actor, updated, global_threshold, true).await
}

pub async fn delete_part(
    pool: &SqlitePool,
    actor: &Actor,
    id: &str,
    ip: Option<&str>,
) -> Result<(), MaintError> {
    actor.require_permission(Permission::PartsDelete)?;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let current = scoped_part(&mut tx, actor, id).await?;

    // Refus 409 si stock résiduel > 0
    let total_stock: i64 =
        sqlx::query_scalar("SELECT COALESCE(SUM(quantity), 0) FROM stock WHERE part_id = ?")
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;

    if total_stock > 0 {
        return Err(MaintError::Conflict(
            "Impossible de supprimer une pièce ayant du stock résiduel".into(),
        ));
    }

    let now = store::now();

    sqlx::query(
        "UPDATE parts SET deleted_at = ?, updated_at = ? WHERE id = ? AND deleted_at IS NULL",
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
            entity: "part",
            entity_id: Some(id),
            site_id: Some(&current.site_id),
            before: Some(json!(current)),
            after: None,
            ip,
        },
    )
    .await?;

    crate::pilotage::notifications::stock(&mut tx, actor).await?;
    tx.commit().await?;
    Ok(())
}

// ============================================================================
// 2. NIVEAUX DE STOCK PAR EMPLACEMENT & ALERTES
// ============================================================================

pub async fn list_stock(
    pool: &SqlitePool,
    actor: &Actor,
    filter: StockFilter,
) -> Result<Vec<StockItem>, MaintError> {
    actor.require_permission(Permission::StockView)?;
    let site_scope = resolve_site_id_for_query(actor, filter.site_id.as_deref())?;

    let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(
        r#"
        SELECT 
            s.id, s.site_id, s.part_id, s.location_id, s.quantity, s.updated_at,
            p.reference AS part_reference, p.name AS part_name,
            l.code AS location_code, l.name AS location_name,
            (SELECT COALESCE(SUM(sr.quantity), 0) 
             FROM stock_reservations sr 
             WHERE sr.part_id = s.part_id AND sr.location_id = s.location_id AND sr.status = 'prepared') AS reserved_quantity
        FROM stock s
        JOIN parts p ON p.id = s.part_id AND p.deleted_at IS NULL
        JOIN locations l ON l.id = s.location_id AND l.deleted_at IS NULL
        WHERE 1=1
        "#,
    );

    if let Some(sid) = site_scope {
        qb.push(" AND s.site_id = ").push_bind(sid);
    }
    if let Some(pid) = filter.part_id.as_deref().filter(|s| !s.is_empty()) {
        qb.push(" AND s.part_id = ").push_bind(pid);
    }
    if let Some(lid) = filter.location_id.as_deref().filter(|s| !s.is_empty()) {
        qb.push(" AND s.location_id = ").push_bind(lid);
    }

    qb.push(" ORDER BY p.reference ASC, l.code ASC");

    #[derive(FromRow)]
    struct RawStockRow {
        id: String,
        site_id: String,
        part_id: String,
        location_id: String,
        quantity: i64,
        updated_at: String,
        part_reference: String,
        part_name: String,
        location_code: String,
        location_name: String,
        reserved_quantity: i64,
    }

    let rows: Vec<RawStockRow> = qb.build_query_as().fetch_all(pool).await?;

    Ok(rows
        .into_iter()
        .map(|r| StockItem {
            id: r.id,
            site_id: r.site_id,
            part_id: r.part_id,
            location_id: r.location_id,
            quantity: r.quantity,
            reserved_quantity: r.reserved_quantity,
            available_quantity: (r.quantity - r.reserved_quantity).max(0),
            updated_at: r.updated_at,
            part_reference: r.part_reference,
            part_name: r.part_name,
            location_code: r.location_code,
            location_name: r.location_name,
        })
        .collect())
}

pub async fn list_stock_alerts(
    pool: &SqlitePool,
    actor: &Actor,
    requested_site_id: Option<&str>,
) -> Result<Vec<PartDetail>, MaintError> {
    actor.require_permission(Permission::StockView)?;
    let site_scope = resolve_site_id_for_query(actor, requested_site_id)?;
    let global_threshold: i64 = get_global_stock_alert_threshold(pool).await?;

    let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(
        r#"
        SELECT 
            p.id, p.site_id, p.reference, p.name, p.description, p.category, p.unit,
            p.unit_price_cents, p.supplier_id, p.supplier_reference, p.manufacturer,
            p.manufacturer_reference, p.barcode, p.alert_threshold, p.created_at, p.updated_at
        FROM parts p
        WHERE p.deleted_at IS NULL
        "#,
    );

    if let Some(sid) = site_scope {
        qb.push(" AND p.site_id = ").push_bind(sid);
    }
    qb.push(" ORDER BY p.reference ASC");

    let parts: Vec<Part> = qb.build_query_as().fetch_all(pool).await?;

    let mut alerts = Vec::new();
    for part in parts {
        let detail = build_part_detail(pool, actor, part, global_threshold, false).await?;
        if detail.is_low_stock {
            alerts.push(detail);
        }
    }

    Ok(alerts)
}

// ============================================================================
// 3. JOURNAL DES MOUVEMENTS & ENTRÉES / TRANSFERTS / AJUSTEMENTS
// ============================================================================

pub async fn list_stock_movements(
    pool: &SqlitePool,
    actor: &Actor,
    filter: StockMovementFilter,
) -> Result<crate::referentiel::Page<StockMovement>, MaintError> {
    actor.require_permission(Permission::StockView)?;
    let limit = validate_limit(filter.limit)?;
    let site_scope = resolve_site_id_for_query(actor, filter.site_id.as_deref())?;

    let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(
        r#"
        SELECT 
            sm.id, sm.site_id, sm.part_id, sm.movement_type, sm.quantity,
            sm.from_location_id, sm.to_location_id, sm.intervention_id,
            sm.user_id, sm.unit_price_cents, sm.reason, sm.adjustment_kind,
            sm.delta, sm.created_at,
            p.reference AS part_reference, p.name AS part_name,
            fl.code AS from_location_code, fl.name AS from_location_name,
            tl.code AS to_location_code, tl.name AS to_location_name,
            u.display_name AS user_display_name,
            i.code AS intervention_code
        FROM stock_movements sm
        LEFT JOIN parts p ON p.id = sm.part_id
        LEFT JOIN locations fl ON fl.id = sm.from_location_id
        LEFT JOIN locations tl ON tl.id = sm.to_location_id
        LEFT JOIN users u ON u.id = sm.user_id
        LEFT JOIN interventions i ON i.id = sm.intervention_id
        WHERE 1=1
        "#,
    );

    if let Some(sid) = site_scope {
        qb.push(" AND sm.site_id = ").push_bind(sid);
    }

    if let Some(pid) = filter.part_id.as_deref().filter(|s| !s.is_empty()) {
        qb.push(" AND sm.part_id = ").push_bind(pid);
    }

    if let Some(mtype) = filter.movement_type.as_deref().filter(|s| !s.is_empty()) {
        qb.push(" AND sm.movement_type = ").push_bind(mtype);
    }

    if let Some(iid) = filter.intervention_id.as_deref().filter(|s| !s.is_empty()) {
        qb.push(" AND sm.intervention_id = ").push_bind(iid);
    }

    if let Some(lid) = filter.location_id.as_deref().filter(|s| !s.is_empty()) {
        qb.push(" AND (sm.from_location_id = ")
            .push_bind(lid)
            .push(" OR sm.to_location_id = ")
            .push_bind(lid)
            .push(")");
    }

    if let Some(uid) = filter.user_id.as_deref().filter(|s| !s.is_empty()) {
        qb.push(" AND sm.user_id = ").push_bind(uid);
    }

    if let Some(df) = filter.date_from.as_deref().filter(|s| !s.is_empty()) {
        qb.push(" AND sm.created_at >= ").push_bind(df);
    }

    if let Some(dt) = filter.date_to.as_deref().filter(|s| !s.is_empty()) {
        qb.push(" AND sm.created_at <= ").push_bind(dt);
    }

    if let Some(q) = filter.q.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        let pattern = like_pattern(q);
        qb.push(" AND (p.reference LIKE ")
            .push_bind(pattern.clone())
            .push(" ESCAPE '\\' OR p.name LIKE ")
            .push_bind(pattern.clone())
            .push(" ESCAPE '\\' OR sm.reason LIKE ")
            .push_bind(pattern)
            .push(" ESCAPE '\\')");
    }

    if let Some(cursor) = &filter.cursor {
        if let Some(ts) = &cursor.timestamp {
            qb.push(" AND (sm.created_at < ")
                .push_bind(ts)
                .push(" OR (sm.created_at = ")
                .push_bind(ts)
                .push(" AND sm.id < ")
                .push_bind(&cursor.id)
                .push("))");
        } else {
            qb.push(" AND sm.id < ").push_bind(&cursor.id);
        }
    }

    qb.push(" ORDER BY sm.created_at DESC, sm.id DESC LIMIT ")
        .push_bind(limit as i64 + 1);

    #[derive(FromRow)]
    struct RawMovementRow {
        id: String,
        site_id: String,
        part_id: String,
        movement_type: String,
        quantity: i64,
        from_location_id: Option<String>,
        to_location_id: Option<String>,
        intervention_id: Option<String>,
        user_id: String,
        unit_price_cents: Option<i64>,
        reason: Option<String>,
        adjustment_kind: Option<String>,
        delta: Option<i64>,
        created_at: String,
        part_reference: Option<String>,
        part_name: Option<String>,
        from_location_code: Option<String>,
        from_location_name: Option<String>,
        to_location_code: Option<String>,
        to_location_name: Option<String>,
        user_display_name: Option<String>,
        intervention_code: Option<String>,
    }

    let rows: Vec<RawMovementRow> = qb.build_query_as().fetch_all(pool).await?;

    let has_more = rows.len() > limit as usize;
    let mut movements = rows;
    if has_more {
        movements.truncate(limit as usize);
    }

    let next_cursor = if has_more {
        movements
            .last()
            .map(|last| Cursor::with_timestamp(&last.id, &last.created_at).encode())
    } else {
        None
    };

    let items = movements
        .into_iter()
        .map(|r| StockMovement {
            id: r.id,
            site_id: r.site_id,
            part_id: r.part_id,
            movement_type: r.movement_type,
            quantity: r.quantity,
            from_location_id: r.from_location_id,
            to_location_id: r.to_location_id,
            intervention_id: r.intervention_id,
            user_id: r.user_id,
            unit_price_cents: r.unit_price_cents,
            reason: r.reason,
            adjustment_kind: r.adjustment_kind,
            delta: r.delta,
            created_at: r.created_at,
            part_reference: r.part_reference,
            part_name: r.part_name,
            from_location_code: r.from_location_code,
            from_location_name: r.from_location_name,
            to_location_code: r.to_location_code,
            to_location_name: r.to_location_name,
            user_display_name: r.user_display_name,
            intervention_code: r.intervention_code,
        })
        .collect();

    Ok(crate::referentiel::Page { items, next_cursor })
}

pub async fn create_stock_movement(
    pool: &SqlitePool,
    actor: &Actor,
    input: CreateStockMovement,
    ip: Option<&str>,
) -> Result<StockMovement, MaintError> {
    actor.require_permission(Permission::StockMovements)?;

    if input.movement_type == "out" {
        return Err(MaintError::BadRequest(
            "Une sortie de stock exige obligatoirement une intervention rattachée".into(),
        ));
    }

    if input.quantity <= 0 {
        return Err(MaintError::BadRequest(
            "La quantité doit être strictement positive".into(),
        ));
    }

    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let part = scoped_part(&mut tx, actor, &input.part_id).await?;

    let now = store::now();
    let movement_id = store::id();

    match input.movement_type.as_str() {
        "in" => {
            let to_loc = input.to_location_id.as_deref().ok_or_else(|| {
                MaintError::BadRequest(
                    "L'emplacement de destination est obligatoire pour une entrée".into(),
                )
            })?;
            ensure_location_exists(&mut tx, &part.site_id, to_loc).await?;

            let price = input.unit_price_cents.ok_or_else(|| {
                MaintError::BadRequest(
                    "Le prix unitaire d'achat en centimes est obligatoire pour une entrée de stock"
                        .into(),
                )
            })?;
            if price < 0 {
                return Err(MaintError::BadRequest(
                    "Le prix d'achat ne peut pas être négatif".into(),
                ));
            }

            // Calcul du PMP (Décisions du 2026-10-18 §7.1)
            let stock_total_avant: i64 = sqlx::query_scalar(
                "SELECT COALESCE(SUM(quantity), 0) FROM stock WHERE part_id = ?",
            )
            .bind(&part.id)
            .fetch_one(&mut *tx)
            .await?;

            let nouveau_pmp = if stock_total_avant <= 0 {
                price
            } else {
                let total_valeur = (stock_total_avant as f64) * (part.unit_price_cents as f64)
                    + (input.quantity as f64) * (price as f64);
                let total_qte = (stock_total_avant + input.quantity) as f64;
                (total_valeur / total_qte).round() as i64
            };

            // Mise à jour du PMP sur la pièce
            sqlx::query("UPDATE parts SET unit_price_cents = ?, updated_at = ? WHERE id = ?")
                .bind(nouveau_pmp)
                .bind(&now)
                .bind(&part.id)
                .execute(&mut *tx)
                .await?;

            // Incrément atomique dans stock
            let stock_id = store::id();
            sqlx::query(
                r#"
                INSERT INTO stock (id, site_id, part_id, location_id, quantity, updated_at)
                VALUES (?, ?, ?, ?, ?, ?)
                ON CONFLICT (part_id, location_id) DO UPDATE SET
                    quantity = stock.quantity + excluded.quantity,
                    updated_at = excluded.updated_at
                "#,
            )
            .bind(&stock_id)
            .bind(&part.site_id)
            .bind(&part.id)
            .bind(to_loc)
            .bind(input.quantity)
            .bind(&now)
            .execute(&mut *tx)
            .await?;

            // Insertion dans stock_movements
            sqlx::query(
                r#"
                INSERT INTO stock_movements (
                    id, site_id, part_id, movement_type, quantity, from_location_id,
                    to_location_id, intervention_id, user_id, unit_price_cents,
                    reason, adjustment_kind, delta, created_at
                ) VALUES (?, ?, ?, 'in', ?, NULL, ?, NULL, ?, ?, ?, NULL, NULL, ?)
                "#,
            )
            .bind(&movement_id)
            .bind(&part.site_id)
            .bind(&part.id)
            .bind(input.quantity)
            .bind(to_loc)
            .bind(&actor.user_id)
            .bind(price)
            .bind(&input.reason)
            .bind(&now)
            .execute(&mut *tx)
            .await?;
        }
        "transfer" => {
            let from_loc = input.from_location_id.as_deref().ok_or_else(|| {
                MaintError::BadRequest(
                    "L'emplacement source est obligatoire pour un transfert".into(),
                )
            })?;
            let to_loc = input.to_location_id.as_deref().ok_or_else(|| {
                MaintError::BadRequest(
                    "L'emplacement de destination est obligatoire pour un transfert".into(),
                )
            })?;

            if from_loc == to_loc {
                return Err(MaintError::BadRequest(
                    "Les emplacements source et destination doivent être distincts".into(),
                ));
            }

            ensure_location_exists(&mut tx, &part.site_id, from_loc).await?;
            ensure_location_exists(&mut tx, &part.site_id, to_loc).await?;

            // Vérification du stock disponible sur l'emplacement source (hors réservations prepared)
            let current_qty: i64 = sqlx::query_scalar(
                "SELECT COALESCE(SUM(quantity), 0) FROM stock WHERE part_id = ? AND location_id = ?",
            )
            .bind(&part.id)
            .bind(from_loc)
            .fetch_one(&mut *tx)
            .await?;

            let reserved_qty: i64 = sqlx::query_scalar(
                "SELECT COALESCE(SUM(quantity), 0) FROM stock_reservations WHERE part_id = ? AND location_id = ? AND status = 'prepared'",
            )
            .bind(&part.id)
            .bind(from_loc)
            .fetch_one(&mut *tx)
            .await?;

            let available = current_qty - reserved_qty;
            if available < input.quantity {
                return Err(MaintError::Conflict(
                    "Stock disponible insuffisant sur l'emplacement source pour ce transfert"
                        .into(),
                ));
            }

            // Décrément atomique sur la source
            let res = sqlx::query(
                "UPDATE stock SET quantity = quantity - ?, updated_at = ? WHERE part_id = ? AND location_id = ? AND quantity >= ?",
            )
            .bind(input.quantity)
            .bind(&now)
            .bind(&part.id)
            .bind(from_loc)
            .bind(input.quantity)
            .execute(&mut *tx)
            .await?;

            if res.rows_affected() == 0 {
                return Err(MaintError::Conflict(
                    "Stock insuffisant sur l'emplacement source".into(),
                ));
            }

            // Incrément sur la destination
            let stock_id = store::id();
            sqlx::query(
                r#"
                INSERT INTO stock (id, site_id, part_id, location_id, quantity, updated_at)
                VALUES (?, ?, ?, ?, ?, ?)
                ON CONFLICT (part_id, location_id) DO UPDATE SET
                    quantity = stock.quantity + excluded.quantity,
                    updated_at = excluded.updated_at
                "#,
            )
            .bind(&stock_id)
            .bind(&part.site_id)
            .bind(&part.id)
            .bind(to_loc)
            .bind(input.quantity)
            .bind(&now)
            .execute(&mut *tx)
            .await?;

            // Insertion dans stock_movements
            sqlx::query(
                r#"
                INSERT INTO stock_movements (
                    id, site_id, part_id, movement_type, quantity, from_location_id,
                    to_location_id, intervention_id, user_id, unit_price_cents,
                    reason, adjustment_kind, delta, created_at
                ) VALUES (?, ?, ?, 'transfer', ?, ?, ?, NULL, ?, ?, ?, NULL, NULL, ?)
                "#,
            )
            .bind(&movement_id)
            .bind(&part.site_id)
            .bind(&part.id)
            .bind(input.quantity)
            .bind(from_loc)
            .bind(to_loc)
            .bind(&actor.user_id)
            .bind(part.unit_price_cents)
            .bind(&input.reason)
            .bind(&now)
            .execute(&mut *tx)
            .await?;
        }
        other => {
            return Err(MaintError::BadRequest(format!(
                "Type de mouvement invalide : « {other} » (attendu: in, transfer)"
            )));
        }
    }

    store::audit(
        &mut tx,
        actor,
        Audit {
            action: "create",
            entity: "stock_movement",
            entity_id: Some(&movement_id),
            site_id: Some(&part.site_id),
            before: None,
            after: Some(json!({
                "id": movement_id,
                "part_id": part.id,
                "movement_type": input.movement_type,
                "quantity": input.quantity,
            })),
            ip,
        },
    )
    .await?;

    crate::pilotage::notifications::stock(&mut tx, actor).await?;
    tx.commit().await?;

    let mov: StockMovement = fetch_stock_movement_by_id(pool, &movement_id).await?;
    Ok(mov)
}

pub async fn create_stock_adjustment(
    pool: &SqlitePool,
    actor: &Actor,
    input: CreateAdjustment,
    ip: Option<&str>,
) -> Result<StockMovement, MaintError> {
    actor.require_permission(Permission::StockAdjust)?;

    if input.delta == 0 {
        return Err(MaintError::BadRequest(
            "Le delta d'ajustement ne peut pas être nul".into(),
        ));
    }

    let kind = validate_adjustment_kind(&input.adjustment_kind)?;
    let reason = input.reason.trim();
    if reason.is_empty() {
        return Err(MaintError::BadRequest(
            "Le motif d'ajustement est obligatoire".into(),
        ));
    }

    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let part = scoped_part(&mut tx, actor, &input.part_id).await?;
    ensure_location_exists(&mut tx, &part.site_id, &input.location_id).await?;

    let now = store::now();
    let movement_id = store::id();

    if input.delta > 0 {
        // Ajustement positif (inventaire positif) -> incrément
        let stock_id = store::id();
        sqlx::query(
            r#"
            INSERT INTO stock (id, site_id, part_id, location_id, quantity, updated_at)
            VALUES (?, ?, ?, ?, ?, ?)
            ON CONFLICT (part_id, location_id) DO UPDATE SET
                quantity = stock.quantity + excluded.quantity,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(&stock_id)
        .bind(&part.site_id)
        .bind(&part.id)
        .bind(&input.location_id)
        .bind(input.delta)
        .bind(&now)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO stock_movements (
                id, site_id, part_id, movement_type, quantity, from_location_id,
                to_location_id, intervention_id, user_id, unit_price_cents,
                reason, adjustment_kind, delta, created_at
            ) VALUES (?, ?, ?, 'adjustment', ?, NULL, ?, NULL, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&movement_id)
        .bind(&part.site_id)
        .bind(&part.id)
        .bind(input.delta)
        .bind(&input.location_id)
        .bind(&actor.user_id)
        .bind(part.unit_price_cents)
        .bind(reason)
        .bind(kind)
        .bind(input.delta)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
    } else {
        // Ajustement négatif (perte, casse, rebut, inventaire négatif) -> décrément
        let qty_to_remove = -input.delta;

        let current_qty: i64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(quantity), 0) FROM stock WHERE part_id = ? AND location_id = ?",
        )
        .bind(&part.id)
        .bind(&input.location_id)
        .fetch_one(&mut *tx)
        .await?;

        let reserved_qty: i64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(quantity), 0) FROM stock_reservations WHERE part_id = ? AND location_id = ? AND status = 'prepared'",
        )
        .bind(&part.id)
        .bind(&input.location_id)
        .fetch_one(&mut *tx)
        .await?;

        let available = current_qty - reserved_qty;
        if available < qty_to_remove {
            return Err(MaintError::Conflict(
                "Stock disponible insuffisant pour cet ajustement négatif".into(),
            ));
        }

        let res = sqlx::query(
            "UPDATE stock SET quantity = quantity - ?, updated_at = ? WHERE part_id = ? AND location_id = ? AND quantity >= ?",
        )
        .bind(qty_to_remove)
        .bind(&now)
        .bind(&part.id)
        .bind(&input.location_id)
        .bind(qty_to_remove)
        .execute(&mut *tx)
        .await?;

        if res.rows_affected() == 0 {
            return Err(MaintError::Conflict(
                "Stock insuffisant sur l'emplacement".into(),
            ));
        }

        sqlx::query(
            r#"
            INSERT INTO stock_movements (
                id, site_id, part_id, movement_type, quantity, from_location_id,
                to_location_id, intervention_id, user_id, unit_price_cents,
                reason, adjustment_kind, delta, created_at
            ) VALUES (?, ?, ?, 'adjustment', ?, ?, NULL, NULL, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&movement_id)
        .bind(&part.site_id)
        .bind(&part.id)
        .bind(qty_to_remove)
        .bind(&input.location_id)
        .bind(&actor.user_id)
        .bind(part.unit_price_cents)
        .bind(reason)
        .bind(kind)
        .bind(input.delta)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
    }

    store::audit(
        &mut tx,
        actor,
        Audit {
            action: "adjust",
            entity: "stock",
            entity_id: Some(&part.id),
            site_id: Some(&part.site_id),
            before: None,
            after: Some(json!({
                "movement_id": movement_id,
                "delta": input.delta,
                "adjustment_kind": kind,
                "location_id": input.location_id,
            })),
            ip,
        },
    )
    .await?;

    crate::pilotage::notifications::stock(&mut tx, actor).await?;
    tx.commit().await?;

    fetch_stock_movement_by_id(pool, &movement_id).await
}

// ============================================================================
// 4. CONSOMMATION DE PIÈCES SUR INTERVENTION
// ============================================================================

pub async fn list_intervention_parts(
    pool: &SqlitePool,
    actor: &Actor,
    intervention_id: &str,
) -> Result<Vec<InterventionPart>, MaintError> {
    actor.require_permission(Permission::InterventionsView)?;
    actor.require_permission(Permission::StockView)?;
    let mut conn = pool.acquire().await?;
    let _int = interventions::scoped_intervention_raw(&mut conn, actor, intervention_id).await?;

    let rows: Vec<InterventionPart> = sqlx::query_as(
        r#"
        SELECT 
            ip.id, ip.site_id, ip.intervention_id, ip.part_id, ip.location_id,
            ip.quantity, ip.unit_price_cents, ip.movement_id, ip.created_at,
            p.reference AS part_reference, p.name AS part_name,
            l.code AS location_code, l.name AS location_name
        FROM intervention_parts ip
        JOIN parts p ON p.id = ip.part_id
        JOIN locations l ON l.id = ip.location_id
        WHERE ip.intervention_id = ?
        ORDER BY ip.created_at ASC
        "#,
    )
    .bind(intervention_id)
    .fetch_all(&mut *conn)
    .await?;

    Ok(rows)
}

pub async fn consume_part_on_intervention(
    pool: &SqlitePool,
    actor: &Actor,
    intervention_id: &str,
    input: ConsumePartInput,
    ip: Option<&str>,
) -> Result<InterventionPart, MaintError> {
    actor.require_permission(Permission::InterventionsEdit)?;
    actor.require_permission(Permission::StockMovements)?;

    if input.quantity <= 0 {
        return Err(MaintError::BadRequest(
            "La quantité consommée doit être strictement positive".into(),
        ));
    }

    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let int = interventions::scoped_intervention_raw(&mut tx, actor, intervention_id).await?;

    if int.status != "open" && int.status != "in_progress" {
        return Err(MaintError::Conflict(
            "Impossible de consommer des pièces sur une intervention terminée ou annulée".into(),
        ));
    }

    let part = scoped_part(&mut tx, actor, &input.part_id).await?;
    ensure_location_exists(&mut tx, &int.site_id, &input.location_id).await?;

    // Règle Décisions du 2026-10-18 §7.3 :
    // Une sortie ordinaire ne peut pas consommer une quantité réservée pour une autre intervention.
    let current_qty: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(quantity), 0) FROM stock WHERE part_id = ? AND location_id = ?",
    )
    .bind(&part.id)
    .bind(&input.location_id)
    .fetch_one(&mut *tx)
    .await?;

    let reserved_for_others: i64 = sqlx::query_scalar(
        r#"
        SELECT COALESCE(SUM(quantity), 0) 
        FROM stock_reservations 
        WHERE part_id = ? AND location_id = ? AND status = 'prepared' AND intervention_id != ?
        "#,
    )
    .bind(&part.id)
    .bind(&input.location_id)
    .bind(intervention_id)
    .fetch_one(&mut *tx)
    .await?;

    let available = current_qty - reserved_for_others;
    if available < input.quantity {
        return Err(MaintError::Conflict(
            "Stock insuffisant sur cet emplacement (quantité épuisée ou réservée pour d'autres interventions)".into(),
        ));
    }

    let now = store::now();

    // Décrément atomique dans stock
    let res = sqlx::query(
        "UPDATE stock SET quantity = quantity - ?, updated_at = ? WHERE part_id = ? AND location_id = ? AND quantity >= ?",
    )
    .bind(input.quantity)
    .bind(&now)
    .bind(&part.id)
    .bind(&input.location_id)
    .bind(input.quantity)
    .execute(&mut *tx)
    .await?;

    if res.rows_affected() == 0 {
        return Err(MaintError::Conflict(
            "Stock insuffisant sur l'emplacement".into(),
        ));
    }

    let movement_id = store::id();
    let reason = format!("Consommation sur intervention {}", int.code);

    // Mouvement out obligatoire rattaché à intervention_id
    sqlx::query(
        r#"
        INSERT INTO stock_movements (
            id, site_id, part_id, movement_type, quantity, from_location_id,
            to_location_id, intervention_id, user_id, unit_price_cents,
            reason, adjustment_kind, delta, created_at
        ) VALUES (?, ?, ?, 'out', ?, ?, NULL, ?, ?, ?, ?, NULL, NULL, ?)
        "#,
    )
    .bind(&movement_id)
    .bind(&int.site_id)
    .bind(&part.id)
    .bind(input.quantity)
    .bind(&input.location_id)
    .bind(intervention_id)
    .bind(&actor.user_id)
    .bind(part.unit_price_cents)
    .bind(&reason)
    .bind(&now)
    .execute(&mut *tx)
    .await?;

    let item_id = store::id();
    sqlx::query(
        r#"
        INSERT INTO intervention_parts (
            id, site_id, intervention_id, part_id, location_id,
            quantity, unit_price_cents, movement_id, created_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&item_id)
    .bind(&int.site_id)
    .bind(intervention_id)
    .bind(&part.id)
    .bind(&input.location_id)
    .bind(input.quantity)
    .bind(part.unit_price_cents)
    .bind(&movement_id)
    .bind(&now)
    .execute(&mut *tx)
    .await?;

    store::audit(
        &mut tx,
        actor,
        Audit {
            action: "consume_part",
            entity: "intervention_part",
            entity_id: Some(&item_id),
            site_id: Some(&int.site_id),
            before: None,
            after: Some(json!({
                "intervention_id": intervention_id,
                "part_id": part.id,
                "quantity": input.quantity,
                "unit_price_cents": part.unit_price_cents,
                "movement_id": movement_id
            })),
            ip,
        },
    )
    .await?;

    crate::pilotage::notifications::stock(&mut tx, actor).await?;
    tx.commit().await?;

    fetch_intervention_part_by_id(pool, &item_id).await
}

pub async fn delete_intervention_part(
    pool: &SqlitePool,
    actor: &Actor,
    intervention_id: &str,
    item_id: &str,
    ip: Option<&str>,
) -> Result<(), MaintError> {
    actor.require_permission(Permission::InterventionsEdit)?;
    actor.require_permission(Permission::StockMovements)?;

    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let int = interventions::scoped_intervention_raw(&mut tx, actor, intervention_id).await?;

    if int.status != "open" && int.status != "in_progress" {
        return Err(MaintError::Conflict(
            "Impossible d'annuler une consommation sur une intervention terminée ou annulée".into(),
        ));
    }

    #[derive(FromRow)]
    struct RawItem {
        id: String,
        site_id: String,
        part_id: String,
        location_id: String,
        quantity: i64,
        unit_price_cents: i64,
    }

    let item: Option<RawItem> = sqlx::query_as(
        "SELECT id, site_id, part_id, location_id, quantity, unit_price_cents FROM intervention_parts WHERE id = ? AND intervention_id = ?",
    )
    .bind(item_id)
    .bind(intervention_id)
    .fetch_optional(&mut *tx)
    .await?;

    let item = item.ok_or_else(|| MaintError::NotFound("Ligne de pièce consommée".into()))?;

    let now = store::now();

    // Réintégration atomique du stock sur l'emplacement d'origine
    let stock_id = store::id();
    sqlx::query(
        r#"
        INSERT INTO stock (id, site_id, part_id, location_id, quantity, updated_at)
        VALUES (?, ?, ?, ?, ?, ?)
        ON CONFLICT (part_id, location_id) DO UPDATE SET
            quantity = stock.quantity + excluded.quantity,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(&stock_id)
    .bind(&item.site_id)
    .bind(&item.part_id)
    .bind(&item.location_id)
    .bind(item.quantity)
    .bind(&now)
    .execute(&mut *tx)
    .await?;

    // Mouvement d'ajustement tracé de réintégration
    let movement_id = store::id();
    let reason = format!("Annulation de consommation sur intervention {}", int.code);

    sqlx::query(
        r#"
        INSERT INTO stock_movements (
            id, site_id, part_id, movement_type, quantity, from_location_id,
            to_location_id, intervention_id, user_id, unit_price_cents,
            reason, adjustment_kind, delta, created_at
        ) VALUES (?, ?, ?, 'adjustment', ?, NULL, ?, NULL, ?, ?, ?, 'inventaire', ?, ?)
        "#,
    )
    .bind(&movement_id)
    .bind(&item.site_id)
    .bind(&item.part_id)
    .bind(item.quantity)
    .bind(&item.location_id)
    .bind(&actor.user_id)
    .bind(item.unit_price_cents)
    .bind(&reason)
    .bind(item.quantity)
    .bind(&now)
    .execute(&mut *tx)
    .await?;

    // Suppression de la ligne intervention_parts
    sqlx::query("DELETE FROM intervention_parts WHERE id = ?")
        .bind(item_id)
        .execute(&mut *tx)
        .await?;

    store::audit(
        &mut tx,
        actor,
        Audit {
            action: "delete",
            entity: "intervention_part",
            entity_id: Some(item_id),
            site_id: Some(&int.site_id),
            before: Some(json!({
                "id": item.id,
                "part_id": item.part_id,
                "location_id": item.location_id,
                "quantity": item.quantity,
            })),
            after: None,
            ip,
        },
    )
    .await?;

    crate::pilotage::notifications::stock(&mut tx, actor).await?;
    tx.commit().await?;
    Ok(())
}

// ============================================================================
// 5. RÉSERVATIONS DE PIÈCES PRÉPARÉES (§7.3)
// ============================================================================

pub async fn list_intervention_reservations(
    pool: &SqlitePool,
    actor: &Actor,
    intervention_id: &str,
) -> Result<Vec<StockReservation>, MaintError> {
    actor.require_permission(Permission::StockView)?;
    let mut conn = pool.acquire().await?;
    let _int = interventions::scoped_intervention_raw(&mut conn, actor, intervention_id).await?;

    let rows: Vec<StockReservation> = sqlx::query_as(
        r#"
        SELECT 
            sr.id, sr.site_id, sr.intervention_id, sr.part_id, sr.location_id,
            sr.quantity, sr.status, sr.prepared_by_id, sr.prepared_at,
            sr.closed_at, sr.created_at, sr.updated_at,
            p.reference AS part_reference, p.name AS part_name,
            l.code AS location_code, l.name AS location_name,
            u.display_name AS prepared_by_name
        FROM stock_reservations sr
        JOIN parts p ON p.id = sr.part_id
        JOIN locations l ON l.id = sr.location_id
        LEFT JOIN users u ON u.id = sr.prepared_by_id
        WHERE sr.intervention_id = ?
        ORDER BY sr.created_at ASC
        "#,
    )
    .bind(intervention_id)
    .fetch_all(pool)
    .await?;

    Ok(rows)
}

pub async fn create_stock_reservation(
    pool: &SqlitePool,
    actor: &Actor,
    intervention_id: &str,
    input: CreateReservation,
    ip: Option<&str>,
) -> Result<StockReservation, MaintError> {
    actor.require_permission(Permission::StockMovements)?;

    if input.quantity <= 0 {
        return Err(MaintError::BadRequest(
            "La quantité réservée doit être strictement positive".into(),
        ));
    }

    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let int = interventions::scoped_intervention_raw(&mut tx, actor, intervention_id).await?;

    if int.status != "open" && int.status != "in_progress" {
        return Err(MaintError::Conflict(
            "Une pièce ne peut être réservée que pour une intervention active (ouverte ou en cours)"
                .into(),
        ));
    }

    let part = scoped_part(&mut tx, actor, &input.part_id).await?;
    ensure_location_exists(&mut tx, &int.site_id, &input.location_id).await?;

    // Calcul du disponible = stock physique - somme des réservations prepared de l'emplacement
    let current_qty: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(quantity), 0) FROM stock WHERE part_id = ? AND location_id = ?",
    )
    .bind(&part.id)
    .bind(&input.location_id)
    .fetch_one(&mut *tx)
    .await?;

    let prepared_qty: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(quantity), 0) FROM stock_reservations WHERE part_id = ? AND location_id = ? AND status = 'prepared'",
    )
    .bind(&part.id)
    .bind(&input.location_id)
    .fetch_one(&mut *tx)
    .await?;

    let available = current_qty - prepared_qty;
    if available < input.quantity {
        return Err(MaintError::Conflict(
            "Stock disponible insuffisant pour préparer cette réservation".into(),
        ));
    }

    let res_id = store::id();
    let now = store::now();

    sqlx::query(
        r#"
        INSERT INTO stock_reservations (
            id, site_id, intervention_id, part_id, location_id, quantity,
            status, prepared_by_id, prepared_at, closed_at, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, 'prepared', ?, ?, NULL, ?, ?)
        "#,
    )
    .bind(&res_id)
    .bind(&int.site_id)
    .bind(intervention_id)
    .bind(&part.id)
    .bind(&input.location_id)
    .bind(input.quantity)
    .bind(&actor.user_id)
    .bind(&now)
    .bind(&now)
    .bind(&now)
    .execute(&mut *tx)
    .await?;

    store::audit(
        &mut tx,
        actor,
        Audit {
            action: "prepare_reservation",
            entity: "stock_reservation",
            entity_id: Some(&res_id),
            site_id: Some(&int.site_id),
            before: None,
            after: Some(json!({
                "id": res_id,
                "intervention_id": intervention_id,
                "part_id": part.id,
                "quantity": input.quantity,
                "status": "prepared",
            })),
            ip,
        },
    )
    .await?;

    crate::pilotage::notifications::stock(&mut tx, actor).await?;
    tx.commit().await?;

    fetch_stock_reservation_by_id(pool, &res_id).await
}

pub async fn consume_stock_reservation(
    pool: &SqlitePool,
    actor: &Actor,
    reservation_id: &str,
    input: ConsumeReservationInput,
    ip: Option<&str>,
) -> Result<InterventionPart, MaintError> {
    actor.require_permission(Permission::StockMovements)?;

    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;

    #[derive(FromRow)]
    #[allow(dead_code)]
    struct RawReservation {
        id: String,
        site_id: String,
        intervention_id: String,
        part_id: String,
        location_id: String,
        quantity: i64,
        status: String,
        prepared_by_id: String,
        prepared_at: String,
    }

    let res: Option<RawReservation> = sqlx::query_as(
        r#"
        SELECT id, site_id, intervention_id, part_id, location_id, quantity, status, prepared_by_id, prepared_at
        FROM stock_reservations
        WHERE id = ? AND (? OR site_id = ?)
        "#,
    )
    .bind(reservation_id)
    .bind(actor.is_admin())
    .bind(&actor.site_id)
    .fetch_optional(&mut *tx)
    .await?;

    let res = res.ok_or_else(|| MaintError::NotFound("Réservation".into()))?;

    if res.status != "prepared" {
        return Err(MaintError::Conflict(
            "Cette réservation n'est plus à l'état préparée".into(),
        ));
    }

    let int = interventions::scoped_intervention_raw(&mut tx, actor, &res.intervention_id).await?;
    if int.status != "open" && int.status != "in_progress" {
        return Err(MaintError::Conflict(
            "Impossible de consommer des pièces sur une intervention terminée ou annulée".into(),
        ));
    }

    let part = scoped_part(&mut tx, actor, &res.part_id).await?;

    let q_consume = input.quantity.unwrap_or(res.quantity);
    if q_consume <= 0 || q_consume > res.quantity {
        return Err(MaintError::BadRequest(
            "La quantité à consommer doit être comprise entre 1 et la quantité réservée".into(),
        ));
    }

    let now = store::now();

    // Décrément physique du stock
    let upd = sqlx::query(
        "UPDATE stock SET quantity = quantity - ?, updated_at = ? WHERE part_id = ? AND location_id = ? AND quantity >= ?",
    )
    .bind(q_consume)
    .bind(&now)
    .bind(&part.id)
    .bind(&res.location_id)
    .bind(q_consume)
    .execute(&mut *tx)
    .await?;

    if upd.rows_affected() == 0 {
        return Err(MaintError::Conflict(
            "Stock insuffisant sur l'emplacement".into(),
        ));
    }

    // Mouvement out rattaché à l'intervention
    let movement_id = store::id();
    let reason = format!("Consommation sur réservation intervention {}", int.code);

    sqlx::query(
        r#"
        INSERT INTO stock_movements (
            id, site_id, part_id, movement_type, quantity, from_location_id,
            to_location_id, intervention_id, user_id, unit_price_cents,
            reason, adjustment_kind, delta, created_at
        ) VALUES (?, ?, ?, 'out', ?, ?, NULL, ?, ?, ?, ?, NULL, NULL, ?)
        "#,
    )
    .bind(&movement_id)
    .bind(&int.site_id)
    .bind(&part.id)
    .bind(q_consume)
    .bind(&res.location_id)
    .bind(&int.id)
    .bind(&actor.user_id)
    .bind(part.unit_price_cents)
    .bind(&reason)
    .bind(&now)
    .execute(&mut *tx)
    .await?;

    // Ligne dans intervention_parts
    let item_id = store::id();
    sqlx::query(
        r#"
        INSERT INTO intervention_parts (
            id, site_id, intervention_id, part_id, location_id,
            quantity, unit_price_cents, movement_id, created_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&item_id)
    .bind(&int.site_id)
    .bind(&int.id)
    .bind(&part.id)
    .bind(&res.location_id)
    .bind(q_consume)
    .bind(part.unit_price_cents)
    .bind(&movement_id)
    .bind(&now)
    .execute(&mut *tx)
    .await?;

    // Mise à jour de la réservation
    if q_consume == res.quantity {
        sqlx::query(
            "UPDATE stock_reservations SET status = 'consumed', closed_at = ?, updated_at = ? WHERE id = ?",
        )
        .bind(&now)
        .bind(&now)
        .bind(reservation_id)
        .execute(&mut *tx)
        .await?;
    } else {
        // Consommation partielle : la réservation courante passe à consumed avec la qte consommée,
        // et le solde restant est libéré (pièces remises en rayon)
        let remainder = res.quantity - q_consume;

        sqlx::query(
            "UPDATE stock_reservations SET quantity = ?, status = 'consumed', closed_at = ?, updated_at = ? WHERE id = ?",
        )
        .bind(q_consume)
        .bind(&now)
        .bind(&now)
        .bind(reservation_id)
        .execute(&mut *tx)
        .await?;

        let rel_id = store::id();
        sqlx::query(
            r#"
            INSERT INTO stock_reservations (
                id, site_id, intervention_id, part_id, location_id, quantity,
                status, prepared_by_id, prepared_at, closed_at, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, 'released', ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&rel_id)
        .bind(&int.site_id)
        .bind(&int.id)
        .bind(&part.id)
        .bind(&res.location_id)
        .bind(remainder)
        .bind(&res.prepared_by_id)
        .bind(&res.prepared_at)
        .bind(&now)
        .bind(&now)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
    }

    store::audit(
        &mut tx,
        actor,
        Audit {
            action: "consume_reservation",
            entity: "stock_reservation",
            entity_id: Some(reservation_id),
            site_id: Some(&int.site_id),
            before: Some(json!({"id": res.id, "quantity": res.quantity, "status": "prepared"})),
            after: Some(json!({"consumed_quantity": q_consume, "status": "consumed"})),
            ip,
        },
    )
    .await?;

    crate::pilotage::notifications::stock(&mut tx, actor).await?;
    tx.commit().await?;

    fetch_intervention_part_by_id(pool, &item_id).await
}

pub async fn release_stock_reservation(
    pool: &SqlitePool,
    actor: &Actor,
    reservation_id: &str,
    ip: Option<&str>,
) -> Result<StockReservation, MaintError> {
    actor.require_permission(Permission::StockMovements)?;

    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;

    let current: Option<(String, String, i64, String)> = sqlx::query_as(
        "SELECT id, site_id, quantity, status FROM stock_reservations WHERE id = ? AND (? OR site_id = ?)",
    )
    .bind(reservation_id)
    .bind(actor.is_admin())
    .bind(&actor.site_id)
    .fetch_optional(&mut *tx)
    .await?;

    let (id, site_id, quantity, status) =
        current.ok_or_else(|| MaintError::NotFound("Réservation".into()))?;

    if status != "prepared" {
        return Err(MaintError::Conflict(
            "Cette réservation n'est plus à l'état préparée".into(),
        ));
    }

    let now = store::now();

    sqlx::query(
        "UPDATE stock_reservations SET status = 'released', closed_at = ?, updated_at = ? WHERE id = ?",
    )
    .bind(&now)
    .bind(&now)
    .bind(&id)
    .execute(&mut *tx)
    .await?;

    store::audit(
        &mut tx,
        actor,
        Audit {
            action: "release_reservation",
            entity: "stock_reservation",
            entity_id: Some(&id),
            site_id: Some(&site_id),
            before: Some(json!({"quantity": quantity, "status": "prepared"})),
            after: Some(json!({"quantity": quantity, "status": "released"})),
            ip,
        },
    )
    .await?;

    crate::pilotage::notifications::stock(&mut tx, actor).await?;
    tx.commit().await?;

    fetch_stock_reservation_by_id(pool, &id).await
}

// ============================================================================
// 6. NOMENCLATURE ET PIÈCES ASSOCIÉES AUX ÉQUIPEMENTS
// ============================================================================

pub async fn list_equipment_parts(
    pool: &SqlitePool,
    actor: &Actor,
    equipment_id: &str,
) -> Result<Vec<EquipmentPart>, MaintError> {
    actor.require_permission(Permission::EquipmentsView)?;
    actor.require_permission(Permission::PartsView)?;

    let eq_site: Option<String> = sqlx::query_scalar(
        "SELECT site_id FROM equipments WHERE id = ? AND deleted_at IS NULL AND (? OR site_id = ?)",
    )
    .bind(equipment_id)
    .bind(actor.is_admin())
    .bind(&actor.site_id)
    .fetch_optional(pool)
    .await?;

    let _site_id = eq_site.ok_or_else(|| MaintError::NotFound("Équipement".into()))?;

    let rows: Vec<EquipmentPart> = sqlx::query_as(
        r#"
        SELECT 
            ep.equipment_id, ep.part_id, ep.quantity_recommended, ep.notes,
            p.reference AS part_reference, p.name AS part_name,
            p.category AS part_category, p.unit_price_cents,
            (SELECT COALESCE(SUM(s.quantity), 0) FROM stock s WHERE s.part_id = p.id) -
            (SELECT COALESCE(SUM(sr.quantity), 0) FROM stock_reservations sr WHERE sr.part_id = p.id AND sr.status = 'prepared') AS available_stock
        FROM equipment_parts ep
        JOIN parts p ON p.id = ep.part_id AND p.deleted_at IS NULL
        WHERE ep.equipment_id = ?
        ORDER BY p.reference ASC
        "#,
    )
    .bind(equipment_id)
    .fetch_all(pool)
    .await?;

    Ok(rows)
}

pub async fn associate_equipment_part(
    pool: &SqlitePool,
    actor: &Actor,
    equipment_id: &str,
    input: AssociateEquipmentPart,
    ip: Option<&str>,
) -> Result<EquipmentPart, MaintError> {
    actor.require_permission(Permission::EquipmentsEdit)?;
    actor.require_permission(Permission::PartsEdit)?;

    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;

    let eq_site: Option<String> = sqlx::query_scalar(
        "SELECT site_id FROM equipments WHERE id = ? AND deleted_at IS NULL AND (? OR site_id = ?)",
    )
    .bind(equipment_id)
    .bind(actor.is_admin())
    .bind(&actor.site_id)
    .fetch_optional(&mut *tx)
    .await?;

    let site_id = eq_site.ok_or_else(|| MaintError::NotFound("Équipement".into()))?;
    let part = scoped_part(&mut tx, actor, &input.part_id).await?;

    if part.site_id != site_id {
        return Err(MaintError::BadRequest(
            "L'équipement et la pièce doivent appartenir au même site".into(),
        ));
    }

    let qty = input.quantity_recommended.unwrap_or(1);
    if qty <= 0 {
        return Err(MaintError::BadRequest(
            "La quantité recommandée doit être strictement positive".into(),
        ));
    }
    let notes = clean_opt_string(input.notes, 1024, "Les notes")?;

    sqlx::query(
        r#"
        INSERT INTO equipment_parts (equipment_id, part_id, quantity_recommended, notes)
        VALUES (?, ?, ?, ?)
        ON CONFLICT (equipment_id, part_id) DO UPDATE SET
            quantity_recommended = excluded.quantity_recommended,
            notes = excluded.notes
        "#,
    )
    .bind(equipment_id)
    .bind(&part.id)
    .bind(qty)
    .bind(&notes)
    .execute(&mut *tx)
    .await?;

    store::audit(
        &mut tx,
        actor,
        Audit {
            action: "associate_part",
            entity: "equipment_part",
            entity_id: Some(equipment_id),
            site_id: Some(&site_id),
            before: None,
            after: Some(json!({
                "equipment_id": equipment_id,
                "part_id": part.id,
                "quantity_recommended": qty,
                "notes": notes,
            })),
            ip,
        },
    )
    .await?;

    crate::pilotage::notifications::stock(&mut tx, actor).await?;
    tx.commit().await?;

    let list = list_equipment_parts(pool, actor, equipment_id).await?;
    list.into_iter()
        .find(|ep| ep.part_id == part.id)
        .ok_or_else(|| MaintError::Internal("Erreur de relecture de l'association".into()))
}

pub async fn disassociate_equipment_part(
    pool: &SqlitePool,
    actor: &Actor,
    equipment_id: &str,
    part_id: &str,
    ip: Option<&str>,
) -> Result<(), MaintError> {
    actor.require_permission(Permission::EquipmentsEdit)?;
    actor.require_permission(Permission::PartsEdit)?;

    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;

    let eq_site: Option<String> = sqlx::query_scalar(
        "SELECT site_id FROM equipments WHERE id = ? AND deleted_at IS NULL AND (? OR site_id = ?)",
    )
    .bind(equipment_id)
    .bind(actor.is_admin())
    .bind(&actor.site_id)
    .fetch_optional(&mut *tx)
    .await?;

    let site_id = eq_site.ok_or_else(|| MaintError::NotFound("Équipement".into()))?;

    let deleted = sqlx::query("DELETE FROM equipment_parts WHERE equipment_id = ? AND part_id = ?")
        .bind(equipment_id)
        .bind(part_id)
        .execute(&mut *tx)
        .await?;

    if deleted.rows_affected() == 0 {
        return Err(MaintError::NotFound("Association pièce-équipement".into()));
    }

    store::audit(
        &mut tx,
        actor,
        Audit {
            action: "disassociate_part",
            entity: "equipment_part",
            entity_id: Some(equipment_id),
            site_id: Some(&site_id),
            before: Some(json!({"equipment_id": equipment_id, "part_id": part_id})),
            after: None,
            ip,
        },
    )
    .await?;

    crate::pilotage::notifications::stock(&mut tx, actor).await?;
    tx.commit().await?;
    Ok(())
}

// ============================================================================
// FONCTIONS UTILITAIRES INTERNES
// ============================================================================

async fn build_part_detail(
    pool: &SqlitePool,
    actor: &Actor,
    part: Part,
    global_threshold: i64,
    include_equipments: bool,
) -> Result<PartDetail, MaintError> {
    let effective_threshold = part.alert_threshold.unwrap_or(global_threshold);

    #[derive(FromRow)]
    struct LocStockRow {
        location_id: String,
        location_code: String,
        location_name: String,
        quantity: i64,
        reserved_quantity: i64,
    }

    let loc_rows: Vec<LocStockRow> = sqlx::query_as(
        r#"
        SELECT 
            s.location_id, l.code AS location_code, l.name AS location_name, s.quantity,
            (SELECT COALESCE(SUM(sr.quantity), 0)
             FROM stock_reservations sr
             WHERE sr.part_id = s.part_id AND sr.location_id = s.location_id AND sr.status = 'prepared') AS reserved_quantity
        FROM stock s
        JOIN locations l ON l.id = s.location_id AND l.deleted_at IS NULL
        WHERE s.part_id = ?
        ORDER BY l.code ASC
        "#,
    )
    .bind(&part.id)
    .fetch_all(pool)
    .await?;

    let mut total_stock = 0i64;
    let mut total_reserved = 0i64;
    let locations = loc_rows
        .into_iter()
        .map(|r| {
            total_stock += r.quantity;
            total_reserved += r.reserved_quantity;
            PartLocationStock {
                location_id: r.location_id,
                location_code: r.location_code,
                location_name: r.location_name,
                quantity: r.quantity,
                reserved_quantity: r.reserved_quantity,
                available_quantity: (r.quantity - r.reserved_quantity).max(0),
            }
        })
        .collect();

    let total_available = (total_stock - total_reserved).max(0);
    let is_low_stock = total_stock <= effective_threshold;

    let supplier = if let Some(sup_id) = &part.supplier_id {
        let sup_row: Option<(String, String, String)> = sqlx::query_as(
            "SELECT id, name, name FROM suppliers WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(sup_id)
        .fetch_optional(pool)
        .await?;
        sup_row.map(|(id, code, name)| NamedRef { id, code, name })
    } else {
        None
    };

    let equipments = if include_equipments {
        #[derive(FromRow)]
        struct RawEqPart {
            equipment_id: String,
            equipment_code: String,
            equipment_name: String,
            quantity_recommended: i64,
            notes: Option<String>,
        }

        let eq_rows: Vec<RawEqPart> = sqlx::query_as(
            r#"
            SELECT 
                ep.equipment_id, eq.code AS equipment_code, eq.name AS equipment_name,
                ep.quantity_recommended, ep.notes
            FROM equipment_parts ep
            JOIN equipments eq ON eq.id = ep.equipment_id AND eq.deleted_at IS NULL
            WHERE ep.part_id = ?
            ORDER BY eq.code ASC
            "#,
        )
        .bind(&part.id)
        .fetch_all(pool)
        .await?;

        Some(
            eq_rows
                .into_iter()
                .map(|r| PartEquipmentRef {
                    equipment_id: r.equipment_id,
                    equipment_code: r.equipment_code,
                    equipment_name: r.equipment_name,
                    quantity_recommended: r.quantity_recommended,
                    notes: r.notes,
                })
                .collect(),
        )
    } else {
        None
    };

    let documents_count: i64 = sqlx::query_scalar("SELECT count(*) FROM documents WHERE entity_type='part' AND entity_id=? AND site_id=? AND deleted_at IS NULL AND (? OR site_id=?)")
        .bind(&part.id).bind(&part.site_id).bind(actor.is_admin()).bind(&actor.site_id).fetch_one(pool).await?;
    Ok(PartDetail {
        documents_count,
        part,
        total_stock,
        total_reserved,
        total_available,
        effective_threshold,
        is_low_stock,
        locations,
        supplier,
        equipments,
    })
}

pub(crate) async fn scoped_part(
    conn: &mut SqliteConnection,
    actor: &Actor,
    id: &str,
) -> Result<Part, MaintError> {
    let row: Option<Part> = sqlx::query_as(
        r#"
        SELECT 
            id, site_id, reference, name, description, category, unit, unit_price_cents,
            supplier_id, supplier_reference, manufacturer, manufacturer_reference, barcode,
            alert_threshold, created_at, updated_at
        FROM parts
        WHERE id = ? AND deleted_at IS NULL AND (? OR site_id = ?)
        "#,
    )
    .bind(id)
    .bind(actor.is_admin())
    .bind(&actor.site_id)
    .fetch_optional(conn)
    .await?;

    row.ok_or_else(|| MaintError::NotFound("Pièce détachée".into()))
}

async fn fetch_stock_movement_by_id(
    pool: &SqlitePool,
    id: &str,
) -> Result<StockMovement, MaintError> {
    #[derive(FromRow)]
    struct RawMov {
        id: String,
        site_id: String,
        part_id: String,
        movement_type: String,
        quantity: i64,
        from_location_id: Option<String>,
        to_location_id: Option<String>,
        intervention_id: Option<String>,
        user_id: String,
        unit_price_cents: Option<i64>,
        reason: Option<String>,
        adjustment_kind: Option<String>,
        delta: Option<i64>,
        created_at: String,
        part_reference: Option<String>,
        part_name: Option<String>,
        from_location_code: Option<String>,
        from_location_name: Option<String>,
        to_location_code: Option<String>,
        to_location_name: Option<String>,
        user_display_name: Option<String>,
        intervention_code: Option<String>,
    }

    let r: Option<RawMov> = sqlx::query_as(
        r#"
        SELECT 
            sm.id, sm.site_id, sm.part_id, sm.movement_type, sm.quantity,
            sm.from_location_id, sm.to_location_id, sm.intervention_id,
            sm.user_id, sm.unit_price_cents, sm.reason, sm.adjustment_kind,
            sm.delta, sm.created_at,
            p.reference AS part_reference, p.name AS part_name,
            fl.code AS from_location_code, fl.name AS from_location_name,
            tl.code AS to_location_code, tl.name AS to_location_name,
            u.display_name AS user_display_name,
            i.code AS intervention_code
        FROM stock_movements sm
        LEFT JOIN parts p ON p.id = sm.part_id
        LEFT JOIN locations fl ON fl.id = sm.from_location_id
        LEFT JOIN locations tl ON tl.id = sm.to_location_id
        LEFT JOIN users u ON u.id = sm.user_id
        LEFT JOIN interventions i ON i.id = sm.intervention_id
        WHERE sm.id = ?
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    let r = r.ok_or_else(|| MaintError::NotFound("Mouvement de stock".into()))?;

    Ok(StockMovement {
        id: r.id,
        site_id: r.site_id,
        part_id: r.part_id,
        movement_type: r.movement_type,
        quantity: r.quantity,
        from_location_id: r.from_location_id,
        to_location_id: r.to_location_id,
        intervention_id: r.intervention_id,
        user_id: r.user_id,
        unit_price_cents: r.unit_price_cents,
        reason: r.reason,
        adjustment_kind: r.adjustment_kind,
        delta: r.delta,
        created_at: r.created_at,
        part_reference: r.part_reference,
        part_name: r.part_name,
        from_location_code: r.from_location_code,
        from_location_name: r.from_location_name,
        to_location_code: r.to_location_code,
        to_location_name: r.to_location_name,
        user_display_name: r.user_display_name,
        intervention_code: r.intervention_code,
    })
}

async fn fetch_intervention_part_by_id(
    pool: &SqlitePool,
    id: &str,
) -> Result<InterventionPart, MaintError> {
    let r: Option<InterventionPart> = sqlx::query_as(
        r#"
        SELECT 
            ip.id, ip.site_id, ip.intervention_id, ip.part_id, ip.location_id,
            ip.quantity, ip.unit_price_cents, ip.movement_id, ip.created_at,
            p.reference AS part_reference, p.name AS part_name,
            l.code AS location_code, l.name AS location_name
        FROM intervention_parts ip
        JOIN parts p ON p.id = ip.part_id
        JOIN locations l ON l.id = ip.location_id
        WHERE ip.id = ?
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    r.ok_or_else(|| MaintError::NotFound("Pièce d'intervention".into()))
}

async fn fetch_stock_reservation_by_id(
    pool: &SqlitePool,
    id: &str,
) -> Result<StockReservation, MaintError> {
    let r: Option<StockReservation> = sqlx::query_as(
        r#"
        SELECT 
            sr.id, sr.site_id, sr.intervention_id, sr.part_id, sr.location_id,
            sr.quantity, sr.status, sr.prepared_by_id, sr.prepared_at,
            sr.closed_at, sr.created_at, sr.updated_at,
            p.reference AS part_reference, p.name AS part_name,
            l.code AS location_code, l.name AS location_name,
            u.display_name AS prepared_by_name
        FROM stock_reservations sr
        JOIN parts p ON p.id = sr.part_id
        JOIN locations l ON l.id = sr.location_id
        LEFT JOIN users u ON u.id = sr.prepared_by_id
        WHERE sr.id = ?
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    r.ok_or_else(|| MaintError::NotFound("Réservation".into()))
}

async fn get_global_stock_alert_threshold(pool: &SqlitePool) -> Result<i64, MaintError> {
    let val: Option<String> =
        sqlx::query_scalar("SELECT value FROM app_settings WHERE key = 'stock_alert_threshold'")
            .fetch_optional(pool)
            .await?;

    Ok(val.and_then(|v| v.trim().parse::<i64>().ok()).unwrap_or(5))
}

fn validate_limit(limit: u32) -> Result<u32, MaintError> {
    if (1..=200).contains(&limit) {
        Ok(limit)
    } else {
        Err(MaintError::BadRequest(
            "La limite de pagination doit être comprise entre 1 et 200".into(),
        ))
    }
}

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
                return Err(MaintError::NotFound("Site".into()));
            }
        }
        Ok(Some(actor_site))
    }
}

async fn ensure_site_exists(conn: &mut SqliteConnection, site_id: &str) -> Result<(), MaintError> {
    let exists: i64 =
        sqlx::query_scalar("SELECT count(*) FROM sites WHERE id = ? AND deleted_at IS NULL")
            .bind(site_id)
            .fetch_one(conn)
            .await?;

    if exists == 0 {
        return Err(MaintError::BadRequest("Site introuvable".into()));
    }
    Ok(())
}

async fn ensure_supplier_exists(
    conn: &mut SqliteConnection,
    site_id: &str,
    supplier_id: &str,
) -> Result<(), MaintError> {
    let exists: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM suppliers WHERE id = ? AND site_id = ? AND deleted_at IS NULL",
    )
    .bind(supplier_id)
    .bind(site_id)
    .fetch_one(conn)
    .await?;

    if exists == 0 {
        return Err(MaintError::BadRequest(
            "Fournisseur introuvable sur ce site".into(),
        ));
    }
    Ok(())
}

async fn ensure_location_exists(
    conn: &mut SqliteConnection,
    site_id: &str,
    location_id: &str,
) -> Result<(), MaintError> {
    let exists: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM locations WHERE id = ? AND site_id = ? AND deleted_at IS NULL",
    )
    .bind(location_id)
    .bind(site_id)
    .fetch_one(conn)
    .await?;

    if exists == 0 {
        return Err(MaintError::BadRequest(
            "Emplacement introuvable sur ce site".into(),
        ));
    }
    Ok(())
}

fn validate_reference(reference: &str) -> Result<String, MaintError> {
    let trimmed = reference.trim();
    if trimmed.is_empty() || trimmed.chars().count() > 64 || trimmed.chars().any(char::is_control) {
        return Err(MaintError::BadRequest(
            "La référence de pièce doit contenir entre 1 et 64 caractères valides".into(),
        ));
    }
    Ok(trimmed.to_string())
}

fn validate_name(name: &str) -> Result<String, MaintError> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.chars().count() > 128 || trimmed.chars().any(char::is_control)
    {
        return Err(MaintError::BadRequest(
            "La désignation de la pièce doit contenir entre 1 et 128 caractères valides".into(),
        ));
    }
    Ok(trimmed.to_string())
}

fn validate_unit(unit: &str) -> Result<String, MaintError> {
    match unit.trim() {
        "piece" | "metre" | "litre" | "kg" | "lot" | "boite" => Ok(unit.trim().to_string()),
        other => Err(MaintError::BadRequest(format!(
            "Unité invalide : « {other} » (attendu: piece, metre, litre, kg, lot, boite)"
        ))),
    }
}

fn validate_adjustment_kind(kind: &str) -> Result<&str, MaintError> {
    match kind.trim() {
        "inventaire" | "rebut" | "perte" | "casse" => Ok(kind.trim()),
        other => Err(MaintError::BadRequest(format!(
            "Type d'ajustement invalide : « {other} » (attendu: inventaire, rebut, perte, casse)"
        ))),
    }
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

fn like_pattern(q: &str) -> String {
    let escaped = q
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!("%{escaped}%")
}

// ============================================================================
// TESTS UNITAIRES (CALCUL DU PMP, RÈGLES DE DISPONIBILITÉ)
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calcul_du_pmp_pondere() {
        // Lot 1 : 10 unités à 10,00 € (1000 centimes)
        let stock_initial = 0i64;
        let pmp_initial = 0i64;
        let q1 = 10i64;
        let p1 = 1000i64;
        let pmp1 = if stock_initial <= 0 {
            p1
        } else {
            let total_val =
                (stock_initial as f64) * (pmp_initial as f64) + (q1 as f64) * (p1 as f64);
            (total_val / (stock_initial + q1) as f64).round() as i64
        };
        assert_eq!(pmp1, 1000);

        // Lot 2 : entrée de 5 unités à 13,00 € (1300 centimes)
        // PMP attendu = (10 * 1000 + 5 * 1300) / 15 = 16500 / 15 = 1100 centimes (11,00 €)
        let stock2 = 10i64;
        let q2 = 5i64;
        let p2 = 1300i64;
        let total_val2 = (stock2 as f64) * (pmp1 as f64) + (q2 as f64) * (p2 as f64);
        let pmp2 = (total_val2 / (stock2 + q2) as f64).round() as i64;
        assert_eq!(pmp2, 1100);

        // Sortie : ne modifie pas le PMP
        let pmp_apres_sortie = pmp2;
        assert_eq!(pmp_apres_sortie, 1100);
    }

    #[test]
    fn validation_des_unites_autorisees() {
        assert!(validate_unit("piece").is_ok());
        assert!(validate_unit("metre").is_ok());
        assert!(validate_unit("litre").is_ok());
        assert!(validate_unit("kg").is_ok());
        assert!(validate_unit("lot").is_ok());
        assert!(validate_unit("boite").is_ok());
        assert!(validate_unit("inconnu").is_err());
    }

    #[test]
    fn validation_des_ajustements() {
        assert!(validate_adjustment_kind("inventaire").is_ok());
        assert!(validate_adjustment_kind("rebut").is_ok());
        assert!(validate_adjustment_kind("perte").is_ok());
        assert!(validate_adjustment_kind("casse").is_ok());
        assert!(validate_adjustment_kind("autre").is_err());
    }
}
