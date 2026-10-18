//! Jeux construits à la main et fichiers relus, sans SMTP extérieur.
use super::*;
use crate::{db, documents::storage::FileStore, store, Actor, MaintError, Permission, Role};
use calamine::{Data, Reader, Xlsx};
use serde_json::json;

async fn fixture() -> (sqlx::SqlitePool, Actor) {
    let pool = db::open(None).await.unwrap();
    for site in ["a", "b"] {
        sqlx::query("INSERT INTO sites(id,name,created_at,updated_at) VALUES(?,?,?,?)")
            .bind(site)
            .bind(site)
            .bind(store::now())
            .bind(store::now())
            .execute(&pool)
            .await
            .unwrap();
    }
    for (id, site, role) in [
        ("admin", "a", "admin"),
        ("responsable", "a", "responsable"),
        ("tech", "a", "technicien"),
        ("autre", "b", "responsable"),
    ] {
        sqlx::query("INSERT INTO users(id,site_id,username,display_name,role,password_hash,email,created_at,updated_at) VALUES(?,?,?,?,?,'hash',?,?,?)").bind(id).bind(site).bind(id).bind(id).bind(role).bind(format!("{id}@example.test")).bind(store::now()).bind(store::now()).execute(&pool).await.unwrap();
    }
    for (id, site) in [("eq-a", "a"), ("eq-b", "b")] {
        sqlx::query("INSERT INTO equipments(id,site_id,code,name,created_at,updated_at) VALUES(?,?,?,?,?,?)").bind(id).bind(site).bind(id).bind(id).bind(store::now()).bind(store::now()).execute(&pool).await.unwrap();
    }
    (pool, Actor::new("admin", Some("a".into()), Role::Admin, []))
}
fn files() -> FileStore {
    FileStore::new(std::env::temp_dir().join("apex-unused"), [17; 32])
}
async fn smtp(pool: &sqlx::SqlitePool, actor: &Actor) {
    settings::save(pool,&files(),actor,"smtp",json!({"host":"127.0.0.1","port":std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port(),"security":"none","from_address":"apex@example.test","enabled":true})).await.unwrap();
    settings::save(
        pool,
        &files(),
        actor,
        "general",
        json!({"public_base_url":"https://maint.example.test"}),
    )
    .await
    .unwrap();
}
fn layout(name: &str) -> dashboard::Layout {
    serde_json::from_value(json!({"widgets":[{"id":"w1","widget":name,"x":0,"y":0,"w":6,"h":3,"params":{"period":"30d"}}]})).unwrap()
}
#[test]
fn validation_dispositions_et_parametres() {
    assert!(dashboard::validate(&layout("inconnu")).is_err());
    let mut l = layout("stock_alerts");
    l.widgets[0].params = json!({"period":"hier"});
    assert!(dashboard::validate(&l).is_err());
    l.widgets[0].params = json!({"from":"date invalide","to":"2026-01-01"});
    assert!(dashboard::validate(&l).is_err());
    l.widgets = vec![layout("stock_alerts").widgets[0].clone(); 25];
    assert!(dashboard::validate(&l).is_err());
}
#[tokio::test]
async fn resolution_dispositions_et_droits() {
    let (pool, admin) = fixture().await;
    dashboard::save(
        &pool,
        &admin,
        Some(Role::Technicien),
        Some(layout("parts_cost")),
    )
    .await
    .unwrap();
    let tech = Actor::new(
        "tech",
        Some("a".into()),
        Role::Technicien,
        [Permission::DashboardView, Permission::ReportingView],
    );
    let result = dashboard::resolve(&pool, &tech).await.unwrap();
    assert_eq!(result["source"], "role");
    assert_eq!(result["widgets"], json!([]));
    assert!(matches!(
        dashboard::save(&pool, &tech, None, Some(layout("stock_value"))).await,
        Err(MaintError::Forbidden(_))
    ));
    dashboard::save(&pool, &admin, None, Some(layout("interventions_summary")))
        .await
        .unwrap();
    assert_eq!(
        dashboard::resolve(&pool, &admin).await.unwrap()["source"],
        "user"
    );
    dashboard::save(&pool, &admin, None, None).await.unwrap();
    assert_eq!(
        dashboard::resolve(&pool, &admin).await.unwrap()["source"],
        "builtin"
    );
}
#[test]
fn formats_export_et_formules() {
    for v in ["=1", "+2", "-3", "@a", "\tt", "\rr"] {
        assert!(exports::neutralize(v).starts_with('\''));
    }
    let headers: Vec<exports::Column> =
        vec!["texte".into(), "prix_cents".into(), "created_at".into()];
    let rows = vec![vec![
        json!("=SOMME(A1)"),
        json!(1234),
        json!("2026-10-18T12:34:00Z"),
    ]];
    let csv = exports::csv(&headers, &rows);
    assert!(csv.starts_with(&[239, 187, 191]));
    let text = String::from_utf8(csv).unwrap();
    assert!(text.contains(";\"12,34\";"));
    assert!(text.contains("'=SOMME"));
    assert!(text.contains("18/10/2026"));
    let bytes = exports::xlsx(&headers, &rows).unwrap();
    let mut book = Xlsx::new(std::io::Cursor::new(bytes)).unwrap();
    let range = book.worksheet_range("Sheet1").unwrap();
    assert_eq!(
        range.get_value((1, 0)),
        Some(&Data::String("'=SOMME(A1)".into()))
    );
    assert_eq!(range.get_value((1, 1)), Some(&Data::Float(12.34)));
    assert!(matches!(range.get_value((1, 2)), Some(Data::DateTime(_))));
    assert!(exports::pdf("Essai français", &headers, &rows, true)
        .unwrap()
        .starts_with(b"%PDF"));
}
#[tokio::test]
async fn smtp_chiffrement_validation_et_audit_masque() {
    let (pool, admin) = fixture().await;
    let result = settings::save(
        &pool,
        &files(),
        &admin,
        "smtp",
        json!({"password":"secret-test"}),
    )
    .await
    .unwrap();
    assert_eq!(result["has_password"], true);
    assert!(result.get("password").is_none());
    let enc = settings::raw(&mut pool.acquire().await.unwrap(), "smtp.password_enc")
        .await
        .unwrap();
    assert!(!enc.contains("secret-test"));
    assert_eq!(files().decrypt(&enc).unwrap(), "secret-test");
    let audit = audit::list(&pool, &admin, &Default::default())
        .await
        .unwrap();
    assert!(!audit.to_string().contains("secret-test"));
    assert!(!audit.to_string().contains(&enc));
    for (section, input) in [
        ("security", json!({"session_timeout_min":14})),
        ("audit", json!({"retention_days":364})),
        ("smtp", json!({"from_address":"invalide"})),
        ("general", json!({"public_base_url":"file:///local"})),
    ] {
        assert!(settings::save(&pool, &files(), &admin, section, input)
            .await
            .is_err());
    }
}
#[test]
fn courrier_echappe_et_lien() {
    let (plain, html) =
        notifications::content("Titre <test>", "A & B", "https://example.test/?a=1&b=2");
    assert!(plain.contains("A & B"));
    assert!(html.contains("&lt;test&gt;"));
    assert!(html.contains("A &amp; B"));
    assert!(html.contains("?a=1&amp;b=2"));
}
async fn panne(
    pool: &sqlx::SqlitePool,
    id: &str,
    site: &str,
    eq: &str,
    reported: &str,
    completed: Option<&str>,
) {
    sqlx::query("INSERT INTO interventions(id,site_id,code,title,type,priority,status,machine_state,equipment_id,reported_by_id,reported_at,started_at,completed_at,created_at,updated_at) VALUES(?,?,?,?,'corrective','high',?,'ARRET',?,'admin',?,?,?,?,?)")
 .bind(id).bind(site).bind(id).bind(id).bind(if completed.is_some(){"completed"}else{"open"}).bind(eq).bind(reported).bind(reported).bind(completed).bind(store::now()).bind(store::now()).execute(pool).await.unwrap();
}
fn periode() -> reports::Filter {
    reports::Filter {
        from: Some("2026-01-01T00:00:00Z".into()),
        to: Some("2026-01-02T00:00:00Z".into()),
        ..Default::default()
    }
}
#[tokio::test]
async fn indicateurs_panne_ouverte_aucune_panne_et_isolation() {
    let (pool, admin) = fixture().await;
    let f = periode();
    assert_eq!(
        reports::report(&pool, &admin, "reliability", &f)
            .await
            .unwrap()["data"]["no_failures"],
        true
    );
    panne(
        &pool,
        "i1",
        "a",
        "eq-a",
        "2026-01-01T01:00:00Z",
        Some("2026-01-01T03:00:00Z"),
    )
    .await;
    panne(&pool, "i2", "a", "eq-a", "2026-01-01T22:00:00Z", None).await;
    panne(&pool, "i3", "b", "eq-b", "2026-01-01T01:00:00Z", None).await;
    let actor = Actor::new(
        "responsable",
        Some("a".into()),
        Role::Responsable,
        [Permission::ReportingView],
    );
    let data = reports::report(&pool, &actor, "reliability", &f)
        .await
        .unwrap();
    let row = &data["data"]["items"][0];
    assert_eq!(data["data"]["items"].as_array().unwrap().len(), 1);
    assert_eq!(row["failures"], 2);
    assert!((row["downtime_minutes"].as_f64().unwrap() - 240.).abs() < 0.001);
    assert!((row["mttr_minutes"].as_f64().unwrap() - 240.).abs() < 0.001);
    assert!((row["mtbf_minutes"].as_f64().unwrap() - 600.).abs() < 0.001);
    let mut foreign = f.clone();
    foreign.equipment_id = Some("eq-b".into());
    assert!(matches!(
        reports::report(&pool, &actor, "activity", &foreign).await,
        Err(MaintError::NotFound(_))
    ));
    let data = reports::report(&pool, &actor, "activity", &f)
        .await
        .unwrap();
    assert_eq!(data["data"]["response_mean_minutes"], 0.0);
}
#[tokio::test]
async fn audit_filtres_pagination_diff_et_purge() {
    let (pool, admin) = fixture().await;
    settings::save(
        &pool,
        &files(),
        &admin,
        "general",
        json!({"app_name":"Essai"}),
    )
    .await
    .unwrap();
    settings::save(
        &pool,
        &files(),
        &admin,
        "general",
        json!({"app_name":"Deuxième"}),
    )
    .await
    .unwrap();
    let actor = Actor::new(
        "responsable",
        Some("a".into()),
        Role::Responsable,
        [Permission::AuditView],
    );
    let f = audit::Filter {
        limit: Some(1),
        entity: Some("settings".into()),
        ..Default::default()
    };
    let page = audit::list(&pool, &actor, &f).await.unwrap();
    assert_eq!(page["items"].as_array().unwrap().len(), 1);
    assert!(page["items"][0]["diff"].is_array());
    let next = audit::list(
        &pool,
        &actor,
        &audit::Filter {
            cursor: page["next_cursor"].as_str().map(str::to_owned),
            ..f
        },
    )
    .await
    .unwrap();
    assert_ne!(page["items"][0]["id"], next["items"][0]["id"]);
    sqlx::query("UPDATE audit_log SET created_at='2000-01-01T00:00:00.000Z'")
        .execute(&pool)
        .await
        .unwrap();
    audit::purge(&pool).await.unwrap();
    let page = audit::list(&pool, &admin, &Default::default())
        .await
        .unwrap();
    assert_eq!(page["items"].as_array().unwrap().len(), 1);
    assert_eq!(page["items"][0]["action"], "purge");
    assert_eq!(page["items"][0]["after"]["deleted_count"], 2);
    assert_eq!(
        audit::list(&pool, &actor, &Default::default())
            .await
            .unwrap()["items"],
        json!([])
    );
}
#[tokio::test]
async fn file_transactionnelle_destinataires_et_annulation() {
    let (pool, admin) = fixture().await;
    smtp(&pool, &admin).await;
    panne(&pool, "i1", "a", "eq-a", "2026-01-01T01:00:00Z", None).await;
    {
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
        notifications::intervention(&mut tx, &admin, "i1", true)
            .await
            .unwrap();
        tx.rollback().await.unwrap();
    }
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM outbox_emails")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    {
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
        notifications::intervention(&mut tx, &admin, "i1", true)
            .await
            .unwrap();
        tx.commit().await.unwrap();
    }
    let addresses: Vec<String> = sqlx::query_scalar("SELECT to_address FROM outbox_emails")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert!(addresses.contains(&"responsable@example.test".into()));
    assert!(!addresses.contains(&"autre@example.test".into()));
    assert!(!addresses.contains(&"admin@example.test".into()));
    let count = addresses.len();
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
    notifications::intervention(&mut tx, &admin, "i1", true)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM outbox_emails")
            .fetch_one(&pool)
            .await
            .unwrap(),
        count as i64
    );
}
#[tokio::test]
async fn reset_jeton_unique_expire_sessions_et_limites() {
    let (pool, admin) = fixture().await;
    smtp(&pool, &admin).await;
    reset::request(&pool, "inconnu", "127.0.0.1").await.unwrap();
    for _ in 0..5 {
        reset::request(&pool, "tech", "127.0.0.1").await.unwrap();
    }
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM password_reset_tokens")
            .fetch_one(&pool)
            .await
            .unwrap(),
        3
    );
    let text: String = sqlx::query_scalar(
        "SELECT body_text FROM outbox_emails ORDER BY created_at DESC,id DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let token = text.split("jeton=").nth(1).unwrap().trim();
    let stored: String =
        sqlx::query_scalar("SELECT token_hash FROM password_reset_tokens WHERE used_at IS NULL")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_ne!(stored, token);
    sqlx::query("INSERT INTO sessions(token_hash,user_id,created_at,last_seen_at) VALUES('session','tech',?,?)").bind(store::now()).bind(store::now()).execute(&pool).await.unwrap();
    reset::confirm(&pool, token, "Nouveau-test123".into())
        .await
        .unwrap();
    assert!(reset::confirm(&pool, token, "Encore-test123".into())
        .await
        .is_err());
    assert!(sqlx::query_scalar::<_, Option<String>>(
        "SELECT revoked_at FROM sessions WHERE token_hash='session'"
    )
    .fetch_one(&pool)
    .await
    .unwrap()
    .is_some());
    reset::request(&pool, "autre", "127.0.0.2").await.unwrap();
    let text: String = sqlx::query_scalar(
        "SELECT body_text FROM outbox_emails WHERE to_address='autre@example.test'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let token = text.split("jeton=").nth(1).unwrap().trim();
    sqlx::query("UPDATE password_reset_tokens SET expires_at='2000-01-01' WHERE user_id='autre'")
        .execute(&pool)
        .await
        .unwrap();
    assert!(reset::confirm(&pool, token, "Nouveau-test123".into())
        .await
        .is_err());
    for _ in 0..15 {
        reset::request(&pool, "inconnu", "127.0.0.3").await.unwrap();
    }
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM password_reset_requests WHERE requested_ip='127.0.0.3'"
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        10
    );
}

#[tokio::test]
async fn smtp_local_capture_et_reprises() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let (pool, admin) = fixture().await;
    smtp(&pool, &admin).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    assert!(![3000, 3100].contains(&port));
    settings::save(&pool, &files(), &admin, "smtp", json!({"port":port}))
        .await
        .unwrap();
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let (reader, mut writer) = socket.into_split();
        let mut reader = BufReader::new(reader);
        writer.write_all(b"220 local.test ESMTP\r\n").await.unwrap();
        let mut message = String::new();
        let mut data = false;
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).await.unwrap() == 0 {
                break;
            }
            if data {
                if line == ".\r\n" {
                    data = false;
                    writer.write_all(b"250 message recu\r\n").await.unwrap();
                } else {
                    message.push_str(&line);
                }
            } else if line.starts_with("EHLO") {
                writer
                    .write_all(b"250-local.test\r\n250 8BITMIME\r\n")
                    .await
                    .unwrap();
            } else if line.starts_with("DATA") {
                data = true;
                writer.write_all(b"354 contenu\r\n").await.unwrap();
            } else if line.starts_with("QUIT") {
                writer.write_all(b"221 termine\r\n").await.unwrap();
                break;
            } else {
                writer.write_all(b"250 ok\r\n").await.unwrap();
            }
        }
        message
    });
    notifications::test(&pool, &admin).await.unwrap();
    notifications::send_pending(&pool, &files()).await.unwrap();
    let message = tokio::time::timeout(std::time::Duration::from_secs(15), server)
        .await
        .unwrap()
        .unwrap();
    assert!(message.contains("admin@example.test"));
    assert!(message.contains("multipart/alternative"));
    let status: String = sqlx::query_scalar("SELECT status FROM outbox_emails")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status, "sent");
    // Ce port vient d'être fermé : aucun serveur externe n'est contacté.
    notifications::test(&pool, &admin).await.unwrap();
    notifications::send_pending(&pool, &files()).await.unwrap();
    let (status, attempts, error): (String, i64, String) = sqlx::query_as(
        "SELECT status,attempts,last_error FROM outbox_emails WHERE status='pending'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(status, "pending");
    assert_eq!(attempts, 1);
    assert!(!error.is_empty());
    for n in 1..5 {
        sqlx::query("UPDATE outbox_emails SET next_attempt_at='2000-01-01' WHERE status='pending'")
            .execute(&pool)
            .await
            .unwrap();
        notifications::send_pending(&pool, &files()).await.unwrap();
        let attempts: i64 =
            sqlx::query_scalar("SELECT attempts FROM outbox_emails WHERE status<>'sent'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(attempts, n + 1);
    }
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT status FROM outbox_emails WHERE status<>'sent'")
            .fetch_one(&pool)
            .await
            .unwrap(),
        "failed"
    );
}

#[tokio::test]
async fn stock_bas_deduplication_et_rearmement() {
    let (pool, admin) = fixture().await;
    smtp(&pool, &admin).await;
    sqlx::query("INSERT INTO parts(id,site_id,reference,name,alert_threshold,created_at,updated_at) VALUES('piece','a','REF','Pièce',5,?,?)").bind(store::now()).bind(store::now()).execute(&pool).await.unwrap();
    for _ in 0..2 {
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
        notifications::stock(&mut tx, &admin).await.unwrap();
        tx.commit().await.unwrap();
    }
    let count =
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM outbox_emails WHERE event='stock_low'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(count > 0);
    sqlx::query("UPDATE parts SET alert_threshold=0 WHERE id='piece'")
        .execute(&pool)
        .await
        .unwrap();
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
    notifications::stock(&mut tx, &admin).await.unwrap();
    tx.commit().await.unwrap();
    sqlx::query("UPDATE parts SET alert_threshold=5 WHERE id='piece'")
        .execute(&pool)
        .await
        .unwrap();
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
    notifications::stock(&mut tx, &admin).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM outbox_emails WHERE event='stock_low'")
            .fetch_one(&pool)
            .await
            .unwrap(),
        count * 2
    );
}

#[tokio::test]
async fn taux_preventif_et_performance_50000_interventions() {
    let (pool, admin) = fixture().await;
    panne(
        &pool,
        "faite",
        "a",
        "eq-a",
        "2026-01-01T01:00:00Z",
        Some("2026-01-01T03:00:00Z"),
    )
    .await;
    sqlx::query("INSERT INTO maintenance_plans(id,site_id,code,title,equipment_id,frequency,created_at,updated_at) VALUES('plan','a','PLAN','Préventif','eq-a','daily',?,?)").bind(store::now()).bind(store::now()).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO maintenance_occurrences(id,site_id,plan_id,due_date,status,intervention_id,skip_reason,created_at,updated_at) VALUES('o1','a','plan','2026-01-01','done','faite',NULL,?,?),('o2','a','plan','2026-01-02','skipped',NULL,'Arrêt planifié',?,?)").bind(store::now()).bind(store::now()).bind(store::now()).bind(store::now()).execute(&pool).await.unwrap();
    let f = reports::Filter {
        from: Some("2026-01-01".into()),
        to: Some("2026-01-02".into()),
        ..Default::default()
    };
    let data = reports::report(&pool, &admin, "preventive", &f)
        .await
        .unwrap();
    assert_eq!(data["data"]["due_count"], 2);
    assert_eq!(data["data"]["on_time_count"], 1);
    assert_eq!(data["data"]["compliance_percent"], 50.);
    sqlx::query("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<50000) INSERT INTO interventions(id,site_id,code,title,type,equipment_id,reported_by_id,reported_at,created_at,updated_at) SELECT 'perf-'||x,'a','PERF-'||x,'Essai','corrective','eq-a','admin','2026-01-01T12:00:00.000Z','2026-01-01T12:00:00.000Z','2026-01-01T12:00:00.000Z' FROM n").execute(&pool).await.unwrap();
    let start = std::time::Instant::now();
    reports::report(&pool, &admin, "activity", &f)
        .await
        .unwrap();
    reports::report(&pool, &admin, "reliability", &f)
        .await
        .unwrap();
    if !cfg!(debug_assertions) {
        assert!(
            start.elapsed() < std::time::Duration::from_millis(500),
            "Agrégats trop lents : {:?}",
            start.elapsed()
        );
    }
}

#[tokio::test]
async fn plafond_exports_et_filtres_stock() {
    let (pool, admin) = fixture().await;
    sqlx::query("INSERT INTO parts(id,site_id,reference,name,created_at,updated_at) VALUES('piece','a','REF','Pièce',?,?)").bind(store::now()).bind(store::now()).execute(&pool).await.unwrap();
    sqlx::query("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<100001) INSERT INTO locations(id,site_id,code,name,kind,created_at,updated_at) SELECT 'loc-'||x,'a','LOC-'||x,'Emplacement','magasin','2026-01-01','2026-01-01' FROM n").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO stock(id,site_id,part_id,location_id,quantity,updated_at) SELECT id,'a','piece',id,1,'2026-01-01' FROM locations").execute(&pool).await.unwrap();
    let mut p = exports::Params::new();
    p.insert("format".into(), "csv".into());
    assert!(matches!(
        exports::list(&pool, &admin, "stock", &p).await,
        Err(MaintError::PayloadTooLarge(_))
    ));
    p.insert("format".into(), "pdf".into());
    assert!(matches!(
        exports::list(&pool, &admin, "stock", &p).await,
        Err(MaintError::PayloadTooLarge(_))
    ));
    p.insert("location_id".into(), "loc-1".into());
    assert_eq!(
        exports::list(&pool, &admin, "stock", &p)
            .await
            .unwrap()
            .len(),
        1
    );
}
