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
pub fn table(rows: &[Value]) -> (Vec<String>, Vec<Vec<Value>>) {
    let mut keys = std::collections::BTreeSet::new();
    for row in rows {
        if let Some(o) = row.as_object() {
            keys.extend(o.keys().cloned());
        }
    }
    let keys: Vec<_> = keys.into_iter().collect();
    let data = rows
        .iter()
        .map(|r| keys.iter().map(|k| r[k].clone()).collect())
        .collect();
    (keys, data)
}
fn date(s: &str) -> Option<chrono::DateTime<chrono::FixedOffset>> {
    chrono::DateTime::parse_from_rfc3339(s).ok()
}
fn display(v: &Value, key: &str) -> String {
    if v.is_null() {
        return String::new();
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
pub fn csv_line(headers: &[String], row: &[Value]) -> Vec<u8> {
    let mut s = row
        .iter()
        .enumerate()
        .map(|(i, v)| format!("\"{}\"", display(v, &headers[i]).replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(";");
    s.push_str("\r\n");
    s.into_bytes()
}
pub fn csv(headers: &[String], rows: &[Vec<Value>]) -> Vec<u8> {
    let mut out = vec![0xef, 0xbb, 0xbf];
    out.extend(csv_line(
        headers,
        &headers.iter().map(|s| json!(s)).collect::<Vec<_>>(),
    ));
    for r in rows {
        out.extend(csv_line(headers, r));
    }
    out
}
pub fn xlsx(headers: &[String], rows: &[Vec<Value>]) -> Result<Vec<u8>, MaintError> {
    let mut workbook = rust_xlsxwriter::Workbook::new();
    let sheet = workbook.add_worksheet();
    let header = rust_xlsxwriter::Format::new().set_bold();
    let money = rust_xlsxwriter::Format::new().set_num_format("# ##0.00 \"€\"");
    let dt = rust_xlsxwriter::Format::new().set_num_format("dd/mm/yyyy hh:mm");
    for (col, key) in headers.iter().enumerate() {
        sheet
            .write_string_with_format(0, col as u16, neutralize(key), &header)
            .map_err(fail)?;
    }
    for (row, values) in rows.iter().enumerate() {
        for (col, value) in values.iter().enumerate() {
            let (r, c) = (row as u32 + 1, col as u16);
            if let Some(n) = value.as_f64() {
                if headers[col].ends_with("_cents") {
                    sheet
                        .write_number_with_format(r, c, n / 100., &money)
                        .map_err(fail)?;
                } else {
                    sheet.write_number(r, c, n).map_err(fail)?;
                }
            } else if let Some(b) = value.as_bool() {
                sheet.write_boolean(r, c, b).map_err(fail)?;
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
    headers: &[String],
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
                    .element(Paragraph::new(key))
                    .element(Paragraph::new(display(value, key)))
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
