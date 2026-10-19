//! Module de gestion de la maintenance préventive (Phase 5).
//!
//! Ce module implémente :
//! - Les compteurs d'équipements et leurs relevés manuels (avec détection de seuil) ;
//! - Les plannings de maintenance préventive (calendrier fixe sans dérive et compteurs) ;
//! - Les occurrences d'échéances et la génération d'interventions préventives (auto et manuelle) ;
//! - Le suivi des états (en retard, bientôt, à jour, suspendu) et la vue planning projetée.

use crate::domain::site::{resolve_site_id_for_create, resolve_site_id_for_query, SitePolicy};
use std::fmt;

use chrono::{Datelike, Duration, Local, NaiveDate};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::json;
use sqlx::{FromRow, QueryBuilder, SqliteConnection, SqlitePool};

use crate::{
    domain::pagination::Cursor,
    domain::sql::like_pattern,
    referentiel::{NamedRef, PatchField},
    store::{self, Audit},
    Actor, MaintError, Permission,
};

// ============================================================================
// ÉNUMÉRATIONS ET MACHINE À ÉTATS
// ============================================================================

/// Fréquences de planification d'un planning de maintenance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Frequency {
    Daily,
    Weekly,
    Monthly,
    Quarterly,
    Yearly,
    IntervalDays,
    Counter,
}

impl Frequency {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Daily => "daily",
            Self::Weekly => "weekly",
            Self::Monthly => "monthly",
            Self::Quarterly => "quarterly",
            Self::Yearly => "yearly",
            Self::IntervalDays => "interval_days",
            Self::Counter => "counter",
        }
    }

    pub fn parse(s: &str) -> Result<Self, MaintError> {
        match s.trim() {
            "daily" => Ok(Self::Daily),
            "weekly" => Ok(Self::Weekly),
            "monthly" => Ok(Self::Monthly),
            "quarterly" => Ok(Self::Quarterly),
            "yearly" => Ok(Self::Yearly),
            "interval_days" => Ok(Self::IntervalDays),
            "counter" => Ok(Self::Counter),
            other => Err(MaintError::BadRequest(format!(
                "Fréquence de maintenance invalide : « {other} »"
            ))),
        }
    }
}

impl fmt::Display for Frequency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for Frequency {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Frequency {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::parse(&s).map_err(serde::de::Error::custom)
    }
}

/// État calculé d'un planning de maintenance pour l'IHM.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanState {
    Overdue,
    DueSoon,
    Ok,
    Suspended,
}

impl PlanState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Overdue => "overdue",
            Self::DueSoon => "due_soon",
            Self::Ok => "ok",
            Self::Suspended => "suspended",
        }
    }
}

// ============================================================================
// CALCUL PUR DES ÉCHÉANCES (SANS I/O)
// ============================================================================

/// Nombre de jours dans un mois donné d'une année donnée.
pub fn last_day_of_month(year: i32, month: u32) -> u32 {
    let next_month_date = if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1)
    };
    next_month_date
        .and_then(|d| d.pred_opt())
        .map(|d| d.day())
        .unwrap_or(30)
}

fn validate_calendar_date(value: &str) -> Result<String, MaintError> {
    NaiveDate::parse_from_str(value.trim(), "%Y-%m-%d")
        .map(|date| date.format("%Y-%m-%d").to_string())
        .map_err(|_| MaintError::BadRequest("Date calendaire invalide (AAAA-MM-JJ attendu)".into()))
}

/// Premier pas proche de la borne, en conservant le jour d'ancrage pour les mois.
fn calendar_step_at(
    anchor: NaiveDate,
    frequency: Frequency,
    interval: Option<i64>,
    bound: NaiveDate,
) -> i32 {
    let days = (bound - anchor).num_days().max(0);
    let months =
        ((bound.year() - anchor.year()) * 12 + bound.month() as i32 - anchor.month() as i32).max(0);
    match frequency {
        Frequency::Daily => days as i32,
        Frequency::Weekly => (days / 7) as i32,
        Frequency::IntervalDays => (days / interval.unwrap_or(1).max(1)) as i32,
        Frequency::Monthly => months,
        Frequency::Quarterly => months / 3,
        Frequency::Yearly => (bound.year() - anchor.year()).max(0),
        Frequency::Counter => 0,
    }
}

/// Calcule la n-ième date ancrée d'un calendrier fixe (step >= 0 ou relatif).
///
/// Pour les mois et les années, le jour est ramené au dernier jour du mois si besoin :
/// un ancrage au 31 janvier donne le 28 ou 29 février, puis le 31 mars, sans dérive.
pub fn step_calendar_date(
    anchor: NaiveDate,
    frequency: Frequency,
    interval_days: Option<i64>,
    step: i32,
) -> NaiveDate {
    match frequency {
        Frequency::Daily => anchor + Duration::days(step as i64),
        Frequency::Weekly => anchor + Duration::weeks(step as i64),
        Frequency::IntervalDays => {
            let interval = interval_days.unwrap_or(1).max(1);
            anchor + Duration::days(step as i64 * interval)
        }
        Frequency::Monthly => {
            let y0 = anchor.year();
            let m0 = anchor.month() as i32;
            let d0 = anchor.day();

            let total_months = (m0 - 1) + step;
            let target_year = y0 + total_months.div_euclid(12);
            let target_month = (total_months.rem_euclid(12) + 1) as u32;

            let max_day = last_day_of_month(target_year, target_month);
            let target_day = d0.min(max_day);

            NaiveDate::from_ymd_opt(target_year, target_month, target_day).unwrap_or(anchor)
        }
        Frequency::Quarterly => {
            let y0 = anchor.year();
            let m0 = anchor.month() as i32;
            let d0 = anchor.day();

            let total_months = (m0 - 1) + 3 * step;
            let target_year = y0 + total_months.div_euclid(12);
            let target_month = (total_months.rem_euclid(12) + 1) as u32;

            let max_day = last_day_of_month(target_year, target_month);
            let target_day = d0.min(max_day);

            NaiveDate::from_ymd_opt(target_year, target_month, target_day).unwrap_or(anchor)
        }
        Frequency::Yearly => {
            let y0 = anchor.year();
            let m0 = anchor.month();
            let d0 = anchor.day();

            let target_year = y0 + step;
            let max_day = last_day_of_month(target_year, m0);
            let target_day = d0.min(max_day);

            NaiveDate::from_ymd_opt(target_year, m0, target_day).unwrap_or(anchor)
        }
        Frequency::Counter => anchor,
    }
}

/// Résultat d'avancement d'un planning calendaire.
#[derive(Debug, PartialEq, Eq)]
pub struct AdvanceCalendarResult {
    pub next_due_date: NaiveDate,
    pub missed_dates: Vec<NaiveDate>,
}

/// Avance un planning calendaire après la clôture ou l'annulation d'une échéance.
///
/// Règle §2.1 :
/// - `next_due_date` devient la première échéance ancrée strictement postérieure à `current_due_date`.
/// - Si cette échéance est dépassée de plus d'une période par rapport à `today`,
///   les échéances entièrement écoulées entre-temps sont retournées dans `missed_dates`.
///   `next_due_date` passe alors à la dernière échéance ancrée inférieure ou égale à aujourd'hui.
pub fn advance_calendar(
    anchor: NaiveDate,
    frequency: Frequency,
    interval_days: Option<i64>,
    current_due_date: NaiveDate,
    today: NaiveDate,
) -> AdvanceCalendarResult {
    // 1. Trouver le step tel que date(step) > current_due_date
    let mut step = 0;
    while step_calendar_date(anchor, frequency, interval_days, step) <= current_due_date {
        step += 1;
        // Garde-fou
        if step > 50000 {
            break;
        }
    }

    let first_next = step_calendar_date(anchor, frequency, interval_days, step);

    if first_next > today {
        return AdvanceCalendarResult {
            next_due_date: first_next,
            missed_dates: Vec::new(),
        };
    }

    // first_next <= today : avancer tant que date(step + 1) <= today
    let mut missed = Vec::new();
    let mut k = step;

    while step_calendar_date(anchor, frequency, interval_days, k + 1) <= today {
        missed.push(step_calendar_date(anchor, frequency, interval_days, k));
        k += 1;
        if k > 50000 {
            break;
        }
    }

    let last_past = step_calendar_date(anchor, frequency, interval_days, k);

    AdvanceCalendarResult {
        next_due_date: last_past,
        missed_dates: missed,
    }
}

/// Résultat d'avancement d'un planning de compteur.
#[derive(Debug, PartialEq, Eq)]
pub struct AdvanceCounterResult {
    pub next_due_counter: i64,
    pub missed_counters: Vec<i64>,
}

/// Nombre maximal d'échéances manquées enregistrées une à une lors d'un rattrapage de compteur.
/// Au-delà, seules les plus récentes sont conservées : un saut aberrant du compteur (saisie ou
/// relevé Supervision) ne doit ni épuiser la mémoire ni retenir le verrou d'écriture.
pub const MAX_MISSED_COUNTERS: i64 = 1000;

/// Avance un planning de compteur après la clôture ou l'annulation d'une échéance.
///
/// Règle §2.2 :
/// - `next_due_counter` augmente de `counter_interval`.
/// - Les échéances déjà franchies par `current_reading` passent en `missed`, dans la limite
///   des `MAX_MISSED_COUNTERS` plus récentes.
///
/// Le nombre d'échéances franchies est calculé par arithmétique, en `i128`, sans boucle ni
/// dépassement d'entier.
pub fn advance_counter(
    counter_interval: i64,
    current_due_counter: i64,
    current_reading: i64,
) -> AdvanceCounterResult {
    let interval = counter_interval.max(1);
    let first_next = current_due_counter.saturating_add(interval);

    if first_next > current_reading {
        return AdvanceCounterResult {
            next_due_counter: first_next,
            missed_counters: Vec::new(),
        };
    }

    let (first, step) = (i128::from(first_next), i128::from(interval));
    // Échéances franchies : first, first + step, … strictement avant la nouvelle échéance.
    let crossed = (i128::from(current_reading) - first) / step;
    // first <= next <= current_reading : la conversion vers i64 ne peut pas échouer.
    let next = i64::try_from(first + crossed * step).unwrap_or(current_reading);
    let kept = crossed.min(i128::from(MAX_MISSED_COUNTERS));
    let missed = ((crossed - kept)..crossed)
        .filter_map(|i| i64::try_from(first + i * step).ok())
        .collect();

    AdvanceCounterResult {
        next_due_counter: next,
        missed_counters: missed,
    }
}

// ============================================================================
// MODÈLE DE DONNÉES ET REPRÉSENTATIONS
// ============================================================================

/// Compteur d'un équipement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EquipmentCounter {
    pub id: String,
    pub site_id: String,
    pub equipment_id: String,
    pub name: String,
    pub unit: String,
    pub current_value: i64,
    pub last_reading_at: Option<String>,
    pub active: bool,
    pub plans_count: i64,
    /// Repère de la mesure côté Apex Supervision (phase 10).
    pub external_ref: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, FromRow)]
struct RawCounterRow {
    id: String,
    site_id: String,
    equipment_id: String,
    name: String,
    unit: String,
    current_value: i64,
    last_reading_at: Option<String>,
    active: i64,
    plans_count: i64,
    external_ref: Option<String>,
    created_at: String,
    updated_at: String,
}

impl From<RawCounterRow> for EquipmentCounter {
    fn from(r: RawCounterRow) -> Self {
        Self {
            id: r.id,
            site_id: r.site_id,
            equipment_id: r.equipment_id,
            name: r.name,
            unit: r.unit,
            current_value: r.current_value,
            last_reading_at: r.last_reading_at,
            active: r.active == 1,
            plans_count: r.plans_count,
            external_ref: r.external_ref,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

/// Relevé de compteur.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CounterReading {
    pub id: String,
    pub site_id: String,
    pub counter_id: String,
    pub value: i64,
    pub read_at: String,
    pub source: String,
    pub is_reset: bool,
    pub recorded_by_id: Option<String>,
    /// Nom de l'auteur du relevé, renseigné par la liste des relevés.
    pub recorded_by_name: Option<String>,
    pub note: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, FromRow)]
struct RawCounterReadingRow {
    id: String,
    site_id: String,
    counter_id: String,
    value: i64,
    read_at: String,
    source: String,
    is_reset: i64,
    recorded_by_id: Option<String>,
    recorded_by_name: Option<String>,
    note: Option<String>,
    created_at: String,
    updated_at: String,
}

impl From<RawCounterReadingRow> for CounterReading {
    fn from(r: RawCounterReadingRow) -> Self {
        Self {
            id: r.id,
            site_id: r.site_id,
            counter_id: r.counter_id,
            value: r.value,
            read_at: r.read_at,
            source: r.source,
            is_reset: r.is_reset == 1,
            recorded_by_id: r.recorded_by_id,
            recorded_by_name: r.recorded_by_name,
            note: r.note,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

/// Référence légère à un compteur pour la fiche planning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CounterRef {
    pub id: String,
    pub name: String,
    pub unit: String,
    pub current_value: i64,
}

/// Référence à une occurrence ouverte sur un planning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenOccurrenceRef {
    pub id: String,
    pub intervention_id: Option<String>,
    pub intervention_code: Option<String>,
}

/// Planning de maintenance préventive.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaintenancePlan {
    pub documents_count: i64,
    pub id: String,
    pub site_id: String,
    pub code: String,
    pub title: String,
    pub equipment_id: String,
    pub description: Option<String>,
    pub instructions: Option<String>,
    pub priority: String,
    pub frequency: String,
    pub interval_days: Option<i64>,
    pub counter_id: Option<String>,
    pub counter_interval: Option<i64>,
    pub anchor_date: Option<String>,
    pub next_due_date: Option<String>,
    pub anchor_counter: Option<i64>,
    pub next_due_counter: Option<i64>,
    pub lead_days: i64,
    pub lead_counter: i64,
    pub auto_generate: bool,
    pub active: bool,
    pub estimated_minutes: Option<i64>,
    pub default_technician_id: Option<String>,
    pub default_technician_name: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub equipment: NamedRef,
    pub counter: Option<CounterRef>,
    pub open_occurrence: Option<OpenOccurrenceRef>,
    pub state: PlanState,
}

/// Occurrence de maintenance traitée ou en cours.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct MaintenanceOccurrence {
    pub id: String,
    pub site_id: String,
    pub plan_id: String,
    pub due_date: Option<String>,
    pub due_counter: Option<i64>,
    pub status: String,
    pub intervention_id: Option<String>,
    pub intervention_code: Option<String>,
    pub generated_at: Option<String>,
    pub closed_at: Option<String>,
    pub generation: Option<String>,
    pub skip_reason: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Élément de la vue planning projetée.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleEntry {
    pub date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remaining: Option<i64>,
    pub plan: NamedRef,
    pub equipment: NamedRef,
    pub status: String,
    pub intervention_id: Option<String>,
}

// ============================================================================
// DTO ENTRÉES
// ============================================================================

#[derive(Debug, Clone, Deserialize)]
pub struct CreateCounter {
    pub name: String,
    pub unit: String,
    pub current_value: Option<i64>,
    pub active: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct UpdateCounter {
    pub name: Option<String>,
    pub unit: Option<String>,
    pub active: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateCounterReading {
    pub value: i64,
    pub read_at: Option<String>,
    pub is_reset: Option<bool>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateMaintenancePlan {
    pub site_id: Option<String>,
    pub code: String,
    pub title: String,
    pub equipment_id: String,
    pub description: Option<String>,
    pub instructions: Option<String>,
    pub priority: Option<String>,
    pub frequency: String,
    pub interval_days: Option<i64>,
    pub counter_id: Option<String>,
    pub counter_interval: Option<i64>,
    pub anchor_date: Option<String>,
    pub next_due_date: Option<String>,
    pub anchor_counter: Option<i64>,
    pub next_due_counter: Option<i64>,
    pub lead_days: Option<i64>,
    pub lead_counter: Option<i64>,
    pub auto_generate: Option<bool>,
    pub active: Option<bool>,
    pub estimated_minutes: Option<i64>,
    pub default_technician_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct UpdateMaintenancePlan {
    pub code: Option<String>,
    pub title: Option<String>,
    pub equipment_id: Option<String>,
    #[serde(default)]
    pub description: PatchField<String>,
    #[serde(default)]
    pub instructions: PatchField<String>,
    pub priority: Option<String>,
    pub frequency: Option<String>,
    #[serde(default)]
    pub interval_days: PatchField<i64>,
    #[serde(default)]
    pub counter_id: PatchField<String>,
    #[serde(default)]
    pub counter_interval: PatchField<i64>,
    #[serde(default)]
    pub anchor_date: PatchField<String>,
    #[serde(default)]
    pub anchor_counter: PatchField<i64>,
    pub lead_days: Option<i64>,
    pub lead_counter: Option<i64>,
    pub auto_generate: Option<bool>,
    pub active: Option<bool>,
    #[serde(default)]
    pub estimated_minutes: PatchField<i64>,
    #[serde(default)]
    pub default_technician_id: PatchField<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SkipPlanBody {
    pub reason: String,
}

#[derive(Debug, Clone, Default)]
pub struct PlanFilter {
    pub q: Option<String>,
    pub site_id: Option<String>,
    pub equipment_id: Option<String>,
    pub frequency: Option<String>,
    pub active: Option<bool>,
    pub due: Option<String>,
    pub cursor: Option<Cursor>,
    pub limit: u32,
}

// ============================================================================
// VALIDATIONS ET UTILITAIRES
// ============================================================================

fn validate_code(code: &str) -> Result<String, MaintError> {
    let trimmed = code.trim();
    if trimmed.is_empty() || trimmed.chars().count() > 64 || trimmed.chars().any(char::is_control) {
        return Err(MaintError::BadRequest(
            "Le code doit contenir de 1 à 64 caractères valides".into(),
        ));
    }
    Ok(trimmed.to_string())
}

fn validate_name(name: &str, field_name: &str, max_len: usize) -> Result<String, MaintError> {
    let trimmed = name.trim();
    if trimmed.is_empty()
        || trimmed.chars().count() > max_len
        || trimmed.chars().any(char::is_control)
    {
        return Err(MaintError::BadRequest(format!(
            "{field_name} doit contenir de 1 à {max_len} caractères valides"
        )));
    }
    Ok(trimmed.to_string())
}

// ============================================================================
// GESTION DES COMPTEURS D'ÉQUIPEMENT
// ============================================================================

/// Liste des compteurs d'un équipement.
pub async fn list_equipment_counters(
    pool: &SqlitePool,
    actor: &Actor,
    equipment_id: &str,
) -> Result<Vec<EquipmentCounter>, MaintError> {
    actor.require_permission(Permission::MaintenanceView)?;

    // Vérifier l'équipement dans le scope du site
    let site_scope = resolve_site_id_for_query(actor, None, SitePolicy::Maintenance)?;
    let mut qb = QueryBuilder::new("SELECT site_id FROM equipments WHERE id = ");
    qb.push_bind(equipment_id).push(" AND deleted_at IS NULL");
    if let Some(sid) = site_scope {
        qb.push(" AND site_id = ").push_bind(sid);
    }
    let eq_site: Option<String> = qb.build_query_scalar().fetch_optional(pool).await?;
    if eq_site.is_none() {
        return Err(MaintError::NotFound("Équipement".into()));
    }

    let rows: Vec<RawCounterRow> = sqlx::query_as(
        r#"
        SELECT
            c.id, c.site_id, c.equipment_id, c.name, c.unit, c.current_value,
            c.last_reading_at, c.active,
            (SELECT count(*) FROM maintenance_plans p WHERE p.counter_id = c.id AND p.deleted_at IS NULL) AS plans_count,
            c.external_ref, c.created_at, c.updated_at
        FROM equipment_counters c
        WHERE c.equipment_id = ? AND c.deleted_at IS NULL
        ORDER BY c.name COLLATE NOCASE ASC
        "#,
    )
    .bind(equipment_id)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(Into::into).collect())
}

/// Création d'un compteur sur un équipement.
pub async fn create_equipment_counter(
    pool: &SqlitePool,
    actor: &Actor,
    equipment_id: &str,
    input: CreateCounter,
    ip: Option<&str>,
) -> Result<EquipmentCounter, MaintError> {
    actor.require_permission(Permission::EquipmentsEdit)?;

    let name = validate_name(&input.name, "Le nom du compteur", 128)?;
    let unit = validate_name(&input.unit, "L'unité du compteur", 32)?;
    let current_val = input.current_value.unwrap_or(0);
    if current_val < 0 {
        return Err(MaintError::BadRequest(
            "La valeur initiale du compteur ne peut pas être négative".into(),
        ));
    }
    let active = if input.active.unwrap_or(true) { 1 } else { 0 };

    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;

    // Récupérer et vérifier l'équipement dans le scope
    let site_scope = resolve_site_id_for_query(actor, None, SitePolicy::Maintenance)?;
    let mut qb = QueryBuilder::new("SELECT site_id FROM equipments WHERE id = ");
    qb.push_bind(equipment_id).push(" AND deleted_at IS NULL");
    if let Some(sid) = site_scope {
        qb.push(" AND site_id = ").push_bind(sid);
    }
    let site_id: String = qb
        .build_query_scalar()
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| MaintError::NotFound("Équipement".into()))?;

    // Vérifier unicité du nom sur l'équipement
    let exists: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM equipment_counters WHERE equipment_id = ? AND name = ? COLLATE NOCASE AND deleted_at IS NULL",
    )
    .bind(equipment_id)
    .bind(&name)
    .fetch_one(&mut *tx)
    .await?;

    if exists > 0 {
        return Err(MaintError::Conflict(
            "Un compteur avec ce nom existe déjà sur cet équipement".into(),
        ));
    }

    let id = store::id();
    let now = store::now();

    sqlx::query(
        r#"
        INSERT INTO equipment_counters (id, site_id, equipment_id, name, unit, current_value, active, created_at, updated_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&id)
    .bind(&site_id)
    .bind(equipment_id)
    .bind(&name)
    .bind(&unit)
    .bind(current_val)
    .bind(active)
    .bind(&now)
    .bind(&now)
    .execute(&mut *tx)
    .await?;

    let counter = EquipmentCounter {
        id: id.clone(),
        site_id: site_id.clone(),
        equipment_id: equipment_id.to_string(),
        name,
        unit,
        current_value: current_val,
        last_reading_at: None,
        active: active == 1,
        plans_count: 0,
        external_ref: None,
        created_at: now,
        updated_at: store::now(),
    };

    store::audit(
        &mut tx,
        actor,
        Audit {
            action: "create",
            entity: "equipment_counter",
            entity_id: Some(&id),
            site_id: Some(&site_id),
            before: None,
            after: Some(json!(counter)),
            ip,
        },
    )
    .await?;

    tx.commit().await?;
    Ok(counter)
}

/// Mise à jour d'un compteur (PATCH).
pub async fn update_counter(
    pool: &SqlitePool,
    actor: &Actor,
    id: &str,
    input: UpdateCounter,
    ip: Option<&str>,
) -> Result<EquipmentCounter, MaintError> {
    actor.require_permission(Permission::EquipmentsEdit)?;

    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;

    let current: RawCounterRow = sqlx::query_as(
        r#"
        SELECT
            c.id, c.site_id, c.equipment_id, c.name, c.unit, c.current_value,
            c.last_reading_at, c.active,
            (SELECT count(*) FROM maintenance_plans p WHERE p.counter_id = c.id AND p.deleted_at IS NULL) AS plans_count,
            c.external_ref, c.created_at, c.updated_at
        FROM equipment_counters c
        WHERE c.id = ? AND c.deleted_at IS NULL
        "#,
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| MaintError::NotFound("Compteur".into()))?;

    if !actor.is_admin() && actor.site_id.as_deref() != Some(&current.site_id) {
        return Err(MaintError::NotFound("Compteur".into()));
    }

    let new_name = match input.name {
        Some(n) => validate_name(&n, "Le nom du compteur", 128)?,
        None => current.name.clone(),
    };

    if new_name != current.name {
        let exists: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM equipment_counters WHERE equipment_id = ? AND name = ? COLLATE NOCASE AND id <> ? AND deleted_at IS NULL",
        )
        .bind(&current.equipment_id)
        .bind(&new_name)
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;

        if exists > 0 {
            return Err(MaintError::Conflict(
                "Un compteur avec ce nom existe déjà sur cet équipement".into(),
            ));
        }
    }

    let new_unit = match input.unit {
        Some(u) => validate_name(&u, "L'unité du compteur", 32)?,
        None => current.unit.clone(),
    };

    let new_active = match input.active {
        Some(a) => {
            if a {
                1
            } else {
                0
            }
        }
        None => current.active,
    };

    let now = store::now();

    sqlx::query(
        "UPDATE equipment_counters SET name = ?, unit = ?, active = ?, updated_at = ? WHERE id = ?",
    )
    .bind(&new_name)
    .bind(&new_unit)
    .bind(new_active)
    .bind(&now)
    .bind(id)
    .execute(&mut *tx)
    .await?;

    let updated = EquipmentCounter {
        id: current.id.clone(),
        site_id: current.site_id.clone(),
        equipment_id: current.equipment_id.clone(),
        name: new_name,
        unit: new_unit,
        current_value: current.current_value,
        last_reading_at: current.last_reading_at,
        active: new_active == 1,
        plans_count: current.plans_count,
        external_ref: current.external_ref.clone(),
        created_at: current.created_at,
        updated_at: now,
    };

    store::audit(
        &mut tx,
        actor,
        Audit {
            action: "update",
            entity: "equipment_counter",
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

/// Suppression logique d'un compteur (DELETE).
pub async fn delete_counter(
    pool: &SqlitePool,
    actor: &Actor,
    id: &str,
    ip: Option<&str>,
) -> Result<(), MaintError> {
    actor.require_permission(Permission::EquipmentsEdit)?;

    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;

    let current: Option<(String, String)> = sqlx::query_as(
        "SELECT site_id, name FROM equipment_counters WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?;

    let (site_id, _) = current.ok_or_else(|| MaintError::NotFound("Compteur".into()))?;

    if !actor.is_admin() && actor.site_id.as_deref() != Some(&site_id) {
        return Err(MaintError::NotFound("Compteur".into()));
    }

    // Garde-fou §2.6 : Supprimer un compteur utilisé par un planning non supprimé est refusé (409)
    let plan_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM maintenance_plans WHERE counter_id = ? AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;

    if plan_count > 0 {
        return Err(MaintError::Conflict(
            "Impossible de supprimer un compteur utilisé par un planning de maintenance".into(),
        ));
    }

    let now = store::now();

    sqlx::query(
        "UPDATE equipment_counters SET deleted_at = ?, updated_at = ? WHERE id = ? AND deleted_at IS NULL",
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
            entity: "equipment_counter",
            entity_id: Some(id),
            site_id: Some(&site_id),
            before: None,
            after: None,
            ip,
        },
    )
    .await?;

    tx.commit().await?;
    Ok(())
}

// ============================================================================
// HISTORIQUE DES RELEVÉS DE COMPTEUR
// ============================================================================

/// Liste paginée des relevés d'un compteur, du plus récent au plus ancien.
pub async fn list_counter_readings(
    pool: &SqlitePool,
    actor: &Actor,
    counter_id: &str,
    cursor: Option<Cursor>,
    limit: u32,
) -> Result<crate::referentiel::Page<CounterReading>, MaintError> {
    actor.require_permission(Permission::MaintenanceView)?;
    let limit = limit.clamp(1, 100);

    let counter_site: Option<String> = sqlx::query_scalar(
        "SELECT site_id FROM equipment_counters WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(counter_id)
    .fetch_optional(pool)
    .await?;

    let site_id = counter_site.ok_or_else(|| MaintError::NotFound("Compteur".into()))?;

    if !actor.is_admin() && actor.site_id.as_deref() != Some(&site_id) {
        return Err(MaintError::NotFound("Compteur".into()));
    }

    let mut qb = QueryBuilder::new(
        r#"
        SELECT r.id, r.site_id, r.counter_id, r.value, r.read_at, r.source, r.is_reset,
            r.recorded_by_id, u.display_name AS recorded_by_name, r.note, r.created_at, r.updated_at
        FROM counter_readings r
        LEFT JOIN users u ON u.id = r.recorded_by_id
        WHERE r.counter_id = 
        "#,
    );
    qb.push_bind(counter_id).push(" AND r.deleted_at IS NULL");

    if let Some(c) = cursor {
        if let Some(ts) = c.timestamp {
            qb.push(" AND (r.read_at < ")
                .push_bind(ts.clone())
                .push(" OR (r.read_at = ")
                .push_bind(ts)
                .push(" AND r.id < ")
                .push_bind(c.id)
                .push("))");
        } else {
            qb.push(" AND r.id < ").push_bind(c.id);
        }
    }

    qb.push(" ORDER BY r.read_at DESC, r.id DESC LIMIT ")
        .push_bind((limit + 1) as i64);

    let mut rows: Vec<RawCounterReadingRow> = qb.build_query_as().fetch_all(pool).await?;

    let next_cursor = if rows.len() > limit as usize {
        rows.truncate(limit as usize);
        rows.last()
            .map(|last| Cursor::with_timestamp(&last.id, &last.read_at).encode())
    } else {
        None
    };

    let items = rows.into_iter().map(Into::into).collect();
    Ok(crate::referentiel::Page { items, next_cursor })
}

/// Saisie d'un relevé de compteur.
pub async fn record_counter_reading(
    pool: &SqlitePool,
    actor: &Actor,
    counter_id: &str,
    input: CreateCounterReading,
    ip: Option<&str>,
) -> Result<CounterReading, MaintError> {
    actor.require_permission(Permission::CountersRecord)?;

    if input.value < 0 {
        return Err(MaintError::BadRequest(
            "La valeur du compteur ne peut pas être négative".into(),
        ));
    }

    let is_reset = input.is_reset.unwrap_or(false);
    let now = store::now();
    let read_at = crate::interventions::normalize_date(input.read_at, "read_at")?
        .unwrap_or_else(|| now.clone());
    let note = input
        .note
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;

    let counter_row: Option<(String, String, i64)> = sqlx::query_as(
        "SELECT site_id, equipment_id, current_value FROM equipment_counters WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(counter_id)
    .fetch_optional(&mut *tx)
    .await?;

    let (site_id, _eq_id, old_current_value) =
        counter_row.ok_or_else(|| MaintError::NotFound("Compteur".into()))?;

    if !actor.is_admin() && actor.site_id.as_deref() != Some(&site_id) {
        return Err(MaintError::NotFound("Compteur".into()));
    }

    // Règle §1 : Un relevé ne peut pas être inférieur au précédent (400), sauf avec is_reset = 1
    if !is_reset && input.value < old_current_value {
        return Err(MaintError::BadRequest(
            "Un relevé de compteur ne peut pas être inférieur au relevé précédent sans remise à zéro".into(),
        ));
    }

    // Règle §2.3 : Remise à zéro d'un compteur
    if is_reset {
        // next_due_counter = nouvelle valeur + (ancienne échéance - ancienne valeur)
        let linked_plans: Vec<(String, i64, i64)> = sqlx::query_as(
            "SELECT id, next_due_counter, counter_interval FROM maintenance_plans WHERE counter_id = ? AND deleted_at IS NULL",
        )
        .bind(counter_id)
        .fetch_all(&mut *tx)
        .await?;

        for (p_id, old_due, interval) in linked_plans {
            let new_due = i64::try_from(
                i128::from(input.value) + i128::from(old_due) - i128::from(old_current_value),
            )
            .map_err(|_| MaintError::BadRequest("Échéance hors limites".into()))?;
            let int_val = interval.max(1);
            let new_anchor = if new_due >= int_val {
                new_due % int_val
            } else {
                new_due
            };

            sqlx::query(
                "UPDATE maintenance_plans SET next_due_counter = ?, anchor_counter = ?, counter_cycle = counter_cycle + 1, updated_at = ? WHERE id = ?",
            )
            .bind(new_due)
            .bind(new_anchor)
            .bind(&now)
            .bind(&p_id)
            .execute(&mut *tx)
            .await?;

            store::audit(
                &mut tx,
                actor,
                Audit {
                    action: "reset_adjust",
                    entity: "maintenance_plan",
                    entity_id: Some(&p_id),
                    site_id: Some(&site_id),
                    before: Some(json!({ "next_due_counter": old_due })),
                    after: Some(
                        json!({ "next_due_counter": new_due, "anchor_counter": new_anchor }),
                    ),
                    ip,
                },
            )
            .await?;
        }
    }

    // Mise à jour de la valeur courante du compteur
    sqlx::query(
        "UPDATE equipment_counters SET current_value = ?, last_reading_at = ?, updated_at = ? WHERE id = ?",
    )
    .bind(input.value)
    .bind(&read_at)
    .bind(&now)
    .bind(counter_id)
    .execute(&mut *tx)
    .await?;

    let reading_id = store::id();
    let recorded_by_id = if actor.user_id.is_empty() {
        None
    } else {
        Some(actor.user_id.clone())
    };

    sqlx::query(
        r#"
        INSERT INTO counter_readings (id, site_id, counter_id, value, read_at, source, is_reset, recorded_by_id, note, created_at, updated_at)
        VALUES (?, ?, ?, ?, ?, 'manual', ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&reading_id)
    .bind(&site_id)
    .bind(counter_id)
    .bind(input.value)
    .bind(&read_at)
    .bind(if is_reset { 1 } else { 0 })
    .bind(&recorded_by_id)
    .bind(&note)
    .bind(&now)
    .bind(&now)
    .execute(&mut *tx)
    .await?;

    let reading = CounterReading {
        id: reading_id.clone(),
        site_id: site_id.clone(),
        counter_id: counter_id.to_string(),
        value: input.value,
        read_at,
        source: "manual".into(),
        is_reset,
        recorded_by_id,
        recorded_by_name: None,
        note,
        created_at: now,
        updated_at: store::now(),
    };

    store::audit(
        &mut tx,
        actor,
        Audit {
            action: "record",
            entity: "counter_reading",
            entity_id: Some(&reading_id),
            site_id: Some(&site_id),
            before: None,
            after: Some(json!(reading)),
            ip,
        },
    )
    .await?;

    tx.commit().await?;

    // Règle §2.4 : La génération s'exécute aussi juste après chaque relevé de compteur
    process_auto_generation_for_counter(pool, counter_id).await;

    Ok(reading)
}

// ============================================================================
// GESTION DES PLANNINGS DE MAINTENANCE PRÉVENTIVE
// ============================================================================

#[derive(Debug, FromRow)]
pub struct RawPlanRow {
    pub documents_count: i64,
    pub id: String,
    pub site_id: String,
    pub code: String,
    pub title: String,
    pub equipment_id: String,
    pub description: Option<String>,
    pub instructions: Option<String>,
    pub priority: String,
    pub frequency: String,
    pub interval_days: Option<i64>,
    pub counter_id: Option<String>,
    pub counter_interval: Option<i64>,
    pub anchor_date: Option<String>,
    pub next_due_date: Option<String>,
    pub anchor_counter: Option<i64>,
    pub next_due_counter: Option<i64>,
    pub lead_days: i64,
    pub lead_counter: i64,
    pub auto_generate: i64,
    pub active: i64,
    pub estimated_minutes: Option<i64>,
    pub default_technician_id: Option<String>,
    pub default_technician_name: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub equipment_code: String,
    pub equipment_name: String,
    pub counter_name: Option<String>,
    pub counter_unit: Option<String>,
    pub counter_current_value: Option<i64>,
    pub counter_active: Option<i64>,
    pub open_occ_id: Option<String>,
    pub open_occ_int_id: Option<String>,
    pub open_occ_int_code: Option<String>,
}

fn compute_plan_state(r: &RawPlanRow, today: NaiveDate) -> PlanState {
    let plan_active = r.active == 1;
    let is_counter = r.frequency == "counter";
    let counter_active = r.counter_active.unwrap_or(1) == 1;

    // Règle §2.2 : Désactiver un compteur suspend la génération des plannings liés
    if !plan_active || (is_counter && !counter_active) {
        return PlanState::Suspended;
    }

    if is_counter {
        let cur = r.counter_current_value.unwrap_or(0);
        let due = r.next_due_counter.unwrap_or(0);
        let lead = r.lead_counter;

        if cur >= due {
            PlanState::Overdue
        } else if cur >= due - lead {
            PlanState::DueSoon
        } else {
            PlanState::Ok
        }
    } else if let Some(due_str) = &r.next_due_date {
        if let Ok(due_date) = NaiveDate::parse_from_str(due_str, "%Y-%m-%d") {
            if due_date <= today {
                PlanState::Overdue
            } else if due_date <= today + Duration::days(30) {
                PlanState::DueSoon
            } else {
                PlanState::Ok
            }
        } else {
            PlanState::Ok
        }
    } else {
        PlanState::Ok
    }
}

fn raw_plan_to_plan(r: RawPlanRow, today: NaiveDate) -> MaintenancePlan {
    let state = compute_plan_state(&r, today);
    let counter = match (
        r.counter_id.clone(),
        r.counter_name,
        r.counter_unit,
        r.counter_current_value,
    ) {
        (Some(cid), Some(cname), Some(cunit), Some(cval)) => Some(CounterRef {
            id: cid,
            name: cname,
            unit: cunit,
            current_value: cval,
        }),
        _ => None,
    };

    let open_occurrence = match r.open_occ_id {
        Some(occ_id) => Some(OpenOccurrenceRef {
            id: occ_id,
            intervention_id: r.open_occ_int_id,
            intervention_code: r.open_occ_int_code,
        }),
        None => None,
    };

    MaintenancePlan {
        documents_count: r.documents_count,
        id: r.id,
        site_id: r.site_id,
        code: r.code,
        title: r.title,
        equipment_id: r.equipment_id,
        description: r.description,
        instructions: r.instructions,
        priority: r.priority,
        frequency: r.frequency,
        interval_days: r.interval_days,
        counter_id: r.counter_id,
        counter_interval: r.counter_interval,
        anchor_date: r.anchor_date,
        next_due_date: r.next_due_date,
        anchor_counter: r.anchor_counter,
        next_due_counter: r.next_due_counter,
        lead_days: r.lead_days,
        lead_counter: r.lead_counter,
        auto_generate: r.auto_generate == 1,
        active: r.active == 1,
        estimated_minutes: r.estimated_minutes,
        default_technician_id: r.default_technician_id,
        default_technician_name: r.default_technician_name,
        created_at: r.created_at,
        updated_at: r.updated_at,
        equipment: NamedRef {
            id: r.equipment_code.clone(), // will be swapped below
            code: r.equipment_code,
            name: r.equipment_name,
        },
        counter,
        open_occurrence,
        state,
    }
}

const PLAN_SELECT_FIELDS: &str = r#"
    p.id, p.site_id, p.code, p.title, p.equipment_id, p.description, p.instructions,
    p.priority, p.frequency, p.interval_days, p.counter_id, p.counter_interval,
    p.anchor_date, p.next_due_date, p.anchor_counter, p.next_due_counter,
    p.lead_days, p.lead_counter, p.auto_generate, p.active, p.estimated_minutes,
    p.default_technician_id, dt.display_name AS default_technician_name, p.created_at, p.updated_at,
    (SELECT count(*) FROM documents d WHERE d.site_id=p.site_id AND d.entity_type='maintenance_plan' AND d.entity_id=p.id AND d.deleted_at IS NULL) AS documents_count,
    eq.code AS equipment_code, eq.name AS equipment_name,
    c.name AS counter_name, c.unit AS counter_unit, c.current_value AS counter_current_value, c.active AS counter_active,
    occ.id AS open_occ_id, occ.intervention_id AS open_occ_int_id, intv.code AS open_occ_int_code
"#;

const PLAN_FROM_JOINS: &str = r#"
    FROM maintenance_plans p
    INNER JOIN equipments eq ON eq.id = p.equipment_id
    LEFT JOIN equipment_counters c ON c.id = p.counter_id
    LEFT JOIN maintenance_occurrences occ ON occ.plan_id = p.id AND occ.status = 'generated' AND occ.deleted_at IS NULL
    LEFT JOIN interventions intv ON intv.id = occ.intervention_id
    LEFT JOIN users dt ON dt.id = p.default_technician_id
"#;

/// Liste paginée et filtrable des plannings de maintenance.
pub async fn list_plans(
    pool: &SqlitePool,
    actor: &Actor,
    filter: PlanFilter,
) -> Result<crate::referentiel::Page<MaintenancePlan>, MaintError> {
    actor.require_permission(Permission::MaintenanceView)?;
    let limit = filter.limit.clamp(1, 100);
    let site_scope =
        resolve_site_id_for_query(actor, filter.site_id.as_deref(), SitePolicy::Maintenance)?;

    let mut qb = QueryBuilder::new(format!(
        "SELECT {PLAN_SELECT_FIELDS} {PLAN_FROM_JOINS} WHERE p.deleted_at IS NULL"
    ));

    if let Some(sid) = site_scope {
        qb.push(" AND p.site_id = ").push_bind(sid);
    }

    if let Some(eq_id) = filter.equipment_id {
        qb.push(" AND p.equipment_id = ").push_bind(eq_id);
    }

    if let Some(freq) = filter.frequency {
        qb.push(" AND p.frequency = ").push_bind(freq);
    }

    if let Some(active) = filter.active {
        qb.push(" AND p.active = ")
            .push_bind(if active { 1 } else { 0 });
    }

    if let Some(q) = filter.q {
        let trimmed = q.trim();
        if !trimmed.is_empty() {
            let pat = like_pattern(trimmed);
            qb.push(" AND (p.code LIKE ")
                .push_bind(pat.clone())
                .push(" ESCAPE '\\' OR p.title LIKE ")
                .push_bind(pat)
                .push(" ESCAPE '\\')");
        }
    }

    if let Some(c) = filter.cursor {
        if let Some(ts) = c.timestamp {
            qb.push(" AND (p.created_at < ")
                .push_bind(ts.clone())
                .push(" OR (p.created_at = ")
                .push_bind(ts)
                .push(" AND p.id < ")
                .push_bind(c.id)
                .push("))");
        } else {
            qb.push(" AND p.id < ").push_bind(c.id);
        }
    }

    qb.push(" ORDER BY p.created_at DESC, p.id DESC");
    if !matches!(filter.due.as_deref(), Some("overdue" | "soon")) {
        qb.push(" LIMIT ").push_bind(i64::from(limit) + 1);
    }

    let rows: Vec<RawPlanRow> = qb.build_query_as().fetch_all(pool).await?;
    let today = Local::now().date_naive();

    // Filtre applicatif sur `due` si spécifié
    let filtered_rows: Vec<RawPlanRow> = if let Some(due) = filter.due.as_deref() {
        match due {
            "overdue" => rows
                .into_iter()
                .filter(|r| compute_plan_state(r, today) == PlanState::Overdue)
                .collect(),
            "soon" => rows
                .into_iter()
                .filter(|r| compute_plan_state(r, today) == PlanState::DueSoon)
                .collect(),
            _ => rows,
        }
    } else {
        rows
    };

    let has_more = filtered_rows.len() > limit as usize;
    let mut page_rows = filtered_rows;
    if has_more {
        page_rows.truncate(limit as usize);
    }

    let next_cursor = if has_more {
        page_rows
            .last()
            .map(|last| Cursor::with_timestamp(&last.id, &last.created_at).encode())
    } else {
        None
    };

    let items = page_rows
        .into_iter()
        .map(|r| {
            let eq_id = r.equipment_id.clone();
            let mut plan = raw_plan_to_plan(r, today);
            plan.equipment.id = eq_id;
            plan
        })
        .collect();

    Ok(crate::referentiel::Page { items, next_cursor })
}

/// Récupère un planning par son identifiant.
pub async fn get_plan(
    pool: &SqlitePool,
    actor: &Actor,
    id: &str,
) -> Result<MaintenancePlan, MaintError> {
    actor.require_permission(Permission::MaintenanceView)?;

    let query = format!(
        "SELECT {PLAN_SELECT_FIELDS} {PLAN_FROM_JOINS} WHERE p.id = ? AND p.deleted_at IS NULL"
    );
    let row: Option<RawPlanRow> = sqlx::query_as(&query).bind(id).fetch_optional(pool).await?;

    let r = row.ok_or_else(|| MaintError::NotFound("Planning de maintenance".into()))?;

    if !actor.is_admin() && actor.site_id.as_deref() != Some(&r.site_id) {
        return Err(MaintError::NotFound("Planning de maintenance".into()));
    }

    let today = Local::now().date_naive();
    let eq_id = r.equipment_id.clone();
    let mut plan = raw_plan_to_plan(r, today);
    plan.equipment.id = eq_id;
    Ok(plan)
}

/// Création d'un planning de maintenance.
pub async fn create_plan(
    pool: &SqlitePool,
    actor: &Actor,
    input: CreateMaintenancePlan,
    ip: Option<&str>,
) -> Result<MaintenancePlan, MaintError> {
    actor.require_permission(Permission::MaintenanceManage)?;

    // Administrateur sans site précisé (vue « Tous les sites ») : site de l'équipement.
    let requested_site = match input.site_id.as_deref().map(str::trim) {
        Some(s) if !s.is_empty() => Some(s.to_string()),
        _ if actor.is_admin() => {
            sqlx::query_scalar("SELECT site_id FROM equipments WHERE id = ? AND deleted_at IS NULL")
                .bind(&input.equipment_id)
                .fetch_optional(pool)
                .await?
        }
        _ => None,
    };
    let site_id =
        resolve_site_id_for_create(actor, requested_site.as_deref(), SitePolicy::Maintenance)?;
    let code = validate_code(&input.code)?;
    let title = validate_name(&input.title, "Le titre du planning", 255)?;
    let frequency = Frequency::parse(&input.frequency)?;

    let priority = match input.priority.as_deref() {
        Some(p) => match p.trim() {
            "low" | "medium" | "high" | "critical" => p.trim().to_string(),
            other => {
                return Err(MaintError::BadRequest(format!(
                    "Priorité invalide : « {other} »"
                )))
            }
        },
        None => "medium".to_string(),
    };

    let lead_days = input.lead_days.unwrap_or(7).clamp(0, 365);
    let lead_counter = input.lead_counter.unwrap_or(0).max(0);
    let auto_generate = if input.auto_generate.unwrap_or(true) {
        1
    } else {
        0
    };
    let active = if input.active.unwrap_or(true) { 1 } else { 0 };

    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;

    // Vérifier l'équipement sur le site
    let eq_exists: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM equipments WHERE id = ? AND site_id = ? AND deleted_at IS NULL",
    )
    .bind(&input.equipment_id)
    .bind(&site_id)
    .fetch_one(&mut *tx)
    .await?;

    if eq_exists == 0 {
        return Err(MaintError::NotFound("Équipement".into()));
    }

    // Vérifier l'unicité du code sur le site
    let code_exists: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM maintenance_plans WHERE site_id = ? AND code = ? COLLATE NOCASE AND deleted_at IS NULL",
    )
    .bind(&site_id)
    .bind(&code)
    .fetch_one(&mut *tx)
    .await?;

    if code_exists > 0 {
        return Err(MaintError::Conflict(
            "Un planning avec ce code existe déjà sur ce site".into(),
        ));
    }

    // Vérifications de cohérence selon la fréquence (§1)
    let (
        interval_days,
        counter_id,
        counter_interval,
        anchor_date,
        next_due_date,
        anchor_counter,
        next_due_counter,
    ) = match frequency {
        Frequency::IntervalDays => {
            let days = input.interval_days.ok_or_else(|| {
                MaintError::BadRequest(
                    "L'intervalle en jours est obligatoire pour la fréquence interval_days".into(),
                )
            })?;
            if !(1..=3650).contains(&days) {
                return Err(MaintError::BadRequest(
                    "L'intervalle en jours doit être compris entre 1 et 3650".into(),
                ));
            }
            let ad = input.anchor_date.ok_or_else(|| {
                MaintError::BadRequest(
                    "La date d'ancrage est obligatoire pour les plannings calendaires".into(),
                )
            })?;
            let ad = validate_calendar_date(&ad)?;
            let next_due =
                validate_calendar_date(&input.next_due_date.unwrap_or_else(|| ad.clone()))?;
            (Some(days), None, None, Some(ad), Some(next_due), None, None)
        }
        Frequency::Daily
        | Frequency::Weekly
        | Frequency::Monthly
        | Frequency::Quarterly
        | Frequency::Yearly => {
            let ad = input.anchor_date.ok_or_else(|| {
                MaintError::BadRequest(
                    "La date d'ancrage est obligatoire pour les plannings calendaires".into(),
                )
            })?;
            let ad = validate_calendar_date(&ad)?;
            let next_due =
                validate_calendar_date(&input.next_due_date.unwrap_or_else(|| ad.clone()))?;
            (None, None, None, Some(ad), Some(next_due), None, None)
        }
        Frequency::Counter => {
            let cid = input.counter_id.ok_or_else(|| {
                MaintError::BadRequest(
                    "Le compteur est obligatoire pour la fréquence counter".into(),
                )
            })?;
            let c_interval = input.counter_interval.ok_or_else(|| {
                MaintError::BadRequest(
                    "L'intervalle de compteur est obligatoire pour la fréquence counter".into(),
                )
            })?;
            if c_interval <= 0 {
                return Err(MaintError::BadRequest(
                    "L'intervalle de compteur doit être strictement positif".into(),
                ));
            }

            // Règle §2.2 : Un planning de compteur ne peut être créé que pour un compteur actif de l'équipement
            let counter_info: Option<(String, String, i64, i64)> = sqlx::query_as(
                    "SELECT site_id, equipment_id, current_value, active FROM equipment_counters WHERE id = ? AND deleted_at IS NULL",
                )
                .bind(&cid)
                .fetch_optional(&mut *tx)
                .await?;

            let (c_site, c_eq, c_val, c_active) = counter_info
                .ok_or_else(|| MaintError::BadRequest("Compteur introuvable".into()))?;

            if c_site != site_id || c_eq != input.equipment_id {
                return Err(MaintError::BadRequest(
                    "Le compteur doit appartenir au même équipement et au même site".into(),
                ));
            }

            if c_active != 1 {
                return Err(MaintError::BadRequest(
                    "Le compteur doit être actif pour créer un planning".into(),
                ));
            }

            let anchor_c = input.anchor_counter.unwrap_or(c_val);
            let next_c = match input.next_due_counter {
                Some(value) => value,
                None => anchor_c
                    .checked_add(c_interval)
                    .ok_or_else(|| MaintError::BadRequest("Échéance hors limites".into()))?,
            };

            (
                None,
                Some(cid),
                Some(c_interval),
                None,
                None,
                Some(anchor_c),
                Some(next_c),
            )
        }
    };

    // Vérifier technicien par défaut si fourni
    if let Some(tid) = &input.default_technician_id {
        let tech_ok: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM users WHERE id = ? AND deleted_at IS NULL AND active = 1 AND (role = 'admin' OR site_id = ?)",
        )
        .bind(tid)
        .bind(&site_id)
        .fetch_one(&mut *tx)
        .await?;

        if tech_ok == 0 {
            return Err(MaintError::BadRequest(
                "Le technicien par défaut est introuvable ou inactif sur ce site".into(),
            ));
        }
    }

    let plan_id = store::id();
    let now = store::now();

    sqlx::query(
        r#"
        INSERT INTO maintenance_plans (
            id, site_id, code, title, equipment_id, description, instructions,
            priority, frequency, interval_days, counter_id, counter_interval,
            anchor_date, next_due_date, anchor_counter, next_due_counter,
            lead_days, lead_counter, auto_generate, active, estimated_minutes,
            default_technician_id, created_at, updated_at
        ) VALUES (
            ?, ?, ?, ?, ?, ?, ?,
            ?, ?, ?, ?, ?,
            ?, ?, ?, ?,
            ?, ?, ?, ?, ?,
            ?, ?, ?
        )
        "#,
    )
    .bind(&plan_id)
    .bind(&site_id)
    .bind(&code)
    .bind(&title)
    .bind(&input.equipment_id)
    .bind(&input.description)
    .bind(&input.instructions)
    .bind(&priority)
    .bind(frequency.as_str())
    .bind(interval_days)
    .bind(&counter_id)
    .bind(counter_interval)
    .bind(&anchor_date)
    .bind(&next_due_date)
    .bind(anchor_counter)
    .bind(next_due_counter)
    .bind(lead_days)
    .bind(lead_counter)
    .bind(auto_generate)
    .bind(active)
    .bind(input.estimated_minutes)
    .bind(&input.default_technician_id)
    .bind(&now)
    .bind(&now)
    .execute(&mut *tx)
    .await?;

    let plan_query = format!("SELECT {PLAN_SELECT_FIELDS} {PLAN_FROM_JOINS} WHERE p.id = ?");
    let raw: RawPlanRow = sqlx::query_as(&plan_query)
        .bind(&plan_id)
        .fetch_one(&mut *tx)
        .await?;

    let today = Local::now().date_naive();
    let eq_id = raw.equipment_id.clone();
    let mut plan = raw_plan_to_plan(raw, today);
    plan.equipment.id = eq_id;

    store::audit(
        &mut tx,
        actor,
        Audit {
            action: "create",
            entity: "maintenance_plan",
            entity_id: Some(&plan_id),
            site_id: Some(&site_id),
            before: None,
            after: Some(json!(plan)),
            ip,
        },
    )
    .await?;

    tx.commit().await?;
    Ok(plan)
}

/// Mise à jour partielle d'un planning de maintenance (PATCH).
pub async fn update_plan(
    pool: &SqlitePool,
    actor: &Actor,
    id: &str,
    input: UpdateMaintenancePlan,
    ip: Option<&str>,
) -> Result<MaintenancePlan, MaintError> {
    actor.require_permission(Permission::MaintenanceManage)?;

    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;

    let plan_query = format!(
        "SELECT {PLAN_SELECT_FIELDS} {PLAN_FROM_JOINS} WHERE p.id = ? AND p.deleted_at IS NULL"
    );
    let current: Option<RawPlanRow> = sqlx::query_as(&plan_query)
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;

    let current = current.ok_or_else(|| MaintError::NotFound("Planning de maintenance".into()))?;

    if !actor.is_admin() && actor.site_id.as_deref() != Some(&current.site_id) {
        return Err(MaintError::NotFound("Planning de maintenance".into()));
    }

    if input
        .equipment_id
        .as_ref()
        .is_some_and(|equipment| equipment != &current.equipment_id)
    {
        return Err(MaintError::BadRequest(
            "Le rattachement du planning à son équipement est immuable".into(),
        ));
    }

    let schedule_params_changed = input.frequency.is_some()
        || !input.interval_days.is_absent()
        || !input.counter_id.is_absent()
        || !input.counter_interval.is_absent()
        || !input.anchor_date.is_absent()
        || !input.anchor_counter.is_absent();

    // Règle §2.6 : Modifier la fréquence, l'ancrage ou l'intervalle est refusé (409) tant qu'une occurrence est ouverte
    if schedule_params_changed && current.open_occ_id.is_some() {
        return Err(MaintError::Conflict(
            "Impossible de modifier la planification tant qu'une occurrence est ouverte".into(),
        ));
    }

    let code = match input.code {
        Some(c) => {
            let valid = validate_code(&c)?;
            if valid != current.code {
                let exists: i64 = sqlx::query_scalar(
                    "SELECT count(*) FROM maintenance_plans WHERE site_id = ? AND code = ? COLLATE NOCASE AND id <> ? AND deleted_at IS NULL",
                )
                .bind(&current.site_id)
                .bind(&valid)
                .bind(id)
                .fetch_one(&mut *tx)
                .await?;
                if exists > 0 {
                    return Err(MaintError::Conflict(
                        "Un planning avec ce code existe déjà sur ce site".into(),
                    ));
                }
            }
            valid
        }
        None => current.code.clone(),
    };

    let title = match input.title {
        Some(t) => validate_name(&t, "Le titre du planning", 255)?,
        None => current.title.clone(),
    };

    let mut description = current.description.clone();
    match input.description {
        PatchField::Absent => {}
        PatchField::Null => description = None,
        PatchField::Value(v) => description = Some(v.trim().to_string()),
    }

    let mut instructions = current.instructions.clone();
    match input.instructions {
        PatchField::Absent => {}
        PatchField::Null => instructions = None,
        PatchField::Value(v) => instructions = Some(v.trim().to_string()),
    }

    let priority = match input.priority {
        Some(p) => match p.trim() {
            "low" | "medium" | "high" | "critical" => p.trim().to_string(),
            other => {
                return Err(MaintError::BadRequest(format!(
                    "Priorité invalide : « {other} »"
                )))
            }
        },
        None => current.priority.clone(),
    };

    let frequency = match input.frequency {
        Some(f) => Frequency::parse(&f)?,
        None => Frequency::parse(&current.frequency)?,
    };

    let mut interval_days = current.interval_days;
    match input.interval_days {
        PatchField::Absent => {}
        PatchField::Null => interval_days = None,
        PatchField::Value(v) => {
            if !(1..=3650).contains(&v) {
                return Err(MaintError::BadRequest(
                    "L'intervalle en jours doit être compris entre 1 et 3650".into(),
                ));
            }
            interval_days = Some(v);
        }
    }

    let mut counter_id = current.counter_id.clone();
    match input.counter_id {
        PatchField::Absent => {}
        PatchField::Null => counter_id = None,
        PatchField::Value(v) => {
            let cid = v.trim().to_string();
            let c_info: Option<(String, String, i64, i64)> = sqlx::query_as(
                "SELECT site_id, equipment_id, current_value, active FROM equipment_counters WHERE id = ? AND deleted_at IS NULL",
            )
            .bind(&cid)
            .fetch_optional(&mut *tx)
            .await?;
            let (c_site, c_eq, _, c_active) =
                c_info.ok_or_else(|| MaintError::BadRequest("Compteur introuvable".into()))?;
            if c_site != current.site_id || c_eq != current.equipment_id {
                return Err(MaintError::BadRequest(
                    "Le compteur doit appartenir au même équipement et au même site".into(),
                ));
            }
            if c_active != 1 {
                return Err(MaintError::BadRequest("Le compteur doit être actif".into()));
            }
            counter_id = Some(cid);
        }
    }

    let mut counter_interval = current.counter_interval;
    match input.counter_interval {
        PatchField::Absent => {}
        PatchField::Null => counter_interval = None,
        PatchField::Value(v) => {
            if v <= 0 {
                return Err(MaintError::BadRequest(
                    "L'intervalle de compteur doit être strictement positif".into(),
                ));
            }
            counter_interval = Some(v);
        }
    }

    let mut anchor_date = current.anchor_date.clone();
    match input.anchor_date {
        PatchField::Absent => {}
        PatchField::Null => anchor_date = None,
        PatchField::Value(v) => anchor_date = Some(validate_calendar_date(&v)?),
    }

    let mut anchor_counter = current.anchor_counter;
    match input.anchor_counter {
        PatchField::Absent => {}
        PatchField::Null => anchor_counter = None,
        PatchField::Value(v) => anchor_counter = Some(v),
    }

    // Recalcul de next_due si planification modifiée
    let (next_due_date, next_due_counter) =
        if schedule_params_changed {
            match frequency {
                Frequency::Counter => {
                    let interval = counter_interval.ok_or_else(|| {
                        MaintError::BadRequest("L'intervalle de compteur est requis".into())
                    })?;
                    let anc = anchor_counter.unwrap_or(current.counter_current_value.unwrap_or(0));
                    (
                        None,
                        Some(anc.checked_add(interval).ok_or_else(|| {
                            MaintError::BadRequest("Échéance hors limites".into())
                        })?),
                    )
                }
                Frequency::Daily
                | Frequency::Weekly
                | Frequency::Monthly
                | Frequency::Quarterly
                | Frequency::Yearly
                | Frequency::IntervalDays => {
                    let ad = anchor_date.clone().ok_or_else(|| {
                        MaintError::BadRequest("La date d'ancrage est requise".into())
                    })?;
                    (Some(ad), None)
                }
            }
        } else {
            (current.next_due_date.clone(), current.next_due_counter)
        };

    let lead_days = input.lead_days.unwrap_or(current.lead_days).clamp(0, 365);
    let lead_counter = input.lead_counter.unwrap_or(current.lead_counter).max(0);
    let auto_generate = match input.auto_generate {
        Some(a) => {
            if a {
                1
            } else {
                0
            }
        }
        None => current.auto_generate,
    };
    let active = match input.active {
        Some(a) => {
            if a {
                1
            } else {
                0
            }
        }
        None => current.active,
    };

    let mut estimated_minutes = current.estimated_minutes;
    match input.estimated_minutes {
        PatchField::Absent => {}
        PatchField::Null => estimated_minutes = None,
        PatchField::Value(v) => estimated_minutes = Some(v.max(0)),
    }

    let mut default_technician_id = current.default_technician_id.clone();
    match input.default_technician_id {
        PatchField::Absent => {}
        PatchField::Null => default_technician_id = None,
        PatchField::Value(v) => {
            let tid = v.trim().to_string();
            let tech_ok: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM users WHERE id = ? AND deleted_at IS NULL AND active = 1 AND (role = 'admin' OR site_id = ?)",
            )
            .bind(&tid)
            .bind(&current.site_id)
            .fetch_one(&mut *tx)
            .await?;
            if tech_ok == 0 {
                return Err(MaintError::BadRequest(
                    "Le technicien est introuvable ou inactif sur ce site".into(),
                ));
            }
            default_technician_id = Some(tid);
        }
    }

    let now = store::now();

    sqlx::query(
        r#"
        UPDATE maintenance_plans SET
            code = ?, title = ?, description = ?, instructions = ?, priority = ?,
            frequency = ?, interval_days = ?, counter_id = ?, counter_interval = ?,
            anchor_date = ?, next_due_date = ?, anchor_counter = ?, next_due_counter = ?,
            lead_days = ?, lead_counter = ?, auto_generate = ?, active = ?,
            estimated_minutes = ?, default_technician_id = ?, updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(&code)
    .bind(&title)
    .bind(&description)
    .bind(&instructions)
    .bind(&priority)
    .bind(frequency.as_str())
    .bind(interval_days)
    .bind(&counter_id)
    .bind(counter_interval)
    .bind(&anchor_date)
    .bind(&next_due_date)
    .bind(anchor_counter)
    .bind(next_due_counter)
    .bind(lead_days)
    .bind(lead_counter)
    .bind(auto_generate)
    .bind(active)
    .bind(estimated_minutes)
    .bind(&default_technician_id)
    .bind(&now)
    .bind(id)
    .execute(&mut *tx)
    .await?;

    let updated_raw: RawPlanRow = sqlx::query_as(&plan_query)
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;

    let today = Local::now().date_naive();
    let eq_id = updated_raw.equipment_id.clone();
    let mut plan = raw_plan_to_plan(updated_raw, today);
    plan.equipment.id = eq_id;

    store::audit(
        &mut tx,
        actor,
        Audit {
            action: "update",
            entity: "maintenance_plan",
            entity_id: Some(id),
            site_id: Some(&current.site_id),
            before: None,
            after: Some(json!(plan)),
            ip,
        },
    )
    .await?;

    tx.commit().await?;
    Ok(plan)
}

/// Suppression logique d'un planning de maintenance (DELETE).
pub async fn delete_plan(
    pool: &SqlitePool,
    actor: &Actor,
    id: &str,
    ip: Option<&str>,
) -> Result<(), MaintError> {
    actor.require_permission(Permission::MaintenanceManage)?;

    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;

    let current: Option<(String, String)> = sqlx::query_as(
        "SELECT site_id, code FROM maintenance_plans WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?;

    let (site_id, _) =
        current.ok_or_else(|| MaintError::NotFound("Planning de maintenance".into()))?;

    if !actor.is_admin() && actor.site_id.as_deref() != Some(&site_id) {
        return Err(MaintError::NotFound("Planning de maintenance".into()));
    }

    let now = store::now();

    sqlx::query(
        "UPDATE maintenance_plans SET deleted_at = ?, updated_at = ? WHERE id = ? AND deleted_at IS NULL",
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
            entity: "maintenance_plan",
            entity_id: Some(id),
            site_id: Some(&site_id),
            before: None,
            after: None,
            ip,
        },
    )
    .await?;

    tx.commit().await?;
    Ok(())
}

// ============================================================================
// GÉNÉRATION D'INTERVENTIONS (MANUELLE ET AUTOMATIQUE)
// ============================================================================

/// Génération manuelle d'une intervention pour un planning.
pub async fn generate_plan_occurrence(
    pool: &SqlitePool,
    actor: &Actor,
    id: &str,
    ip: Option<&str>,
) -> Result<(MaintenanceOccurrence, crate::interventions::Intervention), MaintError> {
    actor.require_permission(Permission::MaintenanceManage)?;

    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;

    let plan_query = format!(
        "SELECT {PLAN_SELECT_FIELDS} {PLAN_FROM_JOINS} WHERE p.id = ? AND p.deleted_at IS NULL"
    );
    let current: Option<RawPlanRow> = sqlx::query_as(&plan_query)
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;

    let plan = current.ok_or_else(|| MaintError::NotFound("Planning de maintenance".into()))?;

    if !actor.is_admin() && actor.site_id.as_deref() != Some(&plan.site_id) {
        return Err(MaintError::NotFound("Planning de maintenance".into()));
    }

    // Règle §2.4 : Elle répond 409 si une occurrence est déjà ouverte
    if plan.open_occ_id.is_some() {
        return Err(MaintError::Conflict(
            "Une occurrence est déjà en cours pour ce planning".into(),
        ));
    }

    let (occ, intv) = do_generate_in_tx(&mut tx, &plan, actor, "manual", ip).await?;

    tx.commit().await?;
    Ok((occ, intv))
}

/// Exécute la génération dans une transaction `BEGIN IMMEDIATE` déjà ouverte.
async fn do_generate_in_tx(
    tx: &mut SqliteConnection,
    plan: &RawPlanRow,
    actor: &Actor,
    generation_mode: &str,
    ip: Option<&str>,
) -> Result<(MaintenanceOccurrence, crate::interventions::Intervention), MaintError> {
    let fresh;
    let plan = if generation_mode == "auto" {
        let query = format!("SELECT {PLAN_SELECT_FIELDS} {PLAN_FROM_JOINS} WHERE p.id = ? AND p.site_id = ? AND p.deleted_at IS NULL AND p.active = 1 AND p.auto_generate = 1");
        fresh = sqlx::query_as::<_, RawPlanRow>(&query)
            .bind(&plan.id)
            .bind(&plan.site_id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| {
                MaintError::Conflict("Le planning automatique n'est plus actif".into())
            })?;
        let due = if fresh.frequency == "counter" {
            fresh.counter_active == Some(1)
                && fresh.counter_current_value.unwrap_or(0)
                    >= fresh
                        .next_due_counter
                        .unwrap_or(0)
                        .saturating_sub(fresh.lead_counter)
        } else {
            fresh
                .next_due_date
                .as_deref()
                .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
                .is_some_and(|d| d - Duration::days(fresh.lead_days) <= Local::now().date_naive())
        };
        if fresh.open_occ_id.is_some() || !due {
            return Err(MaintError::Conflict(
                "Le planning automatique n'est plus à générer".into(),
            ));
        }
        &fresh
    } else {
        plan
    };

    let now = store::now();

    // 1. Récupération ou génération de la référence d'intervention
    let int_code =
        crate::interventions::resolve_or_generate_code(&mut *tx, &plan.site_id, None, &now).await?;

    // Demandeur de l'intervention
    let reported_by_id = if actor.user_id.is_empty() {
        let admin_id: Option<String> = sqlx::query_scalar(
            "SELECT id FROM users WHERE role = 'admin' AND active = 1 AND deleted_at IS NULL LIMIT 1",
        )
        .fetch_optional(&mut *tx)
        .await?;
        admin_id.ok_or_else(|| MaintError::BadRequest("Demandeur introuvable".into()))?
    } else {
        actor.user_id.clone()
    };

    // Description = instructions + échéance (§2.4)
    let due_info = if plan.frequency == "counter" {
        format!(
            "Échéance compteur : {} {}",
            plan.next_due_counter.unwrap_or(0),
            plan.counter_unit.as_deref().unwrap_or("")
        )
    } else {
        format!("Échéance : {}", plan.next_due_date.as_deref().unwrap_or(""))
    };

    let description = match &plan.instructions {
        Some(instr) if !instr.trim().is_empty() => format!("{}\n\n{due_info}", instr.trim()),
        _ => due_info,
    };

    let intervention_id = store::id();
    let occurrence_id = store::id();

    // 2. Création de l'intervention (sans l'occurrence_id au départ pour respecter les contraintes de clés étrangères)
    sqlx::query(
        r#"
        INSERT INTO interventions (
            id, site_id, code, title, description, type, priority, status,
            machine_state, equipment_id, reported_by_id, reported_at,
            maintenance_occurrence_id, created_at, updated_at
        ) VALUES (
            ?, ?, ?, ?, ?, 'preventive', ?, 'open',
            'FONCTIONNEL', ?, ?, ?,
            ?, ?, ?
        )
        "#,
    )
    .bind(&intervention_id)
    .bind(&plan.site_id)
    .bind(&int_code)
    .bind(&plan.title)
    .bind(&description)
    .bind(&plan.priority)
    .bind(&plan.equipment_id)
    .bind(&reported_by_id)
    .bind(&now)
    .bind(None::<String>)
    .bind(&now)
    .bind(&now)
    .execute(&mut *tx)
    .await?;

    // Technicien par défaut si présent et actif
    if let Some(tid) = &plan.default_technician_id {
        let tech_valid: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM users WHERE id = ? AND deleted_at IS NULL AND active = 1 AND (role = 'admin' OR site_id = ?)",
        )
        .bind(tid)
        .bind(&plan.site_id)
        .fetch_one(&mut *tx)
        .await?;

        if tech_valid > 0 {
            sqlx::query(
                "INSERT INTO intervention_technicians (intervention_id, user_id, is_lead, assigned_at) VALUES (?, ?, 1, ?)",
            )
            .bind(&intervention_id)
            .bind(tid)
            .bind(&now)
            .execute(&mut *tx)
            .await?;
        }
    }

    crate::pilotage::notifications::generated(&mut *tx, actor, &plan.id, &intervention_id).await?;
    crate::pilotage::notifications::intervention(&mut *tx, actor, &intervention_id, true).await?;
    // 3. Création de l'occurrence
    sqlx::query(
        r#"
        INSERT INTO maintenance_occurrences (
            id, site_id, plan_id, due_date, due_counter, status,
            intervention_id, generated_at, generation, created_at, updated_at
        ) VALUES (
            ?, ?, ?, ?, ?, 'generated',
            ?, ?, ?, ?, ?
        )
        "#,
    )
    .bind(&occurrence_id)
    .bind(&plan.site_id)
    .bind(&plan.id)
    .bind(&plan.next_due_date)
    .bind(plan.next_due_counter)
    .bind(&intervention_id)
    .bind(&now)
    .bind(generation_mode)
    .bind(&now)
    .bind(&now)
    .execute(&mut *tx)
    .await?;

    // 4. Lier l'intervention à l'occurrence créée
    sqlx::query("UPDATE interventions SET maintenance_occurrence_id = ? WHERE id = ?")
        .bind(&occurrence_id)
        .bind(&intervention_id)
        .execute(&mut *tx)
        .await?;

    let occurrence = MaintenanceOccurrence {
        id: occurrence_id.clone(),
        site_id: plan.site_id.clone(),
        plan_id: plan.id.clone(),
        due_date: plan.next_due_date.clone(),
        due_counter: plan.next_due_counter,
        status: "generated".into(),
        intervention_id: Some(intervention_id.clone()),
        intervention_code: Some(int_code.clone()),
        generated_at: Some(now.clone()),
        closed_at: None,
        generation: Some(generation_mode.to_string()),
        skip_reason: None,
        created_at: now.clone(),
        updated_at: now,
    };

    // 4. Audit
    if generation_mode == "auto" {
        let auto_actor = Actor::new("", Some(plan.site_id.clone()), crate::Role::Admin, []);
        store::audit(
            &mut *tx,
            &auto_actor,
            Audit {
                action: "generate",
                entity: "maintenance_occurrence",
                entity_id: Some(&occurrence_id),
                site_id: Some(&plan.site_id),
                before: None,
                after: Some(json!({
                    "details": { "source": "auto" },
                    "occurrence_id": occurrence_id,
                    "intervention_id": intervention_id
                })),
                ip: None,
            },
        )
        .await?;
    } else {
        store::audit(
            &mut *tx,
            actor,
            Audit {
                action: "generate",
                entity: "maintenance_occurrence",
                entity_id: Some(&occurrence_id),
                site_id: Some(&plan.site_id),
                before: None,
                after: Some(json!({
                    "details": { "source": "manual" },
                    "occurrence_id": occurrence_id,
                    "intervention_id": intervention_id
                })),
                ip,
            },
        )
        .await?;
    }

    let intervention =
        crate::interventions::fetch_single_intervention_with_conn(&mut *tx, &intervention_id)
            .await?;

    Ok((occurrence, intervention))
}

/// Report d'une échéance sans intervention (SKIP).
pub async fn skip_plan_occurrence(
    pool: &SqlitePool,
    actor: &Actor,
    id: &str,
    reason: &str,
    ip: Option<&str>,
) -> Result<MaintenancePlan, MaintError> {
    actor.require_permission(Permission::MaintenanceManage)?;

    let trimmed_reason = reason.trim().to_string();
    if trimmed_reason.is_empty() {
        return Err(MaintError::BadRequest(
            "Le motif de report est obligatoire".into(),
        ));
    }

    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;

    let plan_query = format!(
        "SELECT {PLAN_SELECT_FIELDS} {PLAN_FROM_JOINS} WHERE p.id = ? AND p.deleted_at IS NULL"
    );
    let current: Option<RawPlanRow> = sqlx::query_as(&plan_query)
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;

    let plan = current.ok_or_else(|| MaintError::NotFound("Planning de maintenance".into()))?;

    if !actor.is_admin() && actor.site_id.as_deref() != Some(&plan.site_id) {
        return Err(MaintError::NotFound("Planning de maintenance".into()));
    }

    // Règle §2.5 : Refusé (409) si une occurrence est ouverte
    if plan.open_occ_id.is_some() {
        return Err(MaintError::Conflict(
            "Impossible de reporter l'échéance tant qu'une occurrence est ouverte".into(),
        ));
    }

    let occ_id = store::id();
    let now = store::now();

    // Création de l'occurrence skipped
    sqlx::query(
        r#"
        INSERT INTO maintenance_occurrences (
            id, site_id, plan_id, due_date, due_counter, status,
            closed_at, skip_reason, created_at, updated_at
        ) VALUES (
            ?, ?, ?, ?, ?, 'skipped',
            ?, ?, ?, ?
        )
        "#,
    )
    .bind(&occ_id)
    .bind(&plan.site_id)
    .bind(id)
    .bind(&plan.next_due_date)
    .bind(plan.next_due_counter)
    .bind(&now)
    .bind(&trimmed_reason)
    .bind(&now)
    .bind(&now)
    .execute(&mut *tx)
    .await?;

    // Avancement de l'échéance
    advance_plan_schedule(&mut tx, &plan, ip).await?;

    let updated_raw: RawPlanRow = sqlx::query_as(&plan_query)
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;

    let today = Local::now().date_naive();
    let eq_id = updated_raw.equipment_id.clone();
    let mut updated_plan = raw_plan_to_plan(updated_raw, today);
    updated_plan.equipment.id = eq_id;

    store::audit(
        &mut tx,
        actor,
        Audit {
            action: "skip",
            entity: "maintenance_plan",
            entity_id: Some(id),
            site_id: Some(&plan.site_id),
            before: None,
            after: Some(json!({
                "occurrence_id": occ_id,
                "reason": trimmed_reason
            })),
            ip,
        },
    )
    .await?;

    tx.commit().await?;
    Ok(updated_plan)
}

/// Liste paginée de l'historique des occurrences d'un planning.
pub async fn list_plan_occurrences(
    pool: &SqlitePool,
    actor: &Actor,
    plan_id: &str,
    cursor: Option<Cursor>,
    limit: u32,
) -> Result<crate::referentiel::Page<MaintenanceOccurrence>, MaintError> {
    actor.require_permission(Permission::MaintenanceView)?;
    let limit = limit.clamp(1, 100);

    let plan_site: Option<String> = sqlx::query_scalar(
        "SELECT site_id FROM maintenance_plans WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(plan_id)
    .fetch_optional(pool)
    .await?;

    let site_id =
        plan_site.ok_or_else(|| MaintError::NotFound("Planning de maintenance".into()))?;

    if !actor.is_admin() && actor.site_id.as_deref() != Some(&site_id) {
        return Err(MaintError::NotFound("Planning de maintenance".into()));
    }

    let mut qb = QueryBuilder::new(
        r#"
        SELECT
            o.id, o.site_id, o.plan_id, o.due_date, o.due_counter, o.status,
            o.intervention_id, intv.code AS intervention_code,
            o.generated_at, o.closed_at, o.generation, o.skip_reason,
            o.created_at, o.updated_at
        FROM maintenance_occurrences o
        LEFT JOIN interventions intv ON intv.id = o.intervention_id
        WHERE o.plan_id = 
        "#,
    );
    qb.push_bind(plan_id).push(" AND o.deleted_at IS NULL");

    if let Some(c) = cursor {
        if let Some(ts) = c.timestamp {
            qb.push(" AND (o.created_at < ")
                .push_bind(ts.clone())
                .push(" OR (o.created_at = ")
                .push_bind(ts)
                .push(" AND o.id < ")
                .push_bind(c.id)
                .push("))");
        } else {
            qb.push(" AND o.id < ").push_bind(c.id);
        }
    }

    qb.push(" ORDER BY o.created_at DESC, o.id DESC LIMIT ")
        .push_bind((limit + 1) as i64);

    let mut rows: Vec<MaintenanceOccurrence> = qb.build_query_as().fetch_all(pool).await?;

    let next_cursor = if rows.len() > limit as usize {
        rows.truncate(limit as usize);
        rows.last()
            .map(|last| Cursor::with_timestamp(&last.id, &last.created_at).encode())
    } else {
        None
    };

    let items = rows;

    Ok(crate::referentiel::Page { items, next_cursor })
}

// ============================================================================
// HOOKS DE CLÔTURE, D'ANNULATION ET DE SUPPRESSION
// ============================================================================

/// Hook appelé par `complete_intervention` dans la même transaction.
pub async fn on_intervention_completed(
    conn: &mut SqliteConnection,
    intervention_id: &str,
    completed_at: &str,
    ip: Option<&str>,
) -> Result<(), MaintError> {
    let occ: Option<(String, String)> = sqlx::query_as(
        "SELECT id, plan_id FROM maintenance_occurrences WHERE intervention_id = ? AND status = 'generated' AND deleted_at IS NULL",
    )
    .bind(intervention_id)
    .fetch_optional(&mut *conn)
    .await?;

    if let Some((occ_id, plan_id)) = occ {
        let now = store::now();
        sqlx::query(
            "UPDATE maintenance_occurrences SET status = 'done', closed_at = ?, updated_at = ? WHERE id = ?",
        )
        .bind(completed_at)
        .bind(&now)
        .bind(&occ_id)
        .execute(&mut *conn)
        .await?;

        // Lecture du planning pour avancement
        let plan_query = format!("SELECT {PLAN_SELECT_FIELDS} {PLAN_FROM_JOINS} WHERE p.id = ?");
        let plan: RawPlanRow = sqlx::query_as(&plan_query)
            .bind(&plan_id)
            .fetch_one(&mut *conn)
            .await?;

        advance_plan_schedule(conn, &plan, ip).await?;
    }

    Ok(())
}

/// Hook appelé par `cancel_intervention` dans la même transaction.
pub async fn on_intervention_cancelled(
    conn: &mut SqliteConnection,
    intervention_id: &str,
    reason: &str,
    ip: Option<&str>,
) -> Result<(), MaintError> {
    let occ: Option<(String, String)> = sqlx::query_as(
        "SELECT id, plan_id FROM maintenance_occurrences WHERE intervention_id = ? AND status = 'generated' AND deleted_at IS NULL",
    )
    .bind(intervention_id)
    .fetch_optional(&mut *conn)
    .await?;

    if let Some((occ_id, plan_id)) = occ {
        let now = store::now();
        sqlx::query(
            "UPDATE maintenance_occurrences SET status = 'skipped', closed_at = ?, skip_reason = ?, updated_at = ? WHERE id = ?",
        )
        .bind(&now)
        .bind(reason)
        .bind(&now)
        .bind(&occ_id)
        .execute(&mut *conn)
        .await?;

        let plan_query = format!("SELECT {PLAN_SELECT_FIELDS} {PLAN_FROM_JOINS} WHERE p.id = ?");
        let plan: RawPlanRow = sqlx::query_as(&plan_query)
            .bind(&plan_id)
            .fetch_one(&mut *conn)
            .await?;

        advance_plan_schedule(conn, &plan, ip).await?;
    }

    Ok(())
}

/// Hook appelé par `delete_intervention` dans la même transaction.
///
/// Règle §2.5 : La suppression logique d'une intervention générée remet l'occurrence
/// en échéance non traitée : l'occurrence est supprimée et le planning garde son échéance.
pub async fn on_intervention_deleted(
    conn: &mut SqliteConnection,
    intervention_id: &str,
    _ip: Option<&str>,
) -> Result<(), MaintError> {
    let now = store::now();
    sqlx::query(
        "UPDATE maintenance_occurrences SET deleted_at = ?, updated_at = ? WHERE intervention_id = ? AND status = 'generated' AND deleted_at IS NULL",
    )
    .bind(&now)
    .bind(&now)
    .bind(intervention_id)
    .execute(conn)
    .await?;

    Ok(())
}

/// Avance l'échéance d'un planning et enregistre les échéances manquées si nécessaire.
async fn advance_plan_schedule(
    conn: &mut SqliteConnection,
    plan: &RawPlanRow,
    _ip: Option<&str>,
) -> Result<(), MaintError> {
    let now = store::now();
    let today = Local::now().date_naive();
    let freq = Frequency::parse(&plan.frequency)?;

    if freq == Frequency::Counter {
        let cur_due = plan.next_due_counter.unwrap_or(0);
        let interval = plan.counter_interval.unwrap_or(1);
        let cur_reading = plan.counter_current_value.unwrap_or(0);

        let res = advance_counter(interval, cur_due, cur_reading);

        sqlx::query(
            "UPDATE maintenance_plans SET next_due_counter = ?, updated_at = ? WHERE id = ?",
        )
        .bind(res.next_due_counter)
        .bind(&now)
        .bind(&plan.id)
        .execute(&mut *conn)
        .await?;

        // Enregistrer les échéances missed
        for m in res.missed_counters {
            let m_id = store::id();
            sqlx::query(
                r#"
                INSERT INTO maintenance_occurrences (id, site_id, plan_id, due_counter, status, created_at, updated_at)
                VALUES (?, ?, ?, ?, 'missed', ?, ?)
                "#,
            )
            .bind(&m_id)
            .bind(&plan.site_id)
            .bind(&plan.id)
            .bind(m)
            .bind(&now)
            .bind(&now)
            .execute(&mut *conn)
            .await?;
        }
    } else {
        let anchor_str = plan.anchor_date.as_deref().unwrap_or("");
        let anchor = NaiveDate::parse_from_str(anchor_str, "%Y-%m-%d").unwrap_or(today);
        let cur_due_str = plan.next_due_date.as_deref().unwrap_or(anchor_str);
        let cur_due = NaiveDate::parse_from_str(cur_due_str, "%Y-%m-%d").unwrap_or(today);

        let res = advance_calendar(anchor, freq, plan.interval_days, cur_due, today);
        let next_due_str = res.next_due_date.format("%Y-%m-%d").to_string();

        sqlx::query("UPDATE maintenance_plans SET next_due_date = ?, updated_at = ? WHERE id = ?")
            .bind(&next_due_str)
            .bind(&now)
            .bind(&plan.id)
            .execute(&mut *conn)
            .await?;

        // Enregistrer les échéances missed
        for m in res.missed_dates {
            let m_id = store::id();
            let m_str = m.format("%Y-%m-%d").to_string();
            sqlx::query(
                r#"
                INSERT INTO maintenance_occurrences (id, site_id, plan_id, due_date, status, created_at, updated_at)
                VALUES (?, ?, ?, ?, 'missed', ?, ?)
                "#,
            )
            .bind(&m_id)
            .bind(&plan.site_id)
            .bind(&plan.id)
            .bind(&m_str)
            .bind(&now)
            .bind(&now)
            .execute(&mut *conn)
            .await?;
        }
    }

    Ok(())
}

// ============================================================================
// VUE PLANNING PROJETÉE (SCHEDULE)
// ============================================================================

/// Vue planning non paginée sur une période maximale de 366 jours.
pub async fn get_schedule(
    pool: &SqlitePool,
    actor: &Actor,
    from_str: &str,
    to_str: &str,
    site_id_filter: Option<&str>,
    equipment_id_filter: Option<&str>,
) -> Result<Vec<ScheduleEntry>, MaintError> {
    actor.require_permission(Permission::MaintenanceView)?;

    let from = NaiveDate::parse_from_str(from_str.trim(), "%Y-%m-%d").map_err(|_| {
        MaintError::BadRequest("Format de date invalide pour « from » (AAAA-MM-JJ attendu)".into())
    })?;
    let to = NaiveDate::parse_from_str(to_str.trim(), "%Y-%m-%d").map_err(|_| {
        MaintError::BadRequest("Format de date invalide pour « to » (AAAA-MM-JJ attendu)".into())
    })?;

    if to < from {
        return Err(MaintError::BadRequest(
            "La date de fin ne peut pas être antérieure à la date de début".into(),
        ));
    }

    let diff = (to - from).num_days();
    if diff > 366 {
        return Err(MaintError::BadRequest(
            "La période demandée ne peut pas dépasser 366 jours".into(),
        ));
    }

    let site_scope = resolve_site_id_for_query(actor, site_id_filter, SitePolicy::Maintenance)?;
    let today = Local::now().date_naive();

    // 1. Récupération de tous les plannings actifs
    let mut qb = QueryBuilder::new(format!(
        "SELECT {PLAN_SELECT_FIELDS} {PLAN_FROM_JOINS} WHERE p.deleted_at IS NULL AND p.active = 1"
    ));

    if let Some(sid) = site_scope {
        qb.push(" AND p.site_id = ").push_bind(sid);
    }

    if let Some(eq_id) = equipment_id_filter {
        qb.push(" AND p.equipment_id = ").push_bind(eq_id);
    }

    let plans: Vec<RawPlanRow> = qb.build_query_as().fetch_all(pool).await?;

    let mut calendar_entries: Vec<ScheduleEntry> = Vec::new();
    let mut counter_entries: Vec<ScheduleEntry> = Vec::new();

    for p in &plans {
        let plan_ref = NamedRef {
            id: p.id.clone(),
            code: p.code.clone(),
            name: p.title.clone(),
        };
        let eq_ref = NamedRef {
            id: p.equipment_id.clone(),
            code: p.equipment_code.clone(),
            name: p.equipment_name.clone(),
        };

        let freq = match Frequency::parse(&p.frequency) {
            Ok(f) => f,
            Err(_) => continue,
        };

        if freq == Frequency::Counter {
            let cur = p.counter_current_value.unwrap_or(0);
            let due = p.next_due_counter.unwrap_or(0);
            let remaining = due - cur;

            let status = if p.open_occ_id.is_some() {
                "generated"
            } else if cur >= due {
                "overdue"
            } else {
                "projected"
            };

            counter_entries.push(ScheduleEntry {
                date: None,
                remaining: Some(remaining),
                plan: plan_ref,
                equipment: eq_ref,
                status: status.to_string(),
                intervention_id: p.open_occ_int_id.clone(),
            });
        } else {
            // Plannings calendaires :
            // 1. Occurrences réelles passées/présentes dans [from, to]
            #[derive(Debug, FromRow)]
            struct OccRow {
                due_date: String,
                status: String,
                intervention_id: Option<String>,
            }

            let real_occs: Vec<OccRow> = sqlx::query_as(
                r#"
                SELECT due_date, status, intervention_id
                FROM maintenance_occurrences
                WHERE plan_id = ? AND due_date IS NOT NULL AND due_date BETWEEN ? AND ? AND deleted_at IS NULL
                "#,
            )
            .bind(&p.id)
            .bind(from_str)
            .bind(to_str)
            .fetch_all(pool)
            .await?;

            let mut processed_dates = std::collections::HashSet::new();

            for ro in real_occs {
                let d_str = ro.due_date;
                processed_dates.insert(d_str.clone());
                let st = if ro.status == "generated" {
                    if let Ok(d) = NaiveDate::parse_from_str(&d_str, "%Y-%m-%d") {
                        if d < today {
                            "overdue"
                        } else {
                            "generated"
                        }
                    } else {
                        "generated"
                    }
                } else {
                    ro.status.as_str()
                };

                calendar_entries.push(ScheduleEntry {
                    date: Some(d_str),
                    remaining: None,
                    plan: plan_ref.clone(),
                    equipment: eq_ref.clone(),
                    status: st.to_string(),
                    intervention_id: ro.intervention_id,
                });
            }

            // 2. Next due date si dans [from, to] et pas déjà dans les occurrences
            if let Some(next_due_str) = &p.next_due_date {
                if let Ok(next_due) = NaiveDate::parse_from_str(next_due_str, "%Y-%m-%d") {
                    if (from..=to).contains(&next_due) && !processed_dates.contains(next_due_str) {
                        processed_dates.insert(next_due_str.clone());
                        let st = if next_due < today {
                            "overdue"
                        } else {
                            "projected"
                        };
                        calendar_entries.push(ScheduleEntry {
                            date: Some(next_due_str.clone()),
                            remaining: None,
                            plan: plan_ref.clone(),
                            equipment: eq_ref.clone(),
                            status: st.to_string(),
                            intervention_id: None,
                        });
                    }

                    // 3. Projections futures strictement après next_due jusqu'à `to`
                    let anchor_str = p.anchor_date.as_deref().unwrap_or(next_due_str);
                    if let Ok(anchor) = NaiveDate::parse_from_str(anchor_str, "%Y-%m-%d") {
                        let first =
                            calendar_step_at(anchor, freq, p.interval_days, from.max(next_due));
                        let last = calendar_step_at(anchor, freq, p.interval_days, to);
                        for step in first..=last {
                            let projected = step_calendar_date(anchor, freq, p.interval_days, step);
                            if projected > to {
                                break;
                            }
                            if projected > next_due && (from..=to).contains(&projected) {
                                let proj_str = projected.format("%Y-%m-%d").to_string();
                                if !processed_dates.contains(&proj_str) {
                                    processed_dates.insert(proj_str.clone());
                                    calendar_entries.push(ScheduleEntry {
                                        date: Some(proj_str),
                                        remaining: None,
                                        plan: plan_ref.clone(),
                                        equipment: eq_ref.clone(),
                                        status: "projected".to_string(),
                                        intervention_id: None,
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Tri des éléments calendaires par date croissante
    calendar_entries.sort_by(|a, b| a.date.cmp(&b.date));

    // Concaténation : calendrier d'abord, compteurs ensuite
    let mut all_entries = calendar_entries;
    all_entries.extend(counter_entries);

    Ok(all_entries)
}

// ============================================================================
// TÂCHE DE FOND : GÉNÉRATION AUTOMATIQUE
// ============================================================================

/// Traite les plannings dus qui ont `auto_generate = 1`.
pub async fn process_auto_generation(pool: &SqlitePool) {
    let today = Local::now().date_naive();

    let query = format!(
        "SELECT {PLAN_SELECT_FIELDS} {PLAN_FROM_JOINS} WHERE p.deleted_at IS NULL AND p.active = 1 AND p.auto_generate = 1"
    );

    let plans: Vec<RawPlanRow> = match sqlx::query_as(&query).fetch_all(pool).await {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(
                "Erreur lors de la lecture des plannings pour génération automatique : {e}"
            );
            return;
        }
    };

    for p in plans {
        // Pas d'occurrence generated ouverte
        if p.open_occ_id.is_some() {
            continue;
        }

        let is_due = if p.frequency == "counter" {
            let counter_active = p.counter_active.unwrap_or(0) == 1;
            let cur = p.counter_current_value.unwrap_or(0);
            let due = p.next_due_counter.unwrap_or(0);
            let lead = p.lead_counter;

            counter_active && (cur >= due - lead)
        } else if let Some(due_str) = &p.next_due_date {
            if let Ok(due_date) = NaiveDate::parse_from_str(due_str, "%Y-%m-%d") {
                let lead = p.lead_days;
                due_date - Duration::days(lead) <= today
            } else {
                false
            }
        } else {
            false
        };

        if !is_due {
            continue;
        }

        // Tenter la génération dans une transaction immédiate
        if let Ok(mut tx) = pool.begin_with("BEGIN IMMEDIATE").await {
            // Revérifier qu'aucune occurrence generated n'a été créée entre-temps
            let open_count: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM maintenance_occurrences WHERE plan_id = ? AND status = 'generated' AND deleted_at IS NULL",
            )
            .bind(&p.id)
            .fetch_one(&mut *tx)
            .await
            .unwrap_or(1);

            if open_count == 0 {
                let auto_actor = Actor::new("", Some(p.site_id.clone()), crate::Role::Admin, []);
                if let Err(e) = do_generate_in_tx(&mut tx, &p, &auto_actor, "auto", None).await {
                    tracing::warn!(
                        "Échec de génération automatique pour le planning {} : {e}",
                        p.code
                    );
                    let _ = tx.rollback().await;
                    continue;
                }
                if let Err(e) = tx.commit().await {
                    tracing::warn!("Échec de commit pour le planning {} : {e}", p.code);
                }
            } else {
                let _ = tx.rollback().await;
            }
        }
    }
}

/// Déclenche la génération automatique pour les plannings d'un compteur donné.
pub async fn process_auto_generation_for_counter(pool: &SqlitePool, counter_id: &str) {
    let query = format!(
        "SELECT {PLAN_SELECT_FIELDS} {PLAN_FROM_JOINS} WHERE p.counter_id = ? AND p.deleted_at IS NULL AND p.active = 1 AND p.auto_generate = 1"
    );

    let plans: Vec<RawPlanRow> = match sqlx::query_as(&query)
        .bind(counter_id)
        .fetch_all(pool)
        .await
    {
        Ok(p) => p,
        Err(_) => return,
    };

    for p in plans {
        if p.open_occ_id.is_some() {
            continue;
        }

        let counter_active = p.counter_active.unwrap_or(0) == 1;
        let cur = p.counter_current_value.unwrap_or(0);
        let due = p.next_due_counter.unwrap_or(0);
        let lead = p.lead_counter;

        if counter_active && (cur >= due - lead) {
            if let Ok(mut tx) = pool.begin_with("BEGIN IMMEDIATE").await {
                let open_count: i64 = sqlx::query_scalar(
                    "SELECT count(*) FROM maintenance_occurrences WHERE plan_id = ? AND status = 'generated' AND deleted_at IS NULL",
                )
                .bind(&p.id)
                .fetch_one(&mut *tx)
                .await
                .unwrap_or(1);

                if open_count == 0 {
                    let auto_actor =
                        Actor::new("", Some(p.site_id.clone()), crate::Role::Admin, []);
                    if do_generate_in_tx(&mut tx, &p, &auto_actor, "auto", None)
                        .await
                        .is_ok()
                    {
                        let _ = tx.commit().await;
                    } else {
                        let _ = tx.rollback().await;
                    }
                } else {
                    let _ = tx.rollback().await;
                }
            }
        }
    }
}

// ============================================================================
// TESTS UNITAIRES (CALCUL DES ÉCHÉANCES)
// ============================================================================

/// Génération après relevé externe, dans la transaction de ce relevé.
pub(crate) async fn generate_counter_in_tx(
    conn: &mut SqliteConnection,
    actor: &Actor,
    counter_id: &str,
) -> Result<u64, MaintError> {
    actor.require_permission(Permission::MaintenanceManage)?;
    let query=format!("SELECT {PLAN_SELECT_FIELDS} {PLAN_FROM_JOINS} WHERE p.counter_id=? AND (? OR p.site_id=?) AND p.deleted_at IS NULL AND p.active=1 AND p.auto_generate=1");
    let plans: Vec<RawPlanRow> = sqlx::query_as(&query)
        .bind(counter_id)
        .bind(actor.is_admin())
        .bind(&actor.site_id)
        .fetch_all(&mut *conn)
        .await?;
    let mut generated = 0;
    for plan in plans {
        if plan.open_occ_id.is_none()
            && plan.counter_active == Some(1)
            && plan.counter_current_value.unwrap_or(0)
                >= plan
                    .next_due_counter
                    .unwrap_or(0)
                    .saturating_sub(plan.lead_counter)
        {
            do_generate_in_tx(conn, &plan, actor, "auto", None).await?;
            generated += 1;
        }
    }
    Ok(generated)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn generation_automatique_reverifie_le_planning_sous_verrou() {
        let (pool, actor) = crate::audit_tests::fixture().await;
        let plan = crate::audit_tests::plan(&pool, &actor).await;
        sqlx::query("UPDATE maintenance_plans SET next_due_date='2000-01-01' WHERE id=?")
            .bind(&plan.id)
            .execute(&pool)
            .await
            .unwrap();
        let query = format!("SELECT {PLAN_SELECT_FIELDS} {PLAN_FROM_JOINS} WHERE p.id=?");
        let stale: RawPlanRow = sqlx::query_as(&query)
            .bind(&plan.id)
            .fetch_one(&pool)
            .await
            .unwrap();
        for change in [
            "active=0",
            "auto_generate=0",
            "deleted_at='2026'",
            "next_due_date='2099-01-01'",
        ] {
            let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
            sqlx::query(&format!("UPDATE maintenance_plans SET {change} WHERE id=?"))
                .bind(&plan.id)
                .execute(&mut *tx)
                .await
                .unwrap();
            assert!(matches!(
                do_generate_in_tx(&mut tx, &stale, &actor, "auto", None).await,
                Err(MaintError::Conflict(_))
            ));
            let count: i64 = sqlx::query_scalar("SELECT count(*) FROM maintenance_occurrences")
                .fetch_one(&mut *tx)
                .await
                .unwrap();
            assert_eq!(count, 0);
            tx.rollback().await.unwrap();
        }
    }

    #[test]
    fn test_calcul_chaque_frequence() {
        let anchor = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();

        assert_eq!(
            step_calendar_date(anchor, Frequency::Daily, None, 5),
            NaiveDate::from_ymd_opt(2026, 1, 6).unwrap()
        );
        assert_eq!(
            step_calendar_date(anchor, Frequency::Weekly, None, 2),
            NaiveDate::from_ymd_opt(2026, 1, 15).unwrap()
        );
        assert_eq!(
            step_calendar_date(anchor, Frequency::Monthly, None, 1),
            NaiveDate::from_ymd_opt(2026, 2, 1).unwrap()
        );
        assert_eq!(
            step_calendar_date(anchor, Frequency::Quarterly, None, 1),
            NaiveDate::from_ymd_opt(2026, 4, 1).unwrap()
        );
        assert_eq!(
            step_calendar_date(anchor, Frequency::Yearly, None, 2),
            NaiveDate::from_ymd_opt(2028, 1, 1).unwrap()
        );
        assert_eq!(
            step_calendar_date(anchor, Frequency::IntervalDays, Some(10), 3),
            NaiveDate::from_ymd_opt(2026, 1, 31).unwrap()
        );
    }

    #[test]
    fn test_ancrage_31_janvier_sans_derive() {
        // Ancrage au 31 janvier 2024 (année bissextile)
        let anchor_leap = NaiveDate::from_ymd_opt(2024, 1, 31).unwrap();

        // step 1 -> 29 février 2024
        assert_eq!(
            step_calendar_date(anchor_leap, Frequency::Monthly, None, 1),
            NaiveDate::from_ymd_opt(2024, 2, 29).unwrap()
        );
        // step 2 -> 31 mars 2024 (pas de dérive vers 29 mars !)
        assert_eq!(
            step_calendar_date(anchor_leap, Frequency::Monthly, None, 2),
            NaiveDate::from_ymd_opt(2024, 3, 31).unwrap()
        );
        // step 3 -> 30 avril 2024
        assert_eq!(
            step_calendar_date(anchor_leap, Frequency::Monthly, None, 3),
            NaiveDate::from_ymd_opt(2024, 4, 30).unwrap()
        );

        // Ancrage au 31 janvier 2025 (année non bissextile)
        let anchor_normal = NaiveDate::from_ymd_opt(2025, 1, 31).unwrap();
        assert_eq!(
            step_calendar_date(anchor_normal, Frequency::Monthly, None, 1),
            NaiveDate::from_ymd_opt(2025, 2, 28).unwrap()
        );
        assert_eq!(
            step_calendar_date(anchor_normal, Frequency::Monthly, None, 2),
            NaiveDate::from_ymd_opt(2025, 3, 31).unwrap()
        );
    }

    #[test]
    fn test_annees_bissextiles_yearly() {
        let anchor_29feb = NaiveDate::from_ymd_opt(2024, 2, 29).unwrap();

        // 2025 non bissextile -> 28 février
        assert_eq!(
            step_calendar_date(anchor_29feb, Frequency::Yearly, None, 1),
            NaiveDate::from_ymd_opt(2025, 2, 28).unwrap()
        );
        // 2028 bissextile -> 29 février à nouveau !
        assert_eq!(
            step_calendar_date(anchor_29feb, Frequency::Yearly, None, 4),
            NaiveDate::from_ymd_opt(2028, 2, 29).unwrap()
        );
    }

    #[test]
    fn test_calendrier_fixe_malgre_retard() {
        let anchor = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let current_due = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        // Clôture tardive le 2026-01-25
        let today = NaiveDate::from_ymd_opt(2026, 1, 25).unwrap();

        let res = advance_calendar(anchor, Frequency::Monthly, None, current_due, today);
        // next_due_date est 1er février (ancré), pas décalé
        assert_eq!(
            res.next_due_date,
            NaiveDate::from_ymd_opt(2026, 2, 1).unwrap()
        );
        assert!(res.missed_dates.is_empty());
    }

    #[test]
    fn test_echeances_missed_apres_longue_interruption() {
        let anchor = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let current_due = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        // Planning resté ouvert jusqu'au 15 mai 2026
        let today = NaiveDate::from_ymd_opt(2026, 5, 15).unwrap();

        let res = advance_calendar(anchor, Frequency::Monthly, None, current_due, today);

        // Les échéances de février, mars, avril sont missed
        assert_eq!(
            res.missed_dates,
            vec![
                NaiveDate::from_ymd_opt(2026, 2, 1).unwrap(),
                NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
                NaiveDate::from_ymd_opt(2026, 4, 1).unwrap(),
            ]
        );
        // next_due_date passe à la dernière échue <= aujourd'hui (1er mai 2026), qui apparaît en retard
        assert_eq!(
            res.next_due_date,
            NaiveDate::from_ymd_opt(2026, 5, 1).unwrap()
        );
    }

    #[test]
    fn test_echeances_compteur_et_remise_a_zero() {
        // Avancement standard de compteur
        let res = advance_counter(100, 1000, 1050);
        assert_eq!(res.next_due_counter, 1100);
        assert!(res.missed_counters.is_empty());

        // Long retard de compteur
        let res_missed = advance_counter(100, 1000, 1450);
        assert_eq!(res_missed.missed_counters, vec![1100, 1200, 1300]);
        assert_eq!(res_missed.next_due_counter, 1400);

        // Saut aberrant du compteur : rattrapage borné, sans allocation démesurée.
        let res_saut = advance_counter(1, 0, 1_000_000_000_000);
        assert_eq!(res_saut.next_due_counter, 1_000_000_000_000);
        assert_eq!(res_saut.missed_counters.len() as i64, MAX_MISSED_COUNTERS);
        assert_eq!(res_saut.missed_counters.last(), Some(&999_999_999_999));
        // Valeurs extrêmes : aucun dépassement d'entier.
        let res_max = advance_counter(i64::MAX, i64::MAX - 1, i64::MAX);
        assert_eq!(res_max.next_due_counter, i64::MAX);
        assert!(res_max.missed_counters.is_empty());
        let res_max = advance_counter(1, i64::MAX - 5, i64::MAX);
        assert_eq!(res_max.next_due_counter, i64::MAX);
        assert_eq!(res_max.missed_counters.len(), 4);

        // Remise à zéro préservant la distance restante
        // Compteur était à 950, prochaine échéance à 1000 -> distance = 50
        let old_val = 950;
        let old_due = 1000;
        let new_reading = 0; // reset à 0
        let remaining = old_due - old_val;
        let new_due = new_reading + remaining;
        assert_eq!(new_due, 50);
    }
}
