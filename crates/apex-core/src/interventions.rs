//! Module de gestion des interventions de maintenance (Phase 3).
//!
//! Ce module implémente le cycle de vie complet des interventions (corrective,
//! préventive, a posteriori, hors équipement), les transitions d'état strictement
//! encadrées avec réponses 409, l'impact sur le statut opérationnel des équipements,
//! les sous-interventions à profondeur 1, l'assignation des techniciens du même site,
//! et le suivi des temps par technicien conformément aux décisions du 2026-10-18.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::json;
use sqlx::{FromRow, QueryBuilder, Sqlite, SqliteConnection, SqlitePool};

use crate::{
    domain::pagination::Cursor,
    domain::sql::{ensure_site_exists, like_pattern, validate_limit},
    referentiel::{NamedRef, PatchField},
    store::{self, Audit},
    Actor, MaintError, Permission, Role,
};

/// Analyse une date RFC 3339 reçue du client et la normalise en UTC (millisecondes, `Z`),
/// au format de `store::now()` : la valeur enregistrée est sûre à l'affichage et les
/// comparaisons textuelles entre dates restent chronologiques.
pub(crate) fn normalize_date(
    value: Option<String>,
    field: &str,
) -> Result<Option<String>, MaintError> {
    let Some(raw) = value
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    else {
        return Ok(None);
    };
    chrono::DateTime::parse_from_rfc3339(&raw)
        .map(|d| {
            Some(
                d.with_timezone(&chrono::Utc)
                    .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            )
        })
        .map_err(|_| {
            MaintError::BadRequest(format!(
                "Date invalide pour « {field} » (format RFC 3339 attendu)"
            ))
        })
}

// ============================================================================
// ÉNUMÉRATIONS ET MACHINE À ÉTATS FINIS
// ============================================================================

/// Statuts du cycle de vie d'une intervention.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum InterventionStatus {
    Open,
    InProgress,
    Completed,
    Cancelled,
}

impl InterventionStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn parse(s: &str) -> Result<Self, MaintError> {
        match s.trim() {
            "open" => Ok(Self::Open),
            "in_progress" => Ok(Self::InProgress),
            "completed" => Ok(Self::Completed),
            "cancelled" => Ok(Self::Cancelled),
            other => Err(MaintError::BadRequest(format!(
                "Statut d'intervention invalide : « {other} »"
            ))),
        }
    }
}

impl fmt::Display for InterventionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for InterventionStatus {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for InterventionStatus {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::parse(&s).map_err(serde::de::Error::custom)
    }
}

/// Types d'interventions pris en charge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InterventionType {
    Corrective,
    Preventive,
    APosteriori,
    HorsEquipement,
}

impl InterventionType {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Corrective => "corrective",
            Self::Preventive => "preventive",
            Self::APosteriori => "a_posteriori",
            Self::HorsEquipement => "hors_equipement",
        }
    }

    pub fn parse(s: &str) -> Result<Self, MaintError> {
        match s.trim() {
            "corrective" => Ok(Self::Corrective),
            "preventive" => Ok(Self::Preventive),
            "a_posteriori" => Ok(Self::APosteriori),
            "hors_equipement" => Ok(Self::HorsEquipement),
            other => Err(MaintError::BadRequest(format!(
                "Type d'intervention invalide : « {other} »"
            ))),
        }
    }
}

impl fmt::Display for InterventionType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for InterventionType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for InterventionType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::parse(&s).map_err(serde::de::Error::custom)
    }
}

/// Niveaux de priorité d'une intervention.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InterventionPriority {
    Low,
    Medium,
    High,
    Critical,
}

impl InterventionPriority {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Critical => "critical",
        }
    }

    pub fn parse(s: &str) -> Result<Self, MaintError> {
        match s.trim() {
            "low" => Ok(Self::Low),
            "medium" => Ok(Self::Medium),
            "high" => Ok(Self::High),
            "critical" => Ok(Self::Critical),
            other => Err(MaintError::BadRequest(format!(
                "Priorité d'intervention invalide : « {other} »"
            ))),
        }
    }
}

impl fmt::Display for InterventionPriority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for InterventionPriority {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for InterventionPriority {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::parse(&s).map_err(serde::de::Error::custom)
    }
}

/// État machine lié à l'intervention.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MachineState {
    Arret,
    Degrade,
    Fonctionnel,
}

impl MachineState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Arret => "ARRET",
            Self::Degrade => "DEGRADE",
            Self::Fonctionnel => "FONCTIONNEL",
        }
    }

    pub fn parse(s: &str) -> Result<Self, MaintError> {
        match s.trim() {
            "ARRET" => Ok(Self::Arret),
            "DEGRADE" => Ok(Self::Degrade),
            "FONCTIONNEL" => Ok(Self::Fonctionnel),
            other => Err(MaintError::BadRequest(format!(
                "État machine invalide : « {other} »"
            ))),
        }
    }
}

impl fmt::Display for MachineState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for MachineState {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for MachineState {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::parse(&s).map_err(serde::de::Error::custom)
    }
}

/// Vérifie si une transition d'état d'intervention est formellement autorisée.
///
/// Transitions valides :
/// - `open` -> `in_progress` (prise en charge)
/// - `open` -> `completed` (clôture directe / a posteriori)
/// - `open` -> `cancelled` (annulation)
/// - `in_progress` -> `completed` (clôture)
/// - `in_progress` -> `cancelled` (annulation)
///
/// Toute autre transition renvoie HTTP 409 Conflict.
pub fn check_transition(
    from: InterventionStatus,
    to: InterventionStatus,
) -> Result<(), MaintError> {
    use InterventionStatus::*;
    match (from, to) {
        (Open, InProgress)
        | (Open, Completed)
        | (Open, Cancelled)
        | (InProgress, Completed)
        | (InProgress, Cancelled) => Ok(()),
        _ => Err(MaintError::Conflict(format!(
            "Transition de statut interdite de « {} » vers « {} »",
            from.as_str(),
            to.as_str()
        ))),
    }
}

// ============================================================================
// MODÈLES DE DONNÉES ET REPRÉSENTATIONS JSON
// ============================================================================

/// Référence légère vers un utilisateur (demandeur ou technicien).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, FromRow)]
pub struct UserRef {
    pub id: String,
    pub display_name: String,
}

/// Référence légère vers une intervention parente.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, FromRow)]
pub struct ParentRef {
    pub id: String,
    pub code: String,
    pub title: String,
}

/// Technicien assigné à une intervention.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, FromRow)]
pub struct AssignedTechnician {
    pub user_id: String,
    pub display_name: String,
    pub username: String,
    pub is_lead: bool,
    pub assigned_at: String,
    pub duration_minutes: Option<i64>,
}

/// Représentation complète d'une intervention.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Intervention {
    pub id: String,
    pub site_id: String,
    pub code: String,
    pub title: String,
    pub description: Option<String>,
    pub r#type: String,
    pub priority: String,
    pub status: String,
    pub machine_state: String,
    pub equipment_id: Option<String>,
    pub parent_id: Option<String>,
    pub reported_by_id: String,
    pub reported_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub duration_minutes: Option<i64>,
    pub solution_report: Option<String>,
    pub cancelled_at: Option<String>,
    pub cancelled_by_id: Option<String>,
    pub cancel_reason: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub equipment: Option<NamedRef>,
    pub reported_by: Option<UserRef>,
    pub cancelled_by: Option<UserRef>,
    pub parent: Option<ParentRef>,
    pub technicians: Vec<AssignedTechnician>,
    pub sub_interventions_count: i64,
    pub documents_count: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub_interventions: Option<Vec<Intervention>>,
    /// Planning préventif d'origine `{id, code, title}` (fiche détaillée, intervention générée).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maintenance_plan: Option<serde_json::Value>,
    /// Échéance traitée `{id, due_date, due_counter, status}` (fiche détaillée).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maintenance_occurrence: Option<serde_json::Value>,
}

/// Corps de requête pour la création d'une intervention.
#[derive(Debug, Clone, Deserialize)]
pub struct CreateIntervention {
    pub site_id: Option<String>,
    pub code: Option<String>,
    pub title: String,
    pub description: Option<String>,
    pub r#type: String,
    pub priority: Option<String>,
    pub machine_state: Option<String>,
    pub equipment_id: Option<String>,
    pub parent_id: Option<String>,
    pub reported_at: Option<String>,
    pub technician_ids: Option<Vec<String>>,
    // Pour a_posteriori ou clôture directe
    pub solution_report: Option<String>,
    pub duration_minutes: Option<i64>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

/// Corps de requête pour la mise à jour partielle (PATCH).
#[derive(Debug, Clone, Deserialize, Default)]
pub struct UpdateIntervention {
    pub title: Option<String>,
    #[serde(default)]
    pub description: PatchField<String>,
    #[serde(default)]
    pub solution_report: PatchField<String>,
    pub priority: Option<String>,
    pub machine_state: Option<String>,
}

/// Corps de prise en charge (/take).
#[derive(Debug, Clone, Deserialize, Default)]
pub struct TakeIntervention {
    pub technician_ids: Option<Vec<String>>,
}

/// Corps de clôture (/complete).
#[derive(Debug, Clone, Deserialize)]
pub struct CompleteIntervention {
    pub solution_report: String,
    pub completed_at: Option<String>,
    pub final_equipment_status: Option<String>,
}

/// Corps d'annulation (/cancel).
#[derive(Debug, Clone, Deserialize)]
pub struct CancelIntervention {
    pub reason: String,
}

/// Corps d'assignation d'un technicien.
#[derive(Debug, Clone, Deserialize)]
pub struct AssignTechnician {
    pub user_id: String,
    #[serde(default)]
    pub is_lead: bool,
}

/// Corps de saisie du temps d'un technicien (/technicians/{user_id}/time).
#[derive(Debug, Clone, Deserialize)]
pub struct SetTechnicianTime {
    pub duration_minutes: i64,
}

/// Filtres de recherche pour la liste des interventions.
#[derive(Debug, Clone, Default)]
pub struct InterventionFilter {
    pub q: Option<String>,
    pub site_id: Option<String>,
    pub status: Option<String>,
    pub r#type: Option<String>,
    pub priority: Option<String>,
    pub machine_state: Option<String>,
    pub equipment_id: Option<String>,
    pub assigned_to: Option<String>,
    pub parent_id: Option<String>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    pub cursor: Option<Cursor>,
    pub limit: u32,
}

// Ligne brute retournée par SQL avec les jointures
#[derive(Debug, FromRow)]
pub(crate) struct RawInterventionRow {
    pub(crate) id: String,
    pub(crate) site_id: String,
    pub(crate) code: String,
    pub(crate) title: String,
    pub(crate) description: Option<String>,
    #[sqlx(rename = "type")]
    pub(crate) r#type: String,
    pub(crate) priority: String,
    pub(crate) status: String,
    pub(crate) machine_state: String,
    pub(crate) equipment_id: Option<String>,
    pub(crate) parent_id: Option<String>,
    pub(crate) reported_by_id: String,
    pub(crate) reported_at: String,
    pub(crate) started_at: Option<String>,
    pub(crate) completed_at: Option<String>,
    pub(crate) duration_minutes: Option<i64>,
    pub(crate) solution_report: Option<String>,
    pub(crate) cancelled_at: Option<String>,
    pub(crate) cancelled_by_id: Option<String>,
    pub(crate) cancel_reason: Option<String>,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
    pub(crate) equipment_code: Option<String>,
    pub(crate) equipment_name: Option<String>,
    pub(crate) reported_by_name: Option<String>,
    pub(crate) cancelled_by_name: Option<String>,
    pub(crate) parent_code: Option<String>,
    pub(crate) parent_title: Option<String>,
    pub(crate) sub_interventions_count: i64,
    pub(crate) documents_count: i64,
}

#[derive(Debug, FromRow)]
struct RawTechRow {
    intervention_id: String,
    user_id: String,
    is_lead: i64,
    assigned_at: String,
    duration_minutes: Option<i64>,
    display_name: String,
    username: String,
}

// ============================================================================
// SERVICES ET FONCTIONS DE DÉPÔT
// ============================================================================

const INTERVENTION_SELECT_FIELDS: &str = r#"i.id,
            i.site_id,
            i.code,
            i.title,
            i.description,
            i.type,
            i.priority,
            i.status,
            i.machine_state,
            i.equipment_id,
            i.parent_id,
            i.reported_by_id,
            i.reported_at,
            i.started_at,
            i.completed_at,
            i.duration_minutes,
            i.solution_report,
            i.cancelled_at,
            i.cancelled_by_id,
            i.cancel_reason,
            i.created_at,
            i.updated_at,
            eq.code AS equipment_code,
            eq.name AS equipment_name,
            u_rep.display_name AS reported_by_name,
            u_can.display_name AS cancelled_by_name,
            p.code AS parent_code,
            p.title AS parent_title,
            (SELECT count(*) FROM interventions sub WHERE sub.parent_id = i.id AND sub.deleted_at IS NULL) AS sub_interventions_count,
            (SELECT count(*) FROM documents d WHERE d.site_id=i.site_id AND d.entity_type='intervention' AND d.entity_id=i.id AND d.deleted_at IS NULL) AS documents_count"#;
const INTERVENTION_FROM_JOINS: &str = r#"FROM interventions i
LEFT JOIN equipments eq ON eq.id = i.equipment_id
        LEFT JOIN users u_rep ON u_rep.id = i.reported_by_id
        LEFT JOIN users u_can ON u_can.id = i.cancelled_by_id
        LEFT JOIN interventions p ON p.id = i.parent_id"#;

/// Liste paginée des interventions avec filtres multiples.
pub async fn list_interventions(
    pool: &SqlitePool,
    actor: &Actor,
    filter: InterventionFilter,
) -> Result<crate::referentiel::Page<Intervention>, MaintError> {
    actor.require_permission(Permission::InterventionsView)?;
    let limit = validate_limit(filter.limit)?;
    let site_scope = resolve_site_id_for_query(actor, filter.site_id.as_deref())?;

    let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(format!(
        "SELECT {INTERVENTION_SELECT_FIELDS} {INTERVENTION_FROM_JOINS} WHERE i.deleted_at IS NULL"
    ));

    if let Some(sid) = site_scope {
        qb.push(" AND i.site_id = ").push_bind(sid);
    }

    if let Some(status) = filter
        .status
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        qb.push(" AND i.status = ").push_bind(status);
    }

    if let Some(typ) = filter
        .r#type
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        qb.push(" AND i.type = ").push_bind(typ);
    }

    if let Some(prio) = filter
        .priority
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        qb.push(" AND i.priority = ").push_bind(prio);
    }

    if let Some(ms) = filter
        .machine_state
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        qb.push(" AND i.machine_state = ").push_bind(ms);
    }

    if let Some(eq_id) = filter
        .equipment_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        qb.push(" AND i.equipment_id = ").push_bind(eq_id);
    }

    if let Some(assigned) = filter
        .assigned_to
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        qb.push(" AND EXISTS (SELECT 1 FROM intervention_technicians it WHERE it.intervention_id = i.id AND it.user_id = ")
            .push_bind(assigned)
            .push(")");
    }

    if let Some(pid) = filter
        .parent_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if pid == "root" {
            qb.push(" AND i.parent_id IS NULL");
        } else {
            qb.push(" AND i.parent_id = ").push_bind(pid);
        }
    }

    if let Some(df) = filter
        .date_from
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        qb.push(" AND i.reported_at >= ").push_bind(df);
    }

    if let Some(dt) = filter
        .date_to
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        qb.push(" AND i.reported_at <= ").push_bind(dt);
    }

    if let Some(q) = filter.q.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        let pattern = like_pattern(q);
        qb.push(" AND (i.code LIKE ")
            .push_bind(pattern.clone())
            .push(" ESCAPE '\\' OR i.title LIKE ")
            .push_bind(pattern.clone())
            .push(" ESCAPE '\\' OR (i.description IS NOT NULL AND i.description LIKE ")
            .push_bind(pattern)
            .push(" ESCAPE '\\'))");
    }

    if let Some(cursor) = filter.cursor {
        if let Some(ts) = cursor.timestamp {
            qb.push(" AND (i.created_at < ")
                .push_bind(ts.clone())
                .push(" OR (i.created_at = ")
                .push_bind(ts)
                .push(" AND i.id < ")
                .push_bind(cursor.id)
                .push("))");
        } else {
            qb.push(" AND i.id < ").push_bind(cursor.id);
        }
    }

    qb.push(" ORDER BY i.created_at DESC, i.id DESC LIMIT ")
        .push_bind((limit + 1) as i64);

    let mut rows: Vec<RawInterventionRow> = qb.build_query_as().fetch_all(pool).await?;

    let next_cursor = if rows.len() > limit as usize {
        rows.truncate(limit as usize);
        rows.last()
            .map(|last| Cursor::with_timestamp(&last.id, &last.created_at).encode())
    } else {
        None
    };

    // Chargement groupé des techniciens
    let ids: Vec<String> = rows.iter().map(|r| r.id.clone()).collect();
    let tech_map = fetch_technicians_for_interventions(pool, &ids).await?;

    let items = rows
        .into_iter()
        .map(|r| {
            let technicians = tech_map.get(&r.id).cloned().unwrap_or_default();
            assemble_intervention(r, technicians, None)
        })
        .collect();

    Ok(crate::referentiel::Page { items, next_cursor })
}

/// Récupère le détail complet d'une intervention avec ses sous-interventions directes.
/// Ligne de liaison intervention -> occurrence -> planning : id, due_date, due_counter, status de
/// l'occurrence, puis id, code, title du planning.
type PlanLinkRow = (
    String,
    Option<String>,
    Option<i64>,
    String,
    String,
    String,
    String,
);

pub async fn get_intervention(
    pool: &SqlitePool,
    actor: &Actor,
    id: &str,
) -> Result<Intervention, MaintError> {
    actor.require_permission(Permission::InterventionsView)?;
    let mut conn = pool.acquire().await?;
    let raw = scoped_intervention_raw(&mut conn, actor, id).await?;
    drop(conn);

    let tech_map = fetch_technicians_for_interventions(pool, std::slice::from_ref(&raw.id)).await?;
    let technicians = tech_map.get(&raw.id).cloned().unwrap_or_default();

    // Sous-interventions directes
    let sub_rows: Vec<RawInterventionRow> = sqlx::query_as(
        &format!("SELECT {INTERVENTION_SELECT_FIELDS} {INTERVENTION_FROM_JOINS} WHERE i.parent_id = ? AND i.deleted_at IS NULL ORDER BY i.created_at DESC, i.id DESC"),
    )
    .bind(id)
    .fetch_all(pool)
    .await?;

    let sub_ids: Vec<String> = sub_rows.iter().map(|r| r.id.clone()).collect();
    let sub_tech_map = fetch_technicians_for_interventions(pool, &sub_ids).await?;

    let sub_interventions: Vec<Intervention> = sub_rows
        .into_iter()
        .map(|r| {
            let t = sub_tech_map.get(&r.id).cloned().unwrap_or_default();
            assemble_intervention(r, t, None)
        })
        .collect();

    let mut intervention = assemble_intervention(raw, technicians, Some(sub_interventions));

    // Lien vers le planning préventif d'origine (phase 5)
    let link: Option<PlanLinkRow> = sqlx::query_as(
        r#"
            SELECT o.id, o.due_date, o.due_counter, o.status, p.id, p.code, p.title
            FROM interventions i
            JOIN maintenance_occurrences o ON o.id = i.maintenance_occurrence_id
            JOIN maintenance_plans p ON p.id = o.plan_id
            WHERE i.id = ?
            "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    if let Some((occ_id, due_date, due_counter, status, plan_id, code, title)) = link {
        intervention.maintenance_plan =
            Some(serde_json::json!({ "id": plan_id, "code": code, "title": title }));
        intervention.maintenance_occurrence = Some(serde_json::json!({
            "id": occ_id, "due_date": due_date, "due_counter": due_counter, "status": status
        }));
    }
    Ok(intervention)
}

/// Liste des sous-interventions rattachées à une intervention parente.
pub async fn list_sub_interventions(
    pool: &SqlitePool,
    actor: &Actor,
    parent_id: &str,
) -> Result<Vec<Intervention>, MaintError> {
    actor.require_permission(Permission::InterventionsView)?;
    let mut conn = pool.acquire().await?;
    // Vérification de l'existence et du périmètre du parent (404 si autre site)
    let _parent = scoped_intervention_raw(&mut conn, actor, parent_id).await?;
    drop(conn);

    let sub_rows: Vec<RawInterventionRow> = sqlx::query_as(
        &format!("SELECT {INTERVENTION_SELECT_FIELDS} {INTERVENTION_FROM_JOINS} WHERE i.parent_id = ? AND i.deleted_at IS NULL ORDER BY i.created_at DESC, i.id DESC"),
    )
    .bind(parent_id)
    .fetch_all(pool)
    .await?;

    let sub_ids: Vec<String> = sub_rows.iter().map(|r| r.id.clone()).collect();
    let sub_tech_map = fetch_technicians_for_interventions(pool, &sub_ids).await?;

    let items = sub_rows
        .into_iter()
        .map(|r| {
            let t = sub_tech_map.get(&r.id).cloned().unwrap_or_default();
            assemble_intervention(r, t, None)
        })
        .collect();

    Ok(items)
}

/// Crée une nouvelle intervention.
pub async fn create_intervention(
    pool: &SqlitePool,
    actor: &Actor,
    input: CreateIntervention,
    ip: Option<&str>,
) -> Result<Intervention, MaintError> {
    actor.require_permission(Permission::InterventionsCreate)?;
    if input.parent_id.is_some() {
        actor.require_permission(Permission::InterventionsEdit)?;
    }
    let site_id = resolve_site_id_for_create(actor, input.site_id.as_deref())?;
    let title = validate_title(&input.title)?;
    let description = clean_opt_string(input.description, 4096, "La description")?;
    let int_type = InterventionType::parse(&input.r#type)?;
    let priority = match input.priority {
        Some(p) => InterventionPriority::parse(&p)?,
        None => InterventionPriority::Medium,
    };
    let machine_state = match input.machine_state {
        Some(m) => MachineState::parse(&m)?,
        None => MachineState::Fonctionnel,
    };
    let equipment_id = clean_opt_id(input.equipment_id);

    // Contrainte CHECK équipement vs type (§1.3 I1)
    if int_type == InterventionType::HorsEquipement {
        if equipment_id.is_some() {
            return Err(MaintError::BadRequest(
                "Une intervention hors équipement ne doit pas spécifier d'équipement".into(),
            ));
        }
    } else if equipment_id.is_none() {
        return Err(MaintError::BadRequest(
            "L'équipement est obligatoire pour ce type d'intervention".into(),
        ));
    }

    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    ensure_site_exists(&mut tx, &site_id).await?;

    // Vérification de l'équipement sur le site
    if let Some(eq_id) = &equipment_id {
        let eq_exists: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM equipments WHERE id = ? AND site_id = ? AND deleted_at IS NULL",
        )
        .bind(eq_id)
        .bind(&site_id)
        .fetch_one(&mut *tx)
        .await?;

        if eq_exists == 0 {
            return Err(MaintError::BadRequest(
                "Équipement introuvable sur ce site".into(),
            ));
        }
    }

    let parent_id = clean_opt_id(input.parent_id);
    if let Some(p_id) = &parent_id {
        validate_parent_for_sub_intervention(&mut tx, &site_id, p_id, equipment_id.as_deref())
            .await?;
    }

    let now = store::now();
    let reported_at = crate::terrain::occurred(&mut tx, actor, None)
        .await?
        .or(normalize_date(input.reported_at, "reported_at")?)
        .unwrap_or_else(|| now.clone());
    let code =
        resolve_or_generate_code(&mut tx, &site_id, input.code.as_deref(), &reported_at).await?;
    let intervention_id = store::id();
    let reported_by_id = if actor.user_id.is_empty() {
        // En mode administrateur sans utilisateur rattaché, cherche l'admin principal
        let default_uid: Option<String> = sqlx::query_scalar(
            "SELECT id FROM users WHERE role = 'admin' AND active = 1 AND deleted_at IS NULL LIMIT 1",
        )
        .fetch_optional(&mut *tx)
        .await?;
        default_uid.ok_or_else(|| MaintError::BadRequest("Demandeur introuvable".into()))?
    } else {
        actor.user_id.clone()
    };

    let (status, solution_report, started_at, completed_at, duration_minutes) = if int_type
        == InterventionType::APosteriori
    {
        let rep = input
                .solution_report
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    MaintError::BadRequest(
                        "Le compte-rendu d'intervention est obligatoire pour une intervention a posteriori".into(),
                    )
                })?;
        let dur = input.duration_minutes.ok_or_else(|| {
            MaintError::BadRequest(
                "La durée de l'intervention est obligatoire pour une intervention a posteriori"
                    .into(),
            )
        })?;
        if dur < 0 {
            return Err(MaintError::BadRequest(
                "La durée doit être positive ou nulle".into(),
            ));
        }
        let start =
            normalize_date(input.started_at, "started_at")?.unwrap_or_else(|| reported_at.clone());
        let comp =
            normalize_date(input.completed_at, "completed_at")?.unwrap_or_else(|| now.clone());
        if comp < start {
            return Err(MaintError::BadRequest(
                "La date de fin ne peut pas être antérieure à la date de début".into(),
            ));
        }
        (
            InterventionStatus::Completed,
            Some(rep),
            Some(start),
            Some(comp),
            Some(dur),
        )
    } else {
        (InterventionStatus::Open, None, None, None, None)
    };

    sqlx::query(
        r#"
        INSERT INTO interventions (
            id, site_id, code, title, description, type, priority, status,
            machine_state, equipment_id, parent_id, reported_by_id, reported_at,
            started_at, completed_at, duration_minutes, solution_report,
            cancelled_at, cancelled_by_id, cancel_reason,
            created_at, updated_at, deleted_at
        ) VALUES (
            ?, ?, ?, ?, ?, ?, ?, ?,
            ?, ?, ?, ?, ?,
            ?, ?, ?, ?,
            NULL, NULL, NULL,
            ?, ?, NULL
        )
        "#,
    )
    .bind(&intervention_id)
    .bind(&site_id)
    .bind(&code)
    .bind(&title)
    .bind(&description)
    .bind(int_type.as_str())
    .bind(priority.as_str())
    .bind(status.as_str())
    .bind(machine_state.as_str())
    .bind(&equipment_id)
    .bind(&parent_id)
    .bind(&reported_by_id)
    .bind(&reported_at)
    .bind(&started_at)
    .bind(&completed_at)
    .bind(duration_minutes)
    .bind(&solution_report)
    .bind(&now)
    .bind(&now)
    .execute(&mut *tx)
    .await?;

    // Gestion des techniciens assignés
    let tech_ids = input.technician_ids.unwrap_or_default();
    if !tech_ids.is_empty() {
        for (idx, uid) in tech_ids.iter().enumerate() {
            validate_user_for_assignment(&mut tx, &site_id, uid).await?;
            let is_lead = if idx == 0 { 1 } else { 0 };
            let tech_dur = if int_type == InterventionType::APosteriori && is_lead == 1 {
                duration_minutes
            } else {
                None
            };
            sqlx::query(
                "INSERT INTO intervention_technicians (intervention_id, user_id, is_lead, assigned_at, duration_minutes) VALUES (?, ?, ?, ?, ?)",
            )
            .bind(&intervention_id)
            .bind(uid)
            .bind(is_lead)
            .bind(&now)
            .bind(tech_dur)
            .execute(&mut *tx)
            .await?;
        }
    } else if int_type == InterventionType::APosteriori && !actor.user_id.is_empty() {
        // Pour a posteriori sans technicien spécifié, assigne le demandeur comme lead avec la durée
        if let Ok(()) = validate_user_for_assignment(&mut tx, &site_id, &actor.user_id).await {
            sqlx::query(
                "INSERT INTO intervention_technicians (intervention_id, user_id, is_lead, assigned_at, duration_minutes) VALUES (?, ?, 1, ?, ?)",
            )
            .bind(&intervention_id)
            .bind(&actor.user_id)
            .bind(&now)
            .bind(duration_minutes)
            .execute(&mut *tx)
            .await?;
        }
    }

    // Effet sur le statut de l'équipement (§2.3)
    if let Some(eq_id) = &equipment_id {
        if status == InterventionStatus::Open {
            if machine_state == MachineState::Arret {
                update_equipment_status_direct(&mut tx, eq_id, "out_of_service").await?;
            } else if machine_state == MachineState::Degrade {
                let current_eq_status: String = sqlx::query_scalar(
                    "SELECT status FROM equipments WHERE id = ? AND deleted_at IS NULL",
                )
                .bind(eq_id)
                .fetch_one(&mut *tx)
                .await?;
                if current_eq_status != "out_of_service" {
                    update_equipment_status_direct(&mut tx, eq_id, "degraded").await?;
                }
            }
        } else if status == InterventionStatus::Completed {
            // Recalcul résiduel si clôture directe
            recalculate_equipment_status(&mut tx, eq_id, None).await?;
        }
    }

    crate::pilotage::notifications::intervention(&mut tx, actor, &intervention_id, true).await?;
    let created = fetch_single_intervention_with_conn(&mut tx, &intervention_id).await?;

    store::audit(
        &mut tx,
        actor,
        Audit {
            action: "create",
            entity: "intervention",
            entity_id: Some(&intervention_id),
            site_id: Some(&site_id),
            before: None,
            after: Some(json!(created)),
            ip,
        },
    )
    .await?;

    crate::terrain::record(&mut tx, actor, &created, 201).await?;
    tx.commit().await?;
    Ok(created)
}

/// Crée une sous-intervention rattachée à une intervention parente.
pub async fn create_sub_intervention(
    pool: &SqlitePool,
    actor: &Actor,
    parent_id: &str,
    mut input: CreateIntervention,
    ip: Option<&str>,
) -> Result<Intervention, MaintError> {
    input.parent_id = Some(parent_id.to_string());
    create_intervention(pool, actor, input, ip).await
}

/// Prise en charge d'une intervention (`open` -> `in_progress`).
pub async fn take_intervention(
    pool: &SqlitePool,
    actor: &Actor,
    id: &str,
    input: TakeIntervention,
    ip: Option<&str>,
) -> Result<Intervention, MaintError> {
    actor.require_permission(Permission::InterventionsEdit)?;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let current = scoped_intervention_raw(&mut tx, actor, id).await?;

    let cur_status = InterventionStatus::parse(&current.status)?;
    check_transition(cur_status, InterventionStatus::InProgress)?;

    let now = store::now();

    // Assignation des techniciens fournis ou de l'utilisateur courant si aucun technicien existant
    if let Some(tech_ids) = input.technician_ids {
        if !tech_ids.is_empty() {
            // Un seul référent principal : remise à 0 de is_lead pour les autres techniciens
            sqlx::query(
                "UPDATE intervention_technicians SET is_lead = 0 WHERE intervention_id = ?",
            )
            .bind(id)
            .execute(&mut *tx)
            .await?;

            for (idx, uid) in tech_ids.iter().enumerate() {
                validate_user_for_assignment(&mut tx, &current.site_id, uid).await?;
                let is_lead = if idx == 0 { 1 } else { 0 };
                sqlx::query(
                    r#"
                    INSERT INTO intervention_technicians (intervention_id, user_id, is_lead, assigned_at, duration_minutes)
                    VALUES (?, ?, ?, ?, NULL)
                    ON CONFLICT(intervention_id, user_id) DO UPDATE SET is_lead = excluded.is_lead
                    "#,
                )
                .bind(id)
                .bind(uid)
                .bind(is_lead)
                .bind(&now)
                .execute(&mut *tx)
                .await?;
            }
        }
    } else {
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM intervention_technicians WHERE intervention_id = ?",
        )
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;

        if count == 0 && !actor.user_id.is_empty() {
            validate_user_for_assignment(&mut tx, &current.site_id, &actor.user_id).await?;
            sqlx::query(
                "INSERT INTO intervention_technicians (intervention_id, user_id, is_lead, assigned_at, duration_minutes) VALUES (?, ?, 1, ?, NULL)",
            )
            .bind(id)
            .bind(&actor.user_id)
            .bind(&now)
            .execute(&mut *tx)
            .await?;
        }
    }

    crate::pilotage::notifications::intervention(&mut tx, actor, id, false).await?;
    let action_at = crate::terrain::occurred(&mut tx, actor, Some(&current.reported_at)).await?;
    let started_at = current
        .started_at
        .or(action_at)
        .unwrap_or_else(|| now.clone());

    sqlx::query(
        "UPDATE interventions SET status = 'in_progress', started_at = ?, updated_at = ? WHERE id = ?",
    )
    .bind(&started_at)
    .bind(&now)
    .bind(id)
    .execute(&mut *tx)
    .await?;

    let updated = fetch_single_intervention_with_conn(&mut tx, id).await?;

    store::audit(
        &mut tx,
        actor,
        Audit {
            action: "take",
            entity: "intervention",
            entity_id: Some(id),
            site_id: Some(&current.site_id),
            before: None,
            after: Some(json!(updated)),
            ip,
        },
    )
    .await?;

    crate::terrain::record(&mut tx, actor, &updated, 200).await?;
    tx.commit().await?;
    Ok(updated)
}

/// Clôture d'une intervention (`open` ou `in_progress` -> `completed`).
pub async fn complete_intervention(
    pool: &SqlitePool,
    actor: &Actor,
    id: &str,
    input: CompleteIntervention,
    ip: Option<&str>,
) -> Result<Intervention, MaintError> {
    actor.require_permission(Permission::InterventionsEdit)?;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let current = scoped_intervention_raw(&mut tx, actor, id).await?;
    crate::terrain::require_assignment(&mut tx, actor, id).await?;

    let cur_status = InterventionStatus::parse(&current.status)?;
    check_transition(cur_status, InterventionStatus::Completed)?;

    let solution_report = input.solution_report.trim().to_string();
    if solution_report.is_empty() {
        return Err(MaintError::BadRequest(
            "Le compte-rendu d'intervention est obligatoire".into(),
        ));
    }

    // Décision du 2026-10-18 : la clôture exige qu'au moins un technicien assigné ait un temps renseigné
    let tech_with_time: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM intervention_technicians WHERE intervention_id = ? AND duration_minutes IS NOT NULL",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;

    if tech_with_time == 0 {
        return Err(MaintError::BadRequest(
            "Au moins un technicien assigné doit avoir saisi son temps avant la clôture".into(),
        ));
    }

    // Garde-fou §2.2 : aucune sous-intervention active non terminée
    let active_subs: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM interventions WHERE parent_id = ? AND deleted_at IS NULL AND status IN ('open','in_progress')",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;

    if active_subs > 0 {
        return Err(MaintError::Conflict(
            "Impossible de clôturer une intervention ayant des sous-interventions encore actives"
                .into(),
        ));
    }

    let now = store::now();
    let completed_at = crate::terrain::occurred(
        &mut tx,
        actor,
        current.started_at.as_deref().or(Some(&current.reported_at)),
    )
    .await?
    .or(normalize_date(input.completed_at, "completed_at")?)
    .unwrap_or_else(|| now.clone());
    let started_at = current.started_at.unwrap_or_else(|| completed_at.clone());

    if completed_at < started_at {
        return Err(MaintError::BadRequest(
            "La date de fin ne peut pas être antérieure à la date de début".into(),
        ));
    }

    sqlx::query(
        "UPDATE interventions SET status = 'completed', solution_report = ?, started_at = ?, completed_at = ?, updated_at = ? WHERE id = ?",
    )
    .bind(&solution_report)
    .bind(&started_at)
    .bind(&completed_at)
    .bind(&now)
    .bind(id)
    .execute(&mut *tx)
    .await?;

    // Recalcul du statut de l'équipement (§2.3)
    if let Some(eq_id) = &current.equipment_id {
        recalculate_equipment_status(&mut tx, eq_id, input.final_equipment_status.as_deref())
            .await?;
    }

    // Libération des réservations de pièces préparées non consommées (Phase 4 §7.3)
    sqlx::query(
        "UPDATE stock_reservations SET status = 'released', closed_at = ?, updated_at = ? WHERE intervention_id = ? AND status = 'prepared'",
    )
    .bind(&now)
    .bind(&now)
    .bind(id)
    .execute(&mut *tx)
    .await?;
    // Hook phase 5 : mise à jour de l'occurrence de maintenance liée
    crate::maintenance::on_intervention_completed(&mut tx, id, &completed_at, ip).await?;

    let updated = fetch_single_intervention_with_conn(&mut tx, id).await?;

    store::audit(
        &mut tx,
        actor,
        Audit {
            action: "complete",
            entity: "intervention",
            entity_id: Some(id),
            site_id: Some(&current.site_id),
            before: None,
            after: Some(json!(updated)),
            ip,
        },
    )
    .await?;

    crate::terrain::record(&mut tx, actor, &updated, 200).await?;
    tx.commit().await?;
    Ok(updated)
}

/// Annulation d'une intervention (`open` ou `in_progress` -> `cancelled`).
pub async fn cancel_intervention(
    pool: &SqlitePool,
    actor: &Actor,
    id: &str,
    input: CancelIntervention,
    ip: Option<&str>,
) -> Result<Intervention, MaintError> {
    actor.require_permission(Permission::InterventionsEdit)?;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let current = scoped_intervention_raw(&mut tx, actor, id).await?;

    let cur_status = InterventionStatus::parse(&current.status)?;
    check_transition(cur_status, InterventionStatus::Cancelled)?;

    let reason = input.reason.trim().to_string();
    if reason.is_empty() {
        return Err(MaintError::BadRequest(
            "Le motif d'annulation est obligatoire".into(),
        ));
    }

    // Garde-fou : pas de sous-interventions actives
    let active_subs: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM interventions WHERE parent_id = ? AND deleted_at IS NULL AND status IN ('open','in_progress')",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;

    if active_subs > 0 {
        return Err(MaintError::Conflict(
            "Impossible d'annuler une intervention ayant des sous-interventions encore actives"
                .into(),
        ));
    }

    let now = store::now();

    sqlx::query(
        "UPDATE interventions SET status = 'cancelled', cancelled_at = ?, cancelled_by_id = ?, cancel_reason = ?, updated_at = ? WHERE id = ?",
    )
    .bind(&now)
    .bind(if actor.user_id.is_empty() { None } else { Some(&actor.user_id) })
    .bind(&reason)
    .bind(&now)
    .bind(id)
    .execute(&mut *tx)
    .await?;

    // Recalcul du statut de l'équipement comme une clôture (§7.1)
    if let Some(eq_id) = &current.equipment_id {
        recalculate_equipment_status(&mut tx, eq_id, None).await?;
    }

    // Libération des réservations de pièces préparées (Phase 4 §7.3)
    sqlx::query(
        "UPDATE stock_reservations SET status = 'released', closed_at = ?, updated_at = ? WHERE intervention_id = ? AND status = 'prepared'",
    )
    .bind(&now)
    .bind(&now)
    .bind(id)
    .execute(&mut *tx)
    .await?;
    // Hook phase 5 : mise à jour de l'occurrence de maintenance liée
    crate::maintenance::on_intervention_cancelled(&mut tx, id, &reason, ip).await?;

    let updated = fetch_single_intervention_with_conn(&mut tx, id).await?;

    store::audit(
        &mut tx,
        actor,
        Audit {
            action: "cancel",
            entity: "intervention",
            entity_id: Some(id),
            site_id: Some(&current.site_id),
            before: None,
            after: Some(json!(updated)),
            ip,
        },
    )
    .await?;

    tx.commit().await?;
    Ok(updated)
}

/// Mise à jour partielle d'une intervention (PATCH).
pub async fn update_intervention(
    pool: &SqlitePool,
    actor: &Actor,
    id: &str,
    input: UpdateIntervention,
    ip: Option<&str>,
) -> Result<Intervention, MaintError> {
    actor.require_permission(Permission::InterventionsEdit)?;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let current = scoped_intervention_raw(&mut tx, actor, id).await?;
    crate::terrain::require_assignment(&mut tx, actor, id).await?;
    crate::terrain::check_unmodified(&current.updated_at)?;

    let cur_status = InterventionStatus::parse(&current.status)?;
    if cur_status == InterventionStatus::Completed || cur_status == InterventionStatus::Cancelled {
        return Err(MaintError::Conflict(
            "Impossible de modifier une intervention clôturée ou annulée".into(),
        ));
    }

    let effective_title = match input.title {
        Some(t) => validate_title(&t)?,
        None => current.title.clone(),
    };

    let mut effective_description = current.description.clone();
    match input.description {
        PatchField::Absent => {}
        PatchField::Null => effective_description = None,
        PatchField::Value(v) => {
            effective_description = clean_opt_string(Some(v), 4096, "La description")?;
        }
    }

    let effective_priority = match input.priority {
        Some(p) => InterventionPriority::parse(&p)?.as_str().to_string(),
        None => current.priority.clone(),
    };

    let new_machine_state = match input.machine_state {
        Some(m) => Some(MachineState::parse(&m)?),
        None => None,
    };

    let effective_machine_state = new_machine_state
        .map(|m| m.as_str().to_string())
        .unwrap_or_else(|| current.machine_state.clone());
    let solution_report = match input.solution_report {
        PatchField::Absent => current.solution_report.clone(),
        PatchField::Null => None,
        PatchField::Value(value) => clean_opt_string(Some(value), 16384, "Le compte rendu")?,
    };

    let now = store::now();

    sqlx::query(
        "UPDATE interventions SET title = ?, description = ?, priority = ?, machine_state = ?, solution_report = ?, updated_at = ? WHERE id = ?",
    )
    .bind(&effective_title)
    .bind(&effective_description)
    .bind(&effective_priority)
    .bind(&effective_machine_state)
    .bind(&solution_report)
    .bind(&now)
    .bind(id)
    .execute(&mut *tx)
    .await?;

    // Recalcul du statut de l'équipement si l'état machine a changé
    if new_machine_state.is_some() && effective_machine_state != current.machine_state {
        if let Some(eq_id) = &current.equipment_id {
            recalculate_equipment_status(&mut tx, eq_id, None).await?;
        }
    }

    let updated = fetch_single_intervention_with_conn(&mut tx, id).await?;

    store::audit(
        &mut tx,
        actor,
        Audit {
            action: "update",
            entity: "intervention",
            entity_id: Some(id),
            site_id: Some(&current.site_id),
            before: None,
            after: Some(json!(updated)),
            ip,
        },
    )
    .await?;

    crate::terrain::record(&mut tx, actor, &updated, 200).await?;
    tx.commit().await?;
    Ok(updated)
}

/// Suppression logique d'une intervention (DELETE).
pub async fn delete_intervention(
    pool: &SqlitePool,
    actor: &Actor,
    id: &str,
    ip: Option<&str>,
) -> Result<(), MaintError> {
    actor.require_permission(Permission::InterventionsDelete)?;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let current = scoped_intervention_raw(&mut tx, actor, id).await?;

    // Garde-fou §2.2 : refus si sous-interventions non supprimées existantes
    let sub_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM interventions WHERE parent_id = ? AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;

    if sub_count > 0 {
        return Err(MaintError::Conflict(
            "Impossible de supprimer une intervention ayant des sous-interventions".into(),
        ));
    }

    let now = store::now();

    sqlx::query(
        "UPDATE interventions SET deleted_at = ?, updated_at = ? WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(&now)
    .bind(&now)
    .bind(id)
    .execute(&mut *tx)
    .await?;

    // Recalcul de l'équipement après suppression
    if let Some(eq_id) = &current.equipment_id {
        recalculate_equipment_status(&mut tx, eq_id, None).await?;
    }

    // Libération des réservations de pièces préparées (Phase 4 §7.3)
    sqlx::query(
        "UPDATE stock_reservations SET status = 'released', closed_at = ?, updated_at = ? WHERE intervention_id = ? AND status = 'prepared'",
    )
    .bind(&now)
    .bind(&now)
    .bind(id)
    .execute(&mut *tx)
    .await?;
    // Hook phase 5 : remise en échéance non traitée de l'occurrence liée
    crate::maintenance::on_intervention_deleted(&mut tx, id, ip).await?;

    store::audit(
        &mut tx,
        actor,
        Audit {
            action: "delete",
            entity: "intervention",
            entity_id: Some(id),
            site_id: Some(&current.site_id),
            before: None,
            after: None,
            ip,
        },
    )
    .await?;

    tx.commit().await?;
    Ok(())
}

/// Assigne ou met à jour un technicien sur une intervention.
pub async fn assign_technician(
    pool: &SqlitePool,
    actor: &Actor,
    intervention_id: &str,
    input: AssignTechnician,
    ip: Option<&str>,
) -> Result<AssignedTechnician, MaintError> {
    actor.require_permission(Permission::InterventionsEdit)?;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let current = scoped_intervention_raw(&mut tx, actor, intervention_id).await?;

    let cur_status = InterventionStatus::parse(&current.status)?;
    if cur_status == InterventionStatus::Completed || cur_status == InterventionStatus::Cancelled {
        return Err(MaintError::Conflict(
            "Impossible de modifier les techniciens d'une intervention clôturée ou annulée".into(),
        ));
    }

    validate_user_for_assignment(&mut tx, &current.site_id, &input.user_id).await?;

    let now = store::now();

    if input.is_lead {
        // Un seul référent principal à la fois
        sqlx::query("UPDATE intervention_technicians SET is_lead = 0 WHERE intervention_id = ?")
            .bind(intervention_id)
            .execute(&mut *tx)
            .await?;
    }

    sqlx::query(
        r#"
        INSERT INTO intervention_technicians (intervention_id, user_id, is_lead, assigned_at, duration_minutes)
        VALUES (?, ?, ?, ?, NULL)
        ON CONFLICT(intervention_id, user_id) DO UPDATE SET is_lead = excluded.is_lead
        "#,
    )
    .bind(intervention_id)
    .bind(&input.user_id)
    .bind(if input.is_lead { 1 } else { 0 })
    .bind(&now)
    .execute(&mut *tx)
    .await?;

    sqlx::query("UPDATE interventions SET updated_at = ? WHERE id = ?")
        .bind(&now)
        .bind(intervention_id)
        .execute(&mut *tx)
        .await?;

    let tech =
        fetch_assigned_technician_with_conn(&mut tx, intervention_id, &input.user_id).await?;

    store::audit(
        &mut tx,
        actor,
        Audit {
            action: "assign_technician",
            entity: "intervention",
            entity_id: Some(intervention_id),
            site_id: Some(&current.site_id),
            before: None,
            after: Some(json!(tech)),
            ip,
        },
    )
    .await?;

    crate::pilotage::notifications::intervention(&mut tx, actor, intervention_id, false).await?;
    tx.commit().await?;
    Ok(tech)
}

/// Retire l'assignation d'un technicien.
pub async fn remove_technician(
    pool: &SqlitePool,
    actor: &Actor,
    intervention_id: &str,
    user_id: &str,
    ip: Option<&str>,
) -> Result<(), MaintError> {
    actor.require_permission(Permission::InterventionsEdit)?;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let current = scoped_intervention_raw(&mut tx, actor, intervention_id).await?;

    let cur_status = InterventionStatus::parse(&current.status)?;
    if cur_status == InterventionStatus::Completed || cur_status == InterventionStatus::Cancelled {
        return Err(MaintError::Conflict(
            "Impossible de modifier les techniciens d'une intervention clôturée ou annulée".into(),
        ));
    }

    let now = store::now();
    sqlx::query("DELETE FROM intervention_technicians WHERE intervention_id = ? AND user_id = ?")
        .bind(intervention_id)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;

    sqlx::query("UPDATE interventions SET updated_at = ? WHERE id = ?")
        .bind(&now)
        .bind(intervention_id)
        .execute(&mut *tx)
        .await?;

    sync_intervention_duration(&mut tx, intervention_id).await?;

    store::audit(
        &mut tx,
        actor,
        Audit {
            action: "remove_technician",
            entity: "intervention",
            entity_id: Some(intervention_id),
            site_id: Some(&current.site_id),
            before: None,
            after: Some(json!({ "removed_user_id": user_id })),
            ip,
        },
    )
    .await?;

    tx.commit().await?;
    Ok(())
}

/// Saisie du temps passé par un technicien (§7.3).
///
/// Règle de sécurité : un technicien ne peut saisir que son propre temps (403 si autre).
/// Le responsable et l'admin peuvent saisir le temps de tout technicien assigné.
pub async fn set_technician_time(
    pool: &SqlitePool,
    actor: &Actor,
    intervention_id: &str,
    user_id: &str,
    input: SetTechnicianTime,
    ip: Option<&str>,
) -> Result<AssignedTechnician, MaintError> {
    actor.require_permission(Permission::InterventionsEdit)?;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let current = scoped_intervention_raw(&mut tx, actor, intervention_id).await?;
    crate::terrain::require_assignment(&mut tx, actor, intervention_id).await?;

    let cur_status = InterventionStatus::parse(&current.status)?;
    if cur_status == InterventionStatus::Completed || cur_status == InterventionStatus::Cancelled {
        return Err(MaintError::Conflict(
            "Impossible de modifier les temps d'une intervention clôturée ou annulée".into(),
        ));
    }

    // Un technicien ne peut pas saisir le temps d'un autre (403)
    if actor.role == Role::Technicien && actor.user_id != user_id {
        return Err(MaintError::Forbidden(
            "Un technicien ne peut saisir que son propre temps passé".into(),
        ));
    }

    if input.duration_minutes < 0 {
        return Err(MaintError::BadRequest(
            "La durée doit être positive ou nulle".into(),
        ));
    }

    let is_assigned: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM intervention_technicians WHERE intervention_id = ? AND user_id = ?",
    )
    .bind(intervention_id)
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await?;

    if is_assigned == 0 {
        return Err(MaintError::BadRequest(
            "Ce technicien n'est pas assigné à cette intervention".into(),
        ));
    }

    sqlx::query(
        "UPDATE intervention_technicians SET duration_minutes = ? WHERE intervention_id = ? AND user_id = ?",
    )
    .bind(input.duration_minutes)
    .bind(intervention_id)
    .bind(user_id)
    .execute(&mut *tx)
    .await?;

    // La durée globale de l'intervention est la somme automatique des temps des techniciens
    sync_intervention_duration(&mut tx, intervention_id).await?;

    let tech = fetch_assigned_technician_with_conn(&mut tx, intervention_id, user_id).await?;

    store::audit(
        &mut tx,
        actor,
        Audit {
            action: "set_technician_time",
            entity: "intervention",
            entity_id: Some(intervention_id),
            site_id: Some(&current.site_id),
            before: None,
            after: Some(json!(tech)),
            ip,
        },
    )
    .await?;

    crate::terrain::record(&mut tx, actor, &tech, 200).await?;
    tx.commit().await?;
    Ok(tech)
}

// ============================================================================
// FONCTIONS INTERNES ET UTILITAIRES
// ============================================================================

/// Recalcule la somme des temps des techniciens et met à jour `interventions.duration_minutes`.
async fn sync_intervention_duration(
    conn: &mut SqliteConnection,
    intervention_id: &str,
) -> Result<Option<i64>, MaintError> {
    let total: Option<i64> = sqlx::query_scalar(
        "SELECT sum(duration_minutes) FROM intervention_technicians WHERE intervention_id = ? AND duration_minutes IS NOT NULL",
    )
    .bind(intervention_id)
    .fetch_one(&mut *conn)
    .await?;

    let now = store::now();

    sqlx::query("UPDATE interventions SET duration_minutes = ?, updated_at = ? WHERE id = ?")
        .bind(total)
        .bind(&now)
        .bind(intervention_id)
        .execute(conn)
        .await?;

    Ok(total)
}

/// Met à jour directement le statut d'un équipement.
async fn update_equipment_status_direct(
    conn: &mut SqliteConnection,
    equipment_id: &str,
    new_status: &str,
) -> Result<(), MaintError> {
    let now = store::now();
    sqlx::query(
        "UPDATE equipments SET status = ?, updated_at = ? WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(new_status)
    .bind(&now)
    .bind(equipment_id)
    .execute(conn)
    .await?;
    Ok(())
}

/// Recalcule le statut opérationnel de l'équipement selon la criticité résiduelle (§2.3 et §7.1).
pub async fn recalculate_equipment_status(
    conn: &mut SqliteConnection,
    equipment_id: &str,
    manual_status_override: Option<&str>,
) -> Result<(), MaintError> {
    if let Some(manual) = manual_status_override {
        let trimmed = manual.trim();
        match trimmed {
            "operational" | "degraded" | "out_of_service" => {
                update_equipment_status_direct(conn, equipment_id, trimmed).await?;
                return Ok(());
            }
            other => {
                return Err(MaintError::BadRequest(format!(
                    "Statut d'équipement final invalide : « {other} »"
                )));
            }
        }
    }

    // Requête des interventions actives résiduelles sur cet équipement
    let states: Vec<String> = sqlx::query_scalar(
        r#"
        SELECT machine_state
        FROM interventions
        WHERE equipment_id = ?
          AND status IN ('open', 'in_progress')
          AND deleted_at IS NULL
        "#,
    )
    .bind(equipment_id)
    .fetch_all(&mut *conn)
    .await?;

    let target_status = if states.iter().any(|s| s == "ARRET") {
        "out_of_service"
    } else if states.iter().any(|s| s == "DEGRADE") {
        "degraded"
    } else {
        "operational"
    };

    update_equipment_status_direct(conn, equipment_id, target_status).await?;
    Ok(())
}

/// Vérifie qu'un utilisateur peut être assigné (même site, actif, rôle technicien/responsable/admin).
async fn validate_user_for_assignment(
    conn: &mut SqliteConnection,
    site_id: &str,
    user_id: &str,
) -> Result<(), MaintError> {
    let row: Option<(Option<String>, String, i64)> = sqlx::query_as(
        "SELECT site_id, role, active FROM users WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(user_id)
    .fetch_optional(conn)
    .await?;

    let (u_site, u_role, u_active) = row
        .ok_or_else(|| MaintError::BadRequest("Utilisateur introuvable pour assignation".into()))?;

    if u_active == 0 {
        return Err(MaintError::BadRequest(
            "Impossible d'assigner un utilisateur inactif".into(),
        ));
    }

    if u_site.as_deref() != Some(site_id) && u_role != "admin" {
        return Err(MaintError::BadRequest(
            "L'utilisateur n'appartient pas au même site que l'intervention".into(),
        ));
    }

    if u_role == "production" {
        return Err(MaintError::BadRequest(
            "Les utilisateurs de production ne peuvent pas être assignés comme techniciens".into(),
        ));
    }

    Ok(())
}

/// Valide l'éligibilité d'une intervention parente pour une sous-intervention (§2.2).
async fn validate_parent_for_sub_intervention(
    conn: &mut SqliteConnection,
    site_id: &str,
    parent_id: &str,
    sub_equipment_id: Option<&str>,
) -> Result<(), MaintError> {
    let parent_row: Option<(String, Option<String>, String, Option<String>)> = sqlx::query_as(
        "SELECT site_id, parent_id, status, equipment_id FROM interventions WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(parent_id)
    .fetch_optional(&mut *conn)
    .await?;

    let (p_site, p_parent, p_status, p_eq) = parent_row
        .ok_or_else(|| MaintError::BadRequest("Intervention parente introuvable".into()))?;

    if p_site != site_id {
        return Err(MaintError::BadRequest(
            "L'intervention parente n'appartient pas au même site".into(),
        ));
    }

    if p_parent.is_some() {
        return Err(MaintError::BadRequest(
            "Impossible de créer une sous-intervention sur une sous-intervention (profondeur maximale de 1)".into(),
        ));
    }

    if p_status == "completed" || p_status == "cancelled" {
        return Err(MaintError::Conflict(
            "Impossible de créer une sous-intervention sur une intervention clôturée ou annulée"
                .into(),
        ));
    }

    if let Some(sub_eq) = sub_equipment_id {
        if let Some(parent_eq) = &p_eq {
            if sub_eq != parent_eq {
                // Doit être un sous-équipement hiérarchique
                let is_descendant: i64 = sqlx::query_scalar(
                    r#"
                    WITH RECURSIVE hier(id, parent_id) AS (
                        SELECT id, parent_id FROM equipments WHERE id = ? AND deleted_at IS NULL
                        UNION ALL
                        SELECT e.id, e.parent_id FROM equipments e
                        JOIN hier h ON e.id = h.parent_id
                        WHERE e.deleted_at IS NULL
                    )
                    SELECT count(*) FROM hier WHERE id = ?
                    "#,
                )
                .bind(sub_eq)
                .bind(parent_eq)
                .fetch_one(conn)
                .await?;

                if is_descendant == 0 {
                    return Err(MaintError::BadRequest(
                        "L'équipement de la sous-intervention doit être identique ou enfant de l'équipement parent".into(),
                    ));
                }
            }
        }
    }

    Ok(())
}

/// Récupère une intervention brute avec contrôle d'accès multi-sites (404 si autre site).
pub(crate) async fn scoped_intervention_raw(
    conn: &mut SqliteConnection,
    actor: &Actor,
    id: &str,
) -> Result<RawInterventionRow, MaintError> {
    let row: Option<RawInterventionRow> = sqlx::query_as(
        &format!("SELECT {INTERVENTION_SELECT_FIELDS} {INTERVENTION_FROM_JOINS} WHERE i.id = ? AND i.deleted_at IS NULL AND (? OR i.site_id = ?)"),
    )
    .bind(id)
    .bind(actor.is_admin())
    .bind(&actor.site_id)
    .fetch_optional(conn)
    .await?;

    row.ok_or_else(|| MaintError::NotFound("Intervention".into()))
}

/// Récupère une intervention unique complète avec ses techniciens via une connexion ouverte.
pub(crate) async fn fetch_single_intervention_with_conn(
    conn: &mut SqliteConnection,
    id: &str,
) -> Result<Intervention, MaintError> {
    let raw: RawInterventionRow = sqlx::query_as(
        &format!("SELECT {INTERVENTION_SELECT_FIELDS} {INTERVENTION_FROM_JOINS} WHERE i.id = ? AND i.deleted_at IS NULL"),
    )
    .bind(id)
    .fetch_one(&mut *conn)
    .await?;

    let tech_rows: Vec<RawTechRow> = sqlx::query_as(
        r#"
        SELECT
            it.intervention_id,
            it.user_id,
            it.is_lead,
            it.assigned_at,
            it.duration_minutes,
            u.display_name,
            u.username
        FROM intervention_technicians it
        JOIN users u ON u.id = it.user_id
        WHERE it.intervention_id = ?
        ORDER BY it.is_lead DESC, it.assigned_at ASC
        "#,
    )
    .bind(id)
    .fetch_all(conn)
    .await?;

    let technicians = tech_rows
        .into_iter()
        .map(|t| AssignedTechnician {
            user_id: t.user_id,
            display_name: t.display_name,
            username: t.username,
            is_lead: t.is_lead != 0,
            assigned_at: t.assigned_at,
            duration_minutes: t.duration_minutes,
        })
        .collect();

    Ok(assemble_intervention(raw, technicians, None))
}

/// Récupère un technicien assigné unique.
async fn fetch_assigned_technician_with_conn(
    conn: &mut SqliteConnection,
    intervention_id: &str,
    user_id: &str,
) -> Result<AssignedTechnician, MaintError> {
    let t: RawTechRow = sqlx::query_as(
        r#"
        SELECT
            it.intervention_id,
            it.user_id,
            it.is_lead,
            it.assigned_at,
            it.duration_minutes,
            u.display_name,
            u.username
        FROM intervention_technicians it
        JOIN users u ON u.id = it.user_id
        WHERE it.intervention_id = ? AND it.user_id = ?
        "#,
    )
    .bind(intervention_id)
    .bind(user_id)
    .fetch_one(conn)
    .await?;

    Ok(AssignedTechnician {
        user_id: t.user_id,
        display_name: t.display_name,
        username: t.username,
        is_lead: t.is_lead != 0,
        assigned_at: t.assigned_at,
        duration_minutes: t.duration_minutes,
    })
}

/// Charge les techniciens groupés pour une liste d'interventions.
async fn fetch_technicians_for_interventions(
    pool: &SqlitePool,
    intervention_ids: &[String],
) -> Result<std::collections::HashMap<String, Vec<AssignedTechnician>>, MaintError> {
    use std::collections::HashMap;
    let mut map: HashMap<String, Vec<AssignedTechnician>> = HashMap::new();
    if intervention_ids.is_empty() {
        return Ok(map);
    }

    let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(
        r#"
        SELECT
            it.intervention_id,
            it.user_id,
            it.is_lead,
            it.assigned_at,
            it.duration_minutes,
            u.display_name,
            u.username
        FROM intervention_technicians it
        JOIN users u ON u.id = it.user_id
        WHERE it.intervention_id IN (
        "#,
    );

    let mut separated = qb.separated(", ");
    for id in intervention_ids {
        separated.push_bind(id);
    }
    separated.push_unseparated(") ORDER BY it.is_lead DESC, it.assigned_at ASC");

    let rows: Vec<RawTechRow> = qb.build_query_as().fetch_all(pool).await?;
    for r in rows {
        map.entry(r.intervention_id)
            .or_default()
            .push(AssignedTechnician {
                user_id: r.user_id,
                display_name: r.display_name,
                username: r.username,
                is_lead: r.is_lead != 0,
                assigned_at: r.assigned_at,
                duration_minutes: r.duration_minutes,
            });
    }

    Ok(map)
}

/// Assemble une intervention à partir de sa ligne brute et de ses relations.
fn assemble_intervention(
    r: RawInterventionRow,
    technicians: Vec<AssignedTechnician>,
    sub_interventions: Option<Vec<Intervention>>,
) -> Intervention {
    let equipment = match (r.equipment_id.clone(), r.equipment_code, r.equipment_name) {
        (Some(id), Some(code), Some(name)) => Some(NamedRef { id, code, name }),
        _ => None,
    };

    let reported_by = r.reported_by_name.map(|name| UserRef {
        id: r.reported_by_id.clone(),
        display_name: name,
    });

    let cancelled_by = match (r.cancelled_by_id.clone(), r.cancelled_by_name) {
        (Some(id), Some(display_name)) => Some(UserRef { id, display_name }),
        _ => None,
    };

    let parent = match (r.parent_id.clone(), r.parent_code, r.parent_title) {
        (Some(id), Some(code), Some(title)) => Some(ParentRef { id, code, title }),
        _ => None,
    };

    Intervention {
        id: r.id,
        site_id: r.site_id,
        code: r.code,
        title: r.title,
        description: r.description,
        r#type: r.r#type,
        priority: r.priority,
        status: r.status,
        machine_state: r.machine_state,
        equipment_id: r.equipment_id,
        parent_id: r.parent_id,
        reported_by_id: r.reported_by_id,
        reported_at: r.reported_at,
        started_at: r.started_at,
        completed_at: r.completed_at,
        duration_minutes: r.duration_minutes,
        solution_report: r.solution_report,
        cancelled_at: r.cancelled_at,
        cancelled_by_id: r.cancelled_by_id,
        cancel_reason: r.cancel_reason,
        created_at: r.created_at,
        updated_at: r.updated_at,
        equipment,
        reported_by,
        cancelled_by,
        parent,
        technicians,
        sub_interventions_count: r.sub_interventions_count,
        documents_count: r.documents_count,
        sub_interventions,
        maintenance_plan: None,
        maintenance_occurrence: None,
    }
}

/// Génère ou valide la référence unique d'intervention (format `INT-AAAA-XXXX`).
pub(crate) async fn resolve_or_generate_code(
    conn: &mut SqliteConnection,
    site_id: &str,
    provided_code: Option<&str>,
    reported_at: &str,
) -> Result<String, MaintError> {
    if let Some(c) = provided_code {
        let trimmed = c.trim();
        if trimmed.is_empty()
            || trimmed.chars().count() > 64
            || trimmed.chars().any(char::is_control)
        {
            return Err(MaintError::BadRequest(
                "Le code d'intervention doit contenir entre 1 et 64 caractères valides".into(),
            ));
        }
        let exists: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM interventions WHERE site_id = ? AND code = ? COLLATE NOCASE AND deleted_at IS NULL",
        )
        .bind(site_id)
        .bind(trimmed)
        .fetch_one(conn)
        .await?;

        if exists > 0 {
            return Err(MaintError::Conflict(
                "Une intervention avec cette référence existe déjà sur ce site".into(),
            ));
        }
        return Ok(trimmed.to_string());
    }

    // Extraction de l'année à partir de reported_at, à défaut année courante
    let year = reported_at
        .get(0..4)
        .filter(|y| y.chars().all(|c| c.is_ascii_digit()))
        .map(str::to_string)
        .unwrap_or_else(|| chrono::Utc::now().format("%Y").to_string());

    let prefix = format!("INT-{year}-");
    let pattern = format!("{prefix}%");

    // Les références des interventions supprimées comptent aussi : un numéro n'est jamais réattribué.
    let existing_codes: Vec<String> =
        sqlx::query_scalar("SELECT code FROM interventions WHERE site_id = ? AND code LIKE ?")
            .bind(site_id)
            .bind(&pattern)
            .fetch_all(&mut *conn)
            .await?;

    let mut max_seq = 0u32;
    for c in &existing_codes {
        if let Some(suffix) = c.strip_prefix(&prefix) {
            if let Ok(seq) = suffix.parse::<u32>() {
                if seq > max_seq {
                    max_seq = seq;
                }
            }
        }
    }

    let mut next_seq = max_seq + 1;
    loop {
        let candidate = format!("{prefix}{next_seq:04}");
        let collision: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM interventions WHERE site_id = ? AND code = ? COLLATE NOCASE AND deleted_at IS NULL",
        )
        .bind(site_id)
        .bind(&candidate)
        .fetch_one(&mut *conn)
        .await?;

        if collision == 0 {
            return Ok(candidate);
        }
        next_seq += 1;
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

fn validate_title(title: &str) -> Result<String, MaintError> {
    let trimmed = title.trim();
    if trimmed.is_empty() || trimmed.chars().count() > 128 || trimmed.chars().any(char::is_control)
    {
        return Err(MaintError::BadRequest(
            "Le titre d'intervention doit contenir entre 1 et 128 caractères valides".into(),
        ));
    }
    Ok(trimmed.to_string())
}

fn clean_opt_id(id: Option<String>) -> Option<String> {
    id.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn clean_opt_string(
    val: Option<String>,
    max_len: usize,
    field_name: &str,
) -> Result<Option<String>, MaintError> {
    match val {
        None => Ok(None),
        Some(s) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                Ok(None)
            } else if trimmed.chars().count() > max_len {
                Err(MaintError::BadRequest(format!(
                    "{field_name} ne peut pas dépasser {max_len} caractères"
                )))
            } else {
                Ok(Some(trimmed.to_string()))
            }
        }
    }
}

// ============================================================================
// TESTS UNITAIRES DE LA MACHINE À ÉTATS ET DES VALIDATIONS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transitions_autorisees() {
        use InterventionStatus::*;

        // Transitions directes valides
        assert!(check_transition(Open, InProgress).is_ok());
        assert!(check_transition(Open, Completed).is_ok());
        assert!(check_transition(Open, Cancelled).is_ok());
        assert!(check_transition(InProgress, Completed).is_ok());
        assert!(check_transition(InProgress, Cancelled).is_ok());
    }

    #[test]
    fn test_transitions_refusees_en_conflit_409() {
        use InterventionStatus::*;

        // Retour arrière in_progress -> open
        let err = check_transition(InProgress, Open).unwrap_err();
        assert_eq!(err.http_status(), 409);

        // Reprises depuis completed
        assert_eq!(
            check_transition(Completed, Open).unwrap_err().http_status(),
            409
        );
        assert_eq!(
            check_transition(Completed, InProgress)
                .unwrap_err()
                .http_status(),
            409
        );
        assert_eq!(
            check_transition(Completed, Cancelled)
                .unwrap_err()
                .http_status(),
            409
        );
        assert_eq!(
            check_transition(Completed, Completed)
                .unwrap_err()
                .http_status(),
            409
        );

        // Reprises depuis cancelled
        assert_eq!(
            check_transition(Cancelled, Open).unwrap_err().http_status(),
            409
        );
        assert_eq!(
            check_transition(Cancelled, InProgress)
                .unwrap_err()
                .http_status(),
            409
        );
        assert_eq!(
            check_transition(Cancelled, Completed)
                .unwrap_err()
                .http_status(),
            409
        );
        assert_eq!(
            check_transition(Cancelled, Cancelled)
                .unwrap_err()
                .http_status(),
            409
        );

        // Même état
        assert_eq!(check_transition(Open, Open).unwrap_err().http_status(), 409);
        assert_eq!(
            check_transition(InProgress, InProgress)
                .unwrap_err()
                .http_status(),
            409
        );
    }

    #[test]
    fn test_parsing_des_enums() {
        assert_eq!(
            InterventionStatus::parse("open").unwrap(),
            InterventionStatus::Open
        );
        assert_eq!(
            InterventionStatus::parse("in_progress").unwrap(),
            InterventionStatus::InProgress
        );
        assert_eq!(
            InterventionStatus::parse("completed").unwrap(),
            InterventionStatus::Completed
        );
        assert_eq!(
            InterventionStatus::parse("cancelled").unwrap(),
            InterventionStatus::Cancelled
        );
        assert!(InterventionStatus::parse("inconnu").is_err());

        assert_eq!(
            InterventionType::parse("corrective").unwrap(),
            InterventionType::Corrective
        );
        assert_eq!(
            InterventionType::parse("preventive").unwrap(),
            InterventionType::Preventive
        );
        assert_eq!(
            InterventionType::parse("a_posteriori").unwrap(),
            InterventionType::APosteriori
        );
        assert_eq!(
            InterventionType::parse("hors_equipement").unwrap(),
            InterventionType::HorsEquipement
        );
        assert!(InterventionType::parse("autre").is_err());

        assert_eq!(
            InterventionPriority::parse("low").unwrap(),
            InterventionPriority::Low
        );
        assert_eq!(
            InterventionPriority::parse("critical").unwrap(),
            InterventionPriority::Critical
        );

        assert_eq!(MachineState::parse("ARRET").unwrap(), MachineState::Arret);
        assert_eq!(
            MachineState::parse("DEGRADE").unwrap(),
            MachineState::Degrade
        );
        assert_eq!(
            MachineState::parse("FONCTIONNEL").unwrap(),
            MachineState::Fonctionnel
        );
    }

    #[test]
    fn test_like_pattern_echappement() {
        assert_eq!(like_pattern("test"), "%test%");
        assert_eq!(like_pattern("100%"), "%100\\%%");
        assert_eq!(like_pattern("a_b"), "%a\\_b%");
        assert_eq!(like_pattern("c\\d"), "%c\\\\d%");
    }
}
