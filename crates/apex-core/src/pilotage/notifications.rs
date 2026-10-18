//! File d'envoi transactionnelle et transport SMTP sans secrets dans les erreurs publiées.
use super::settings::{put, raw};
use crate::{documents::storage::FileStore, store, Actor, MaintError, Permission, Role};
use lettre::{
    message::{Mailbox, MultiPart},
    transport::smtp::{
        authentication::Credentials,
        client::{Tls, TlsParameters},
    },
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
};
use serde_json::{json, Value};
use sqlx::{SqliteConnection, SqlitePool};
pub const EVENTS: &[&str] = &[
    "stock_low",
    "maintenance_generated",
    "maintenance_overdue",
    "intervention_critical",
    "intervention_assigned",
];
pub fn event_permission(event: &str) -> Result<Permission, MaintError> {
    Ok(match event {
        "stock_low" => Permission::StockView,
        "maintenance_overdue" => Permission::MaintenanceManage,
        "maintenance_generated" => Permission::InterventionsView,
        "intervention_critical" => Permission::InterventionsEdit,
        "intervention_assigned" => Permission::InterventionsView,
        _ => return Err(MaintError::BadRequest("Alerte inconnue".into())),
    })
}
pub fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
pub fn content(title: &str, text: &str, url: &str) -> (String, String) {
    (
        format!("{title}\n\n{text}\n\n{url}"),
        format!(
            "<h1>{}</h1><p>{}</p><p><a href=\"{}\">Ouvrir dans Apex Maint</a></p>",
            escape(title),
            escape(text),
            escape(url)
        ),
    )
}
// Les champs restent explicites pour les appels effectués dans les transactions métier.
#[allow(clippy::too_many_arguments)]
pub async fn queue(
    conn: &mut SqliteConnection,
    site: Option<&str>,
    to: &str,
    event: &str,
    dedup: Option<&str>,
    title: &str,
    text: &str,
    url: &str,
) -> Result<(), MaintError> {
    if raw(conn, "smtp.enabled").await? != "true" {
        return Ok(());
    }
    let (plain, html) = content(title, text, url);
    sqlx::query("INSERT OR IGNORE INTO outbox_emails(id,site_id,to_address,subject,body_text,body_html,event,dedup_key,created_at,next_attempt_at) VALUES(?,?,?,?,?,?,?,?,?,?)")
 .bind(store::id()).bind(site).bind(to).bind(title).bind(plain).bind(html).bind(event).bind(dedup).bind(store::now()).bind(store::now()).execute(conn).await?;
    Ok(())
}
async fn enabled(
    conn: &mut SqliteConnection,
    uid: &str,
    role: Role,
    event: &str,
) -> Result<bool, MaintError> {
    let own: Option<bool> = sqlx::query_scalar(
        "SELECT enabled FROM notification_preferences WHERE user_id=? AND event=?",
    )
    .bind(uid)
    .bind(event)
    .fetch_optional(&mut *conn)
    .await?;
    if let Some(b) = own {
        return Ok(b);
    }
    let defaults: Value =
        serde_json::from_str(&raw(conn, "alerts.defaults").await?).unwrap_or(json!({}));
    Ok(defaults[role.as_str()][event]
        .as_bool()
        .unwrap_or(role != Role::Admin))
}
#[allow(clippy::too_many_arguments)]
async fn recipients(
    conn: &mut SqliteConnection,
    actor: &Actor,
    site: &str,
    event: &str,
    target: Option<&str>,
    entity: &str,
    title: &str,
    path: &str,
) -> Result<(), MaintError> {
    if !actor.is_admin() && actor.site_id.as_deref() != Some(site) {
        return Err(MaintError::NotFound("Site".into()));
    }
    if raw(conn, "smtp.enabled").await? != "true" {
        return Ok(());
    }
    let users:Vec<(String,String,String)>=sqlx::query_as("SELECT id,role,email FROM users WHERE site_id=? AND active=1 AND deleted_at IS NULL AND email IS NOT NULL AND (? IS NULL OR id=?)").bind(site).bind(target).bind(target).fetch_all(&mut *conn).await?;
    let base = raw(conn, "public_base_url").await?;
    for (uid, role, email) in users {
        let role = Role::parse(&role).map_err(|_| MaintError::Internal("Rôle invalide".into()))?;
        let permission = if event == "maintenance_generated" && target.is_none() {
            Permission::MaintenanceManage
        } else {
            event_permission(event)?
        };
        let can = role == Role::Admin
            || sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(SELECT 1 FROM role_permissions WHERE role=? AND permission=?)",
            )
            .bind(role.as_str())
            .bind(permission.as_str())
            .fetch_one(&mut *conn)
            .await?;
        if can && enabled(conn, &uid, role, event).await? {
            let dedup = format!("{event}:{entity}:{uid}");
            queue(
                conn,
                Some(site),
                &email,
                event,
                Some(&dedup),
                title,
                title,
                &format!("{}{path}", base.trim_end_matches('/')),
            )
            .await?;
        }
    }
    Ok(())
}
pub async fn intervention(
    conn: &mut SqliteConnection,
    actor: &Actor,
    id: &str,
    critical: bool,
) -> Result<(), MaintError> {
    let (site,title,priority,state):(String,String,String,String)=sqlx::query_as("SELECT site_id,title,priority,machine_state FROM interventions WHERE id=? AND deleted_at IS NULL AND (? OR site_id=?)").bind(id).bind(actor.is_admin()).bind(&actor.site_id).fetch_one(&mut *conn).await?;
    if critical && (priority == "critical" || state == "ARRET") {
        recipients(
            conn,
            actor,
            &site,
            "intervention_critical",
            None,
            id,
            &format!("Intervention urgente : {title}"),
            &format!("/#/interventions/{id}"),
        )
        .await?;
    }
    let assigned: Vec<(String, String)> = sqlx::query_as(
        "SELECT user_id,assigned_at FROM intervention_technicians WHERE intervention_id=?",
    )
    .bind(id)
    .fetch_all(&mut *conn)
    .await?;
    for (uid, assigned_at) in assigned {
        recipients(
            conn,
            actor,
            &site,
            "intervention_assigned",
            Some(&uid),
            &format!("{id}:{assigned_at}"),
            &format!("Intervention affectée : {title}"),
            &format!("/#/interventions/{id}"),
        )
        .await?;
    }
    Ok(())
}
pub async fn stock(conn: &mut SqliteConnection, actor: &Actor) -> Result<(), MaintError> {
    if raw(conn, "smtp.enabled").await? != "true" {
        return Ok(());
    }
    let rows:Vec<(String,String,String,i64,i64)>=sqlx::query_as("SELECT p.id,p.site_id,p.name,COALESCE((SELECT SUM(quantity) FROM stock WHERE part_id=p.id AND site_id=p.site_id),0),COALESCE(p.alert_threshold,CAST((SELECT value FROM app_settings WHERE key='stock_alert_threshold') AS INTEGER)) FROM parts p WHERE p.deleted_at IS NULL AND (? OR p.site_id=?)")
 .bind(actor.is_admin()).bind(&actor.site_id).fetch_all(&mut *conn).await?;
    for (id, site, name, quantity, threshold) in rows {
        let key = format!("alerts.stock_low.{id}");
        let previous = raw(conn, &key).await?;
        if quantity < threshold {
            if previous != "low" {
                recipients(
                    conn,
                    actor,
                    &site,
                    "stock_low",
                    None,
                    &format!("{id}:{}", store::id()),
                    &format!("Stock bas : {name}"),
                    &format!("/#/pieces/{id}"),
                )
                .await?;
                put(conn, &key, "low").await?;
            }
        } else {
            put(conn, &key, "normal").await?;
        }
    }
    Ok(())
}
pub async fn generated(
    conn: &mut SqliteConnection,
    actor: &Actor,
    plan: &str,
    intervention: &str,
) -> Result<(), MaintError> {
    let (site,title,target):(String,String,Option<String>)=sqlx::query_as("SELECT site_id,title,default_technician_id FROM maintenance_plans WHERE id=? AND deleted_at IS NULL AND (? OR site_id=?)").bind(plan).bind(actor.is_admin()).bind(&actor.site_id).fetch_one(&mut *conn).await?;
    recipients(
        conn,
        actor,
        &site,
        "maintenance_generated",
        target.as_deref(),
        intervention,
        &format!("Maintenance générée : {title}"),
        &format!("/#/interventions/{intervention}"),
    )
    .await
}
pub async fn preferences(
    pool: &SqlitePool,
    actor: &Actor,
    input: Option<Value>,
) -> Result<Value, MaintError> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    if let Some(input) = input {
        let before: Vec<(String, bool)> = sqlx::query_as(
            "SELECT event,enabled FROM notification_preferences WHERE user_id=? ORDER BY event",
        )
        .bind(&actor.user_id)
        .fetch_all(&mut *tx)
        .await?;
        let map = input
            .as_object()
            .ok_or_else(|| MaintError::BadRequest("Objet JSON attendu".into()))?;
        for (event, v) in map {
            actor.require_permission(event_permission(event)?)?;
            let b = v
                .as_bool()
                .ok_or_else(|| MaintError::BadRequest("Préférence booléenne attendue".into()))?;
            sqlx::query("INSERT INTO notification_preferences(user_id,event,enabled) VALUES(?,?,?) ON CONFLICT(user_id,event) DO UPDATE SET enabled=excluded.enabled").bind(&actor.user_id).bind(event).bind(b).execute(&mut *tx).await?;
        }
        store::audit(
            &mut tx,
            actor,
            store::Audit {
                action: "update",
                entity: "notification_preferences",
                entity_id: Some(&actor.user_id),
                site_id: actor.site_id.as_deref(),
                before: Some(json!(before
                    .into_iter()
                    .collect::<std::collections::BTreeMap<_, _>>())),
                after: Some(input),
                ip: None,
            },
        )
        .await?;
    }
    let mut out = serde_json::Map::new();
    for event in EVENTS {
        if actor.can(event_permission(event)?) {
            out.insert(
                (*event).into(),
                json!(enabled(&mut tx, &actor.user_id, actor.role, event).await?),
            );
        }
    }
    tx.commit().await?;
    Ok(Value::Object(out))
}
#[derive(Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutboxFilter {
    pub cursor: Option<String>,
    pub limit: Option<u32>,
}
pub async fn outbox(
    pool: &SqlitePool,
    actor: &Actor,
    filter: &OutboxFilter,
) -> Result<Value, MaintError> {
    actor.require_permission(Permission::SettingsView)?;
    type OutboxRow = (
        String,
        Option<String>,
        String,
        String,
        String,
        i64,
        Option<String>,
        String,
        Option<String>,
    );
    let cursor = filter
        .cursor
        .as_deref()
        .map(crate::Cursor::decode)
        .transpose()?;
    let timestamp = cursor.as_ref().and_then(|c| c.timestamp.as_deref());
    if cursor.is_some() && timestamp.is_none() {
        return Err(MaintError::BadRequest("Curseur temporel attendu".into()));
    }
    let limit = filter.limit.unwrap_or(100).clamp(1, 1000);
    let mut rows:Vec<OutboxRow>=sqlx::query_as("SELECT id,site_id,to_address,event,status,attempts,last_error,created_at,sent_at FROM outbox_emails WHERE (? OR site_id=?) AND (? IS NULL OR (created_at,id)<(?,?)) ORDER BY created_at DESC,id DESC LIMIT ?")
        .bind(actor.is_admin()).bind(&actor.site_id).bind(timestamp).bind(timestamp).bind(cursor.as_ref().map(|c|&c.id)).bind(i64::from(limit)+1).fetch_all(pool).await?;
    let more = rows.len() > limit as usize;
    rows.truncate(limit as usize);
    let next_cursor = if more {
        rows.last()
            .map(|r| crate::Cursor::with_timestamp(&r.0, &r.7).encode())
    } else {
        None
    };
    Ok(
        json!({"items":rows.into_iter().map(|r|json!({"id":r.0,"site_id":r.1,"to_address":r.2,"event":r.3,"status":r.4,"attempts":r.5,"last_error":r.6,"created_at":r.7,"sent_at":r.8})).collect::<Vec<_>>(),"next_cursor":next_cursor}),
    )
}
pub async fn test(pool: &SqlitePool, actor: &Actor) -> Result<(), MaintError> {
    actor.require_permission(Permission::SettingsEdit)?;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    if raw(&mut tx, "smtp.enabled").await? != "true" {
        return Err(MaintError::BadRequest("SMTP désactivé".into()));
    }
    let address:Option<String>=sqlx::query_scalar("SELECT email FROM users WHERE id=? AND active=1 AND deleted_at IS NULL AND (? OR site_id=?)").bind(&actor.user_id).bind(actor.is_admin()).bind(&actor.site_id).fetch_one(&mut *tx).await?;
    let address = address.ok_or_else(|| {
        MaintError::BadRequest("Votre compte ne possède pas d'adresse e-mail".into())
    })?;
    queue(
        &mut tx,
        actor.site_id.as_deref(),
        &address,
        "test",
        None,
        "Test Apex Maint",
        "La configuration SMTP permet l'envoi d'e-mails.",
        "",
    )
    .await?;
    tx.commit().await?;
    Ok(())
}
pub async fn send_pending(pool: &SqlitePool, files: &FileStore) -> Result<(), MaintError> {
    let mut conn = pool.acquire().await?;
    if raw(&mut conn, "smtp.enabled").await? != "true" {
        return Ok(());
    }
    let host = raw(&mut conn, "smtp.host").await?;
    let port = raw(&mut conn, "smtp.port")
        .await?
        .parse::<u16>()
        .unwrap_or(587);
    let security = raw(&mut conn, "smtp.security").await?;
    let mut builder = AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&host)
        .port(port)
        .timeout(Some(std::time::Duration::from_secs(10)));
    if security != "none" {
        let tls = TlsParameters::new(host)
            .map_err(|_| MaintError::BadRequest("Nom de serveur SMTP invalide".into()))?;
        builder = builder.tls(if security == "tls" {
            Tls::Wrapper(tls)
        } else {
            Tls::Required(tls)
        });
    }
    let username = raw(&mut conn, "smtp.username").await?;
    let enc = raw(&mut conn, "smtp.password_enc").await?;
    if !username.is_empty() {
        builder = builder.credentials(Credentials::new(
            username,
            if enc.is_empty() {
                String::new()
            } else {
                files.decrypt(&enc)?
            },
        ));
    }
    let transport = builder.build();
    let from = Mailbox::new(
        Some(raw(&mut conn, "smtp.from_name").await?),
        raw(&mut conn, "smtp.from_address")
            .await?
            .parse()
            .map_err(|_| MaintError::BadRequest("Expéditeur SMTP invalide".into()))?,
    );
    let rows:Vec<(String,String,String,String,String,i64)>=sqlx::query_as("SELECT id,to_address,subject,body_text,body_html,attempts FROM outbox_emails WHERE status='pending' AND next_attempt_at<=? ORDER BY created_at LIMIT 50").bind(store::now()).fetch_all(&mut *conn).await?;
    drop(conn);
    for (id, to, subject, text, html, attempts) in rows {
        let message = to.parse().ok().and_then(|to| {
            Message::builder()
                .from(from.clone())
                .to(Mailbox::new(None, to))
                .subject(subject)
                .multipart(MultiPart::alternative_plain_html(text, html))
                .ok()
        });
        let success = if let Some(message) = message {
            transport.send(message).await.is_ok()
        } else {
            false
        };
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
        let delay = [1, 5, 30, 120].get(attempts as usize).copied();
        let status = if success {
            "sent"
        } else if delay.is_some() {
            "pending"
        } else {
            "failed"
        };
        let next = (chrono::Utc::now() + chrono::Duration::minutes(delay.unwrap_or(120)))
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        sqlx::query("UPDATE outbox_emails SET status=?,attempts=attempts+1,last_error=?,sent_at=?,next_attempt_at=? WHERE id=? AND status='pending'").bind(status).bind(if success{None}else{Some("Échec de l'envoi SMTP ; vérifiez la connexion et les paramètres")}).bind(if success{Some(store::now())}else{None}).bind(next).bind(id).execute(&mut *tx).await?;
        tx.commit().await?;
    }
    Ok(())
}
pub async fn digest(pool: &SqlitePool) -> Result<(), MaintError> {
    use chrono::Timelike;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    if raw(&mut tx, "smtp.enabled").await? != "true" {
        return Ok(());
    }
    let now = chrono::Local::now();
    let hour = raw(&mut tx, "alerts.overdue_digest_hour")
        .await?
        .parse::<u32>()
        .unwrap_or(7);
    let today = now.date_naive().to_string();
    if now.hour() < hour || raw(&mut tx, "alerts.last_digest_date").await? == today {
        return Ok(());
    }
    let sites:Vec<String>=sqlx::query_scalar("SELECT DISTINCT p.site_id FROM maintenance_plans p LEFT JOIN equipment_counters c ON c.id=p.counter_id AND c.active=1 AND c.deleted_at IS NULL WHERE p.deleted_at IS NULL AND p.active=1 AND (p.next_due_date<? OR c.current_value>=p.next_due_counter)").bind(&today).fetch_all(&mut *tx).await?;
    let system = Actor::new("", None, Role::Admin, []);
    for site in sites {
        recipients(
            &mut tx,
            &system,
            &site,
            "maintenance_overdue",
            None,
            &today,
            "Des échéances de maintenance sont en retard",
            "/#/maintenance",
        )
        .await?;
    }
    put(&mut tx, "alerts.last_digest_date", &today).await?;
    tx.commit().await?;
    Ok(())
}
