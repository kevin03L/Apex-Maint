//! Exports filtrés : neutralisation des formules et types natifs dans les classeurs.
use crate::{
    interventions, maintenance, referentiel, stock, store, Actor, Cursor, MaintError, Permission,
};
use chrono::{Datelike, Timelike};
use serde_json::{json, Value};
use sqlx::SqlitePool;
use std::collections::BTreeMap;
pub type Params = BTreeMap<String, String>;
pub fn neutralize(s: &str) -> String {
    if s.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{s}")
    } else {
        s.into()
    }
}
fn fail(e: impl std::fmt::Display) -> MaintError {
    MaintError::Internal(format!("Génération du document impossible : {e}"))
}
fn string(p: &Params, k: &str) -> Option<String> {
    p.get(k).cloned().filter(|s| !s.is_empty())
}
fn boolean(p: &Params, k: &str) -> Result<Option<bool>, MaintError> {
    p.get(k)
        .map(|s| {
            s.parse()
                .map_err(|_| MaintError::BadRequest(format!("Filtre booléen invalide : {k}")))
        })
        .transpose()
}
pub async fn list(
    pool: &SqlitePool,
    actor: &Actor,
    name: &str,
    p: &Params,
) -> Result<Vec<Value>, MaintError> {
    actor.require_permission(Permission::ExportsRun)?;
    let mut cursor = None;
    let mut items = Vec::new();
    loop {
        let (mut rows, next) = match name {
            "interventions" => {
                let page = interventions::list_interventions(
                    pool,
                    actor,
                    interventions::InterventionFilter {
                        q: string(p, "q"),
                        site_id: string(p, "site_id"),
                        status: string(p, "status"),
                        r#type: string(p, "type"),
                        priority: string(p, "priority"),
                        machine_state: string(p, "machine_state"),
                        equipment_id: string(p, "equipment_id"),
                        assigned_to: string(p, "assigned_to"),
                        parent_id: string(p, "parent_id"),
                        date_from: string(p, "date_from"),
                        date_to: string(p, "date_to"),
                        cursor: cursor.clone(),
                        limit: 200,
                    },
                )
                .await?;
                (
                    page.items.into_iter().map(|v| json!(v)).collect::<Vec<_>>(),
                    page.next_cursor,
                )
            }
            "equipments" => {
                let page = referentiel::list_equipments(
                    pool,
                    actor,
                    referentiel::EquipmentFilter {
                        q: string(p, "q"),
                        site_id: string(p, "site_id"),
                        status: string(p, "status"),
                        production_line_id: string(p, "production_line_id"),
                        parent_id: string(p, "parent_id"),
                        cursor: cursor.clone(),
                        limit: 200,
                    },
                )
                .await?;
                (
                    page.items.into_iter().map(|v| json!(v)).collect(),
                    page.next_cursor,
                )
            }
            "parts" => {
                let page = stock::list_parts(
                    pool,
                    actor,
                    stock::PartFilter {
                        q: string(p, "q"),
                        site_id: string(p, "site_id"),
                        category: string(p, "category"),
                        supplier_id: string(p, "supplier_id"),
                        low_stock: boolean(p, "low_stock")?,
                        cursor: cursor.clone(),
                        limit: 200,
                    },
                )
                .await?;
                (
                    page.items.into_iter().map(|v| json!(v)).collect(),
                    page.next_cursor,
                )
            }
            "stock" => {
                let rows = stock::list_stock(
                    pool,
                    actor,
                    stock::StockFilter {
                        site_id: string(p, "site_id"),
                        part_id: string(p, "part_id"),
                        location_id: string(p, "location_id"),
                    },
                )
                .await?;
                (rows.into_iter().map(|v| json!(v)).collect(), None)
            }
            "movements" => {
                let page = stock::list_stock_movements(
                    pool,
                    actor,
                    stock::StockMovementFilter {
                        q: string(p, "q"),
                        site_id: string(p, "site_id"),
                        part_id: string(p, "part_id"),
                        movement_type: string(p, "movement_type"),
                        intervention_id: string(p, "intervention_id"),
                        location_id: string(p, "location_id"),
                        user_id: string(p, "user_id"),
                        date_from: string(p, "date_from"),
                        date_to: string(p, "date_to"),
                        cursor: cursor.clone(),
                        limit: 200,
                    },
                )
                .await?;
                (
                    page.items.into_iter().map(|v| json!(v)).collect(),
                    page.next_cursor,
                )
            }
            "maintenance_plans" => {
                let page = maintenance::list_plans(
                    pool,
                    actor,
                    maintenance::PlanFilter {
                        q: string(p, "q"),
                        site_id: string(p, "site_id"),
                        equipment_id: string(p, "equipment_id"),
                        frequency: string(p, "frequency"),
                        active: boolean(p, "active")?,
                        due: string(p, "due"),
                        cursor: cursor.clone(),
                        limit: 200,
                    },
                )
                .await?;
                (
                    page.items.into_iter().map(|v| json!(v)).collect(),
                    page.next_cursor,
                )
            }
            "audit" => {
                let page = super::audit::list(
                    pool,
                    actor,
                    &super::audit::Filter {
                        from: string(p, "from"),
                        to: string(p, "to"),
                        site_id: string(p, "site_id"),
                        user_id: string(p, "user_id"),
                        entity: string(p, "entity"),
                        action: string(p, "action"),
                        entity_id: string(p, "entity_id"),
                        q: string(p, "q"),
                        cursor: cursor.as_ref().map(Cursor::encode),
                        limit: Some(1000),
                    },
                )
                .await?;
                (
                    page["items"].as_array().cloned().unwrap_or_default(),
                    page["next_cursor"].as_str().map(str::to_owned),
                )
            }
            _ => return Err(MaintError::NotFound("Liste à exporter".into())),
        };
        items.append(&mut rows);
        let limit = if p.get("format").is_some_and(|s| s == "pdf") {
            5000
        } else {
            100000
        };
        if items.len() > limit {
            return Err(MaintError::PayloadTooLarge(format!(
                "Export limité à {limit} lignes ; affinez les filtres"
            )));
        }
        match next {
            Some(c) => cursor = Some(Cursor::decode(&c)?),
            None => break,
        }
    }
    Ok(items)
}
pub async fn record(
    pool: &SqlitePool,
    actor: &Actor,
    name: &str,
    format: &str,
    params: &Params,
    count: usize,
) -> Result<(), MaintError> {
    actor.require_permission(Permission::ExportsRun)?;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    store::audit(
        &mut tx,
        actor,
        store::Audit {
            action: "export",
            entity: "export",
            entity_id: None,
            site_id: actor.site_id.as_deref(),
            before: None,
            after: Some(json!({"type":name,"format":format,"filters":params,"row_count":count})),
            ip: None,
        },
    )
    .await?;
    tx.commit().await?;
    Ok(())
}
/// Colonnes exportées par liste : chemin dans la représentation JSON (`a.b` pour un objet
/// imbriqué) et libellé de l'en-tête.
fn columns(name: &str) -> Option<&'static [(&'static str, &'static str)]> {
    Some(match name {
        "interventions" => &[
            ("code", "Code"),
            ("title", "Titre"),
            ("type", "Type"),
            ("priority", "Priorité"),
            ("status", "Statut"),
            ("machine_state", "État machine"),
            ("equipment.code", "Équipement"),
            ("equipment.name", "Nom de l'équipement"),
            ("reported_by.display_name", "Déclarée par"),
            ("reported_at", "Déclarée le"),
            ("started_at", "Démarrée le"),
            ("completed_at", "Terminée le"),
            ("duration_minutes", "Durée (min)"),
            ("solution_report", "Compte rendu"),
        ],
        "equipments" => &[
            ("code", "Code"),
            ("name", "Nom"),
            ("status", "Statut"),
            ("production_line.name", "Ligne de production"),
            ("parent.code", "Équipement parent"),
            ("manufacturer", "Fabricant"),
            ("model", "Modèle"),
            ("serial_number", "N° de série"),
            ("commissioned_on", "Mise en service"),
            ("children_count", "Sous-équipements"),
            ("documents_count", "Documents"),
        ],
        "parts" => &[
            ("reference", "Référence"),
            ("name", "Désignation"),
            ("category", "Catégorie"),
            ("unit", "Unité"),
            ("unit_price_cents", "Prix unitaire"),
            ("total_stock", "Stock"),
            ("total_reserved", "Réservé"),
            ("total_available", "Disponible"),
            ("effective_threshold", "Seuil d'alerte"),
            ("is_low_stock", "Stock bas"),
            ("supplier.name", "Fournisseur"),
            ("supplier_reference", "Réf. fournisseur"),
            ("manufacturer", "Fabricant"),
            ("manufacturer_reference", "Réf. fabricant"),
        ],
        "stock" => &[
            ("part_reference", "Référence"),
            ("part_name", "Désignation"),
            ("location_code", "Emplacement"),
            ("location_name", "Nom de l'emplacement"),
            ("quantity", "Quantité"),
            ("reserved_quantity", "Réservé"),
            ("available_quantity", "Disponible"),
            ("updated_at", "Mis à jour le"),
        ],
        "movements" => &[
            ("created_at", "Date"),
            ("movement_type", "Type"),
            ("part_reference", "Référence"),
            ("part_name", "Désignation"),
            ("quantity", "Quantité"),
            ("delta", "Écart d'inventaire"),
            ("from_location_code", "Depuis"),
            ("to_location_code", "Vers"),
            ("intervention_code", "Intervention"),
            ("unit_price_cents", "Prix unitaire"),
            ("reason", "Motif"),
            ("user_display_name", "Utilisateur"),
        ],
        "maintenance_plans" => &[
            ("code", "Code"),
            ("title", "Titre"),
            ("equipment.code", "Équipement"),
            ("equipment.name", "Nom de l'équipement"),
            ("frequency", "Fréquence"),
            ("interval_days", "Intervalle (jours)"),
            ("counter_interval", "Intervalle compteur"),
            ("priority", "Priorité"),
            ("next_due_date", "Prochaine échéance"),
            ("next_due_counter", "Échéance compteur"),
            ("lead_days", "Anticipation (jours)"),
            ("estimated_minutes", "Durée estimée (min)"),
            ("auto_generate", "Génération automatique"),
            ("active", "Actif"),
        ],
        "audit" => &[
            ("created_at", "Date"),
            ("user_display_name", "Utilisateur"),
            ("action", "Action"),
            ("entity", "Entité"),
            ("entity_id", "Identifiant"),
            ("ip", "Adresse IP"),
            ("before", "Avant"),
            ("after", "Après"),
        ],
        _ => return None,
    })
}
/// Libellé français d'une clé exportée ; à défaut, la clé elle-même.
pub fn label(key: &str) -> String {
    let known: &[(&str, &str)] = &[
        ("section", "Section"),
        ("equipment_code", "Code équipement"),
        ("equipment_name", "Équipement"),
        ("user_display_name", "Intervenant"),
        ("name", "Désignation"),
        ("reference", "Référence"),
        ("key", "Valeur"),
        ("count", "Nombre"),
        ("equipment_id", "Équipement"),
        ("user_id", "Utilisateur"),
        ("part_id", "Pièce"),
        ("failures", "Pannes"),
        ("closed_failures", "Pannes closes"),
        ("downtime_minutes", "Arrêt (min)"),
        ("mttr_minutes", "MTTR (min)"),
        ("mtbf_minutes", "MTBF (min)"),
        ("no_failures", "Aucune panne"),
        ("message", "Message"),
        ("due_count", "Échéances dues"),
        ("on_time_count", "Réalisées à l'heure"),
        ("compliance_percent", "Respect (%)"),
        ("parts_cost_cents", "Coût des pièces"),
        ("cost_cents", "Coût"),
        ("value_cents", "Valeur"),
        ("threshold", "Seuil"),
        ("low", "Stock bas"),
        ("response_mean_minutes", "Délai moyen de réponse (min)"),
        ("response_median_minutes", "Délai médian de réponse (min)"),
        ("description", "Description"),
        ("created_at", "Créée le"),
        ("updated_at", "Mise à jour le"),
        ("cancelled_at", "Annulée le"),
        ("cancel_reason", "Motif d'annulation"),
        ("technicians_summary", "Intervenants"),
        ("equipment_label", "Équipement"),
        ("code", "Code"),
        ("title", "Titre"),
        ("type", "Type"),
        ("priority", "Priorité"),
        ("status", "Statut"),
        ("machine_state", "État machine"),
        ("reported_at", "Déclarée le"),
        ("started_at", "Démarrée le"),
        ("completed_at", "Terminée le"),
        ("duration_minutes", "Durée (min)"),
        ("solution_report", "Compte rendu"),
        ("documents_count", "Documents"),
        ("quantity", "Quantité"),
        ("unit_price_cents", "Prix unitaire"),
        ("part_reference", "Référence"),
        ("part_name", "Désignation"),
        ("maintenance_plan_url", "Planning préventif"),
        ("sub_interventions_count", "Sous-interventions"),
        ("location_code", "Emplacement"),
    ];
    known
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, l)| (*l).to_string())
        .unwrap_or_else(|| key.to_string())
}
/// Colonne d'export : clé (chemin JSON, qui guide aussi le format) et en-tête affiché.
#[derive(Debug, Clone)]
pub struct Column {
    pub key: String,
    pub label: String,
}
impl From<&str> for Column {
    fn from(key: &str) -> Self {
        Self {
            key: key.into(),
            label: label(key),
        }
    }
}
/// Valeur d'une colonne : chemin `a.b` vers un objet imbriqué.
fn lookup(row: &Value, path: &str) -> Value {
    path.split('.')
        .try_fold(row, |v, k| v.get(k))
        .cloned()
        .unwrap_or(Value::Null)
}
/// Libellé français des codes d'énumération, pour une lecture directe du fichier.
pub fn code_label(key: &str, code: &str) -> Option<&'static str> {
    let field = key.rsplit('.').next().unwrap_or(key);
    Some(match (field, code) {
        ("status", "open") => "Ouverte",
        ("status", "in_progress") => "En cours",
        ("status", "completed") => "Terminée",
        ("status", "cancelled") => "Annulée",
        ("status", "operational") => "En service",
        ("status", "degraded") => "Dégradé",
        ("status", "out_of_service") => "Hors service",
        ("priority", "low") => "Basse",
        ("priority", "medium") => "Moyenne",
        ("priority", "high") => "Haute",
        ("priority", "critical") => "Critique",
        ("type", "corrective") => "Corrective",
        ("type", "preventive") => "Préventive",
        ("type", "a_posteriori") => "A posteriori",
        ("type", "hors_equipement") => "Hors équipement",
        ("machine_state", "ARRET") => "À l'arrêt",
        ("machine_state", "DEGRADE") => "Dégradé",
        ("machine_state", "FONCTIONNEL") => "Fonctionnel",
        ("movement_type", "in") => "Entrée",
        ("movement_type", "out") => "Sortie",
        ("movement_type", "transfer") => "Transfert",
        ("movement_type", "adjustment") => "Inventaire",
        ("frequency", "daily") => "Quotidienne",
        ("frequency", "weekly") => "Hebdomadaire",
        ("frequency", "monthly") => "Mensuelle",
        ("frequency", "quarterly") => "Trimestrielle",
        ("frequency", "yearly") => "Annuelle",
        ("frequency", "interval_days") => "Tous les N jours",
        ("frequency", "counter") => "Compteur",
        ("unit", "piece") => "Pièce",
        ("unit", "metre") => "Mètre",
        ("unit", "litre") => "Litre",
        ("unit", "kg") => "kg",
        ("unit", "lot") => "Lot",
        ("unit", "boite") => "Boîte",
        _ => return None,
    })
}
/// Tableau d'export : colonnes choisies pour les listes connues ; sinon toutes les clés
/// simples, sans identifiants techniques ni objets imbriqués.
pub fn table(name: &str, rows: &[Value]) -> (Vec<Column>, Vec<Vec<Value>>) {
    let keys: Vec<Column> = match columns(name) {
        Some(cols) => cols
            .iter()
            .map(|(k, l)| Column {
                key: (*k).into(),
                label: (*l).into(),
            })
            .collect(),
        None => {
            let mut keys = std::collections::BTreeSet::new();
            for row in rows {
                if let Some(o) = row.as_object() {
                    keys.extend(
                        o.iter()
                            .filter(|(k, v)| {
                                !matches!(k.as_str(), "id" | "site_id")
                                    && !k.ends_with("_id")
                                    && !v.is_object()
                                    && !v.is_array()
                            })
                            .map(|(k, _)| k.clone()),
                    );
                }
            }
            // La section d'un rapport ouvre chaque ligne.
            let mut keys: Vec<String> = keys.into_iter().collect();
            if let Some(i) = keys.iter().position(|k| k == "section") {
                let section = keys.remove(i);
                keys.insert(0, section);
            }
            keys.into_iter().map(|k| Column::from(k.as_str())).collect()
        }
    };
    let data = rows
        .iter()
        .map(|r| {
            keys.iter()
                .map(|k| {
                    let v = lookup(r, &k.key);
                    match v.as_str().and_then(|s| code_label(&k.key, s)) {
                        Some(l) => json!(l),
                        None => v,
                    }
                })
                .collect()
        })
        .collect();
    (keys, data)
}
fn date(s: &str) -> Option<chrono::DateTime<chrono::FixedOffset>> {
    chrono::DateTime::parse_from_rfc3339(s).ok()
}
/// Date civile `AAAA-MM-JJ` (mise en service, échéance), sans fuseau.
fn civil_date(s: &str) -> Option<chrono::NaiveDate> {
    if s.len() == 10 {
        chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
    } else {
        None
    }
}
fn display(v: &Value, key: &str) -> String {
    if v.is_null() {
        return String::new();
    }
    if let Some(b) = v.as_bool() {
        return if b { "Oui" } else { "Non" }.into();
    }
    if let Some(d) = v.as_str().and_then(civil_date) {
        return d.format("%d/%m/%Y").to_string();
    }
    if key.ends_with("_cents") {
        if let Some(n) = v.as_i64() {
            return format!("{:.2}", n as f64 / 100.).replace('.', ",");
        }
    }
    if let Some(s) = v.as_str() {
        if let Some(d) = date(s) {
            return d
                .with_timezone(&chrono::Local)
                .format("%d/%m/%Y %H:%M")
                .to_string();
        }
        return neutralize(s);
    }
    if v.is_number() {
        return v.to_string().replace('.', ",");
    }
    neutralize(&v.to_string())
}
pub fn csv_line(headers: &[Column], row: &[Value]) -> Vec<u8> {
    let mut s = row
        .iter()
        .enumerate()
        .map(|(i, v)| format!("\"{}\"", display(v, &headers[i].key).replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(";");
    s.push_str("\r\n");
    s.into_bytes()
}
pub fn csv(headers: &[Column], rows: &[Vec<Value>]) -> Vec<u8> {
    let mut out = vec![0xef, 0xbb, 0xbf];
    out.extend(csv_line(
        headers,
        &headers.iter().map(|c| json!(c.label)).collect::<Vec<_>>(),
    ));
    for r in rows {
        out.extend(csv_line(headers, r));
    }
    out
}
pub fn xlsx(headers: &[Column], rows: &[Vec<Value>]) -> Result<Vec<u8>, MaintError> {
    let mut workbook = rust_xlsxwriter::Workbook::new();
    let sheet = workbook.add_worksheet();
    let header = rust_xlsxwriter::Format::new().set_bold();
    let money = rust_xlsxwriter::Format::new().set_num_format("# ##0.00 \"€\"");
    let dt = rust_xlsxwriter::Format::new().set_num_format("dd/mm/yyyy hh:mm");
    let day = rust_xlsxwriter::Format::new().set_num_format("dd/mm/yyyy");
    for (col, key) in headers.iter().enumerate() {
        sheet
            .write_string_with_format(0, col as u16, neutralize(&key.label), &header)
            .map_err(fail)?;
    }
    for (row, values) in rows.iter().enumerate() {
        for (col, value) in values.iter().enumerate() {
            let (r, c) = (row as u32 + 1, col as u16);
            if let Some(n) = value.as_f64() {
                if headers[col].key.ends_with("_cents") {
                    sheet
                        .write_number_with_format(r, c, n / 100., &money)
                        .map_err(fail)?;
                } else {
                    sheet.write_number(r, c, n).map_err(fail)?;
                }
            } else if let Some(b) = value.as_bool() {
                sheet.write_boolean(r, c, b).map_err(fail)?;
            } else if let Some(d) = value.as_str().and_then(civil_date) {
                let value = rust_xlsxwriter::ExcelDateTime::from_ymd(
                    d.year() as u16,
                    d.month() as u8,
                    d.day() as u8,
                )
                .map_err(fail)?;
                sheet
                    .write_datetime_with_format(r, c, &value, &day)
                    .map_err(fail)?;
            } else if let Some(d) = value.as_str().and_then(date) {
                let d = d.with_timezone(&chrono::Local);
                let value = rust_xlsxwriter::ExcelDateTime::from_ymd(
                    d.year() as u16,
                    d.month() as u8,
                    d.day() as u8,
                )
                .and_then(|v| v.and_hms(d.hour() as u16, d.minute() as u8, d.second() as f64))
                .map_err(fail)?;
                sheet
                    .write_datetime_with_format(r, c, &value, &dt)
                    .map_err(fail)?;
            } else if !value.is_null() {
                sheet
                    .write_string(
                        r,
                        c,
                        neutralize(
                            value
                                .as_str()
                                .map(str::to_owned)
                                .unwrap_or_else(|| value.to_string())
                                .as_str(),
                        ),
                    )
                    .map_err(fail)?;
            }
        }
    }
    sheet.set_freeze_panes(1, 0).map_err(fail)?;
    if !headers.is_empty() {
        sheet
            .autofilter(0, 0, rows.len() as u32, headers.len() as u16 - 1)
            .map_err(fail)?;
    }
    sheet.autofit();
    workbook.save_to_buffer().map_err(fail)
}
pub fn pdf(
    title: &str,
    headers: &[Column],
    rows: &[Vec<Value>],
    landscape: bool,
) -> Result<Vec<u8>, MaintError> {
    use genpdf::{
        elements::Paragraph,
        fonts::{FontData, FontFamily},
        Element,
    };
    let fonts = FontFamily {
        regular: FontData::new(
            include_bytes!("../../assets/fonts/LiberationSans-Regular.ttf").to_vec(),
            None,
        )
        .map_err(fail)?,
        bold: FontData::new(
            include_bytes!("../../assets/fonts/LiberationSans-Bold.ttf").to_vec(),
            None,
        )
        .map_err(fail)?,
        italic: FontData::new(
            include_bytes!("../../assets/fonts/LiberationSans-Italic.ttf").to_vec(),
            None,
        )
        .map_err(fail)?,
        bold_italic: FontData::new(
            include_bytes!("../../assets/fonts/LiberationSans-BoldItalic.ttf").to_vec(),
            None,
        )
        .map_err(fail)?,
    };
    let mut doc = genpdf::Document::new(fonts);
    doc.set_title(title);
    doc.set_font_size(9);
    if landscape {
        doc.set_paper_size(genpdf::Size::new(297, 210));
    }
    let mut decorator = genpdf::SimplePageDecorator::new();
    decorator.set_margins(10);
    doc.set_page_decorator(decorator);
    doc.push(Paragraph::new(title).styled(genpdf::style::Style::new().bold().with_font_size(16)));
    for row in rows {
        let mut table = genpdf::elements::TableLayout::new(vec![1, 3]);
        table.set_cell_decorator(genpdf::elements::FrameCellDecorator::new(true, true, false));
        for (key, value) in headers.iter().zip(row) {
            if !value.is_null() {
                table
                    .row()
                    .element(Paragraph::new(key.label.as_str()))
                    .element(Paragraph::new(display(value, &key.key)))
                    .push()
                    .map_err(fail)?;
            }
        }
        doc.push(table);
        doc.push(genpdf::elements::Break::new(1));
    }
    let mut bytes = Vec::new();
    doc.render(&mut bytes).map_err(fail)?;
    Ok(bytes)
}
