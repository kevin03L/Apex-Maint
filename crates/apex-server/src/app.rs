//! Cycle de vie du moteur : démarrage contrôlé, exécution, arrêt propre.
//!
//! Ordre de démarrage (tout échec arrête le processus immédiatement) :
//! 1. configuration : lecture, migration en mémoire, validation (code 3) ;
//! 2. journaux (sortie standard + fichiers journaliers avec journal de démarrage en cas d'erreur) ;
//! 3. verrou d'instance unique sur data_dir (code 4) ;
//! 4. réservation du port web TCP listener AVANT d'ouvrir la base (code 5) ;
//! 5. base de données SQLite + migrations automatiques (code 6) ;
//! 6. jeton de contrôle local run/control.token (aléatoire 256 bits, régénéré à chaque démarrage, supprimé à l'arrêt) ;
//! 7. service HTTP Axum et attente des signaux d'arrêt ;
//! 8. arrêt propre : suppression du jeton, fermeture propre de la base avec checkpoint WAL TRUNCATE, libération du verrou.

use std::{net::SocketAddr, path::PathBuf, sync::Arc};

use apex_config::{has_errors, store, AppConfig, Layout, LayoutKind, Severity};
use chrono::Utc;
use tokio::sync::watch;
use tracing::{error, info, warn};

use crate::{
    cli::exit,
    http::{self, AppState, RuntimeInfo},
    instance::{InstanceLock, LockError},
    logging,
};

/// Mode d'exécution (affiché dans l'état et l'utilitaire desktop).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunMode {
    Manual,
    Service,
}

impl RunMode {
    pub fn as_str(self) -> &'static str {
        match self {
            RunMode::Manual => "manual",
            RunMode::Service => "service",
        }
    }
}

/// Échec de démarrage, avec le code de sortie correspondant.
#[derive(Debug)]
pub struct StartupError {
    pub code: i32,
    pub message: String,
}

impl StartupError {
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

/// Configuration effective : fichier (ou valeurs par défaut) + surcharges éventuelles.
pub struct Effective {
    pub layout: Layout,
    pub config: AppConfig,
    pub file_exists: bool,
    pub migrated_from: Option<u32>,
    /// Empreinte de la configuration du fichier chargée au démarrage.
    pub file_fingerprint: String,
}

impl Effective {
    /// Le logiciel est-il installé ? (mode portable, ou dossier de configuration système créé
    /// par l'installateur). Sinon, rien ne doit être créé dans les emplacements système.
    pub fn installed(&self) -> bool {
        self.layout.kind == LayoutKind::Portable
            || self.layout.config_file.parent().is_some_and(|d| d.is_dir())
    }
}

/// Lit la configuration effective sans rien démarrer (utilisé aussi par `config check/show`).
pub fn load_effective(config_path: Option<&std::path::Path>) -> Result<Effective, StartupError> {
    let layout = Layout::resolve(config_path);
    let default = match layout.kind {
        LayoutKind::Portable => AppConfig::portable_default(),
        LayoutKind::System => AppConfig::default(),
    };
    let file_exists = layout.config_file.is_file();
    let loaded = store::load_or(&layout.config_file, default)
        .map_err(|e| StartupError::new(exit::CONFIG, e.to_string()))?;
    let config = loaded.config;
    let file_fingerprint = config.fingerprint();
    let layout = layout.with_overrides(&config);
    Ok(Effective {
        layout,
        config,
        file_exists,
        migrated_from: loaded.migrated_from,
        file_fingerprint,
    })
}

/// Démarre le moteur et le serveur web, puis attend l'arrêt.
pub async fn run(
    config_path: Option<PathBuf>,
    mode: RunMode,
    mut stop: watch::Receiver<bool>,
    on_ready: impl FnOnce(),
) -> Result<(), StartupError> {
    // 1. Configuration (code 3 en cas d'erreur)
    let eff = match load_effective(config_path.as_deref()) {
        Ok(e) => e,
        Err(e) => {
            let layout = Layout::resolve(config_path.as_deref());
            if layout.kind == LayoutKind::Portable
                || layout.config_file.parent().is_some_and(|d| d.is_dir())
            {
                logging::startup_error(&layout.log_dir, &e.message);
            }
            return Err(e);
        }
    };
    if !eff.installed() {
        return Err(StartupError::new(
            exit::CONFIG,
            format!(
                "logiciel non installé : « {} » n'existe pas. Installez Apex Maint, ou lancez \
                 une instance portable avec --config <fichier>.",
                eff.layout
                    .config_file
                    .parent()
                    .map(|d| d.display().to_string())
                    .unwrap_or_default()
            ),
        ));
    }
    let issues = eff.config.validate();
    if has_errors(&issues) {
        let detail = issues
            .iter()
            .filter(|i| i.severity == Severity::Error)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join("\n  ");
        let message = format!(
            "configuration invalide ({}) :\n  {}",
            eff.layout.config_file.display(),
            detail
        );
        logging::startup_error(&eff.layout.log_dir, &message);
        return Err(StartupError::new(exit::CONFIG, message));
    }
    crate::tls::check(&eff.config.server.tls, &eff.layout.data_dir)
        .map_err(|e| StartupError::new(exit::CONFIG, e))?;

    // 2. Journaux
    let _log_guard = logging::init(&eff.config, &eff.layout.log_dir).map_err(|e| {
        logging::startup_error(&eff.layout.log_dir, &e);
        StartupError::new(exit::FAILURE, e)
    })?;

    let cfg = &eff.config;
    let layout = &eff.layout;
    info!("================================================================");
    info!(
        "  {} {} — instance « {} »",
        apex_config::version::PRODUCT_NAME,
        apex_config::version::full_version(),
        cfg.instance.name
    );
    info!("================================================================");
    info!(
        "Configuration : {}{}",
        layout.config_file.display(),
        if eff.file_exists {
            ""
        } else {
            " (absente : valeurs par défaut)"
        }
    );
    if let Some(v) = eff.migrated_from {
        warn!(
            "Configuration au format v{v} migrée en mémoire vers v{} : exécutez « apex-maint config migrate » pour réécrire le fichier",
            apex_config::CONFIG_VERSION
        );
    }
    for i in issues.iter().filter(|i| i.severity == Severity::Warning) {
        warn!("Configuration : {i}");
    }
    info!(
        "Données : {} · Journaux : {}",
        layout.data_dir.display(),
        layout.log_dir.display()
    );

    // 3. Instance unique sur data_dir (code 4)
    let _instance = InstanceLock::acquire(&layout.lock_file()).map_err(|e| {
        let code = match e {
            LockError::AlreadyRunning { .. } => exit::ALREADY_RUNNING,
            LockError::Io(_) => exit::FAILURE,
        };
        error!("{e}");
        StartupError::new(code, e.to_string())
    })?;

    let tls = crate::tls::prepare(&cfg.server.tls, &layout.data_dir, &cfg.server.bind)
        .map_err(|e| StartupError::new(exit::CONFIG, e))?;
    let redirect_listener = if let Some(port) = cfg.server.tls.http_redirect_port {
        Some(
            tokio::net::TcpListener::bind((cfg.server.bind.as_str(), port))
                .await
                .map_err(|e| StartupError::new(exit::BIND, format!("Port de redirection : {e}")))?,
        )
    } else {
        None
    };

    // 4. Réservation du port web AVANT d'ouvrir la base (code 5)
    let ip: std::net::IpAddr = cfg.server.bind.trim().parse().map_err(|_| {
        StartupError::new(
            exit::CONFIG,
            format!("adresse d'écoute invalide « {} »", cfg.server.bind),
        )
    })?;
    let addr = SocketAddr::new(ip, cfg.server.port);
    let listener = tokio::net::TcpListener::bind(addr).await.map_err(|e| {
        let message = format!(
            "impossible d'écouter sur {addr} : {e} (port déjà utilisé par un autre programme, \
             ou adresse non attribuée à ce poste)"
        );
        error!("{message}");
        StartupError::new(exit::BIND, message)
    })?;

    // 5. Base de données + migrations (code 6)
    let db_file = layout.database_file(cfg);
    let pool = apex_core::db::open(Some(&db_file)).await.map_err(|e| {
        let message = format!(
            "base de données « {} » inaccessible : {e}",
            db_file.display()
        );
        error!("{message}");
        StartupError::new(exit::DATABASE, message)
    })?;
    info!("Base SQLite : {} (mode WAL)", db_file.display());
    let auth = apex_core::services::Auth::new(pool.clone(), cfg.security.session_ttl_hours);
    auth.initialize(cfg.demo.enabled)
        .await
        .map_err(|e| StartupError::new(exit::DATABASE, e.to_string()))?;

    // 6. Jeton de contrôle local run/control.token (aléatoire 256 bits)
    let control_token = generate_control_token();
    let token_file = layout.control_token_file();
    if let Err(e) = write_control_token(&token_file, &control_token) {
        warn!(
            "Écriture du jeton de contrôle local « {} » impossible : {e}",
            token_file.display()
        );
    }

    let web_dir = layout.web_dir(cfg);
    if !web_dir.join("index.html").is_file() {
        warn!(
            "IHM web introuvable dans « {} » : renseignez paths.web_dir dans la configuration",
            web_dir.display()
        );
    }

    let started_at = Utc::now();
    let runtime = RuntimeInfo {
        instance_name: cfg.instance.name.trim().to_string(),
        config_fingerprint: eff.file_fingerprint.clone(),
        config_file: layout.config_file.display().to_string(),
        data_dir: layout.data_dir.display().to_string(),
        log_dir: layout.log_dir.display().to_string(),
        web_dir: web_dir.display().to_string(),
        run_mode: mode.as_str().to_string(),
        mode: mode.as_str().to_string(),
        pid: std::process::id(),
        started_at: started_at.to_rfc3339(),
        full_version: apex_config::version::full_version(),
    };

    let files_dir = layout.files_dir(cfg);
    std::fs::create_dir_all(&files_dir).map_err(|e| {
        StartupError::new(
            exit::DATABASE,
            format!("Dossier des documents inaccessible : {e}"),
        )
    })?;
    let secret_file = layout.data_dir.join("secret.key");
    if !secret_file.exists() {
        use rand::RngCore;
        let mut key = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut key);
        if let Err(e) = write_private_file(&secret_file, &key) {
            let _ = std::fs::remove_file(&secret_file);
            return Err(StartupError::new(
                exit::DATABASE,
                format!("Création de la clé secrète impossible : {e}"),
            ));
        }
    }
    let key: [u8; 32] = std::fs::read(&secret_file)
        .map_err(|e| {
            StartupError::new(
                exit::DATABASE,
                format!("Lecture de la clé secrète impossible : {e}"),
            )
        })?
        .try_into()
        .map_err(|_| {
            StartupError::new(exit::DATABASE, "Clé secrète invalide (32 octets attendus)")
        })?;
    let files = apex_core::documents::storage::FileStore::new(files_dir, key);
    let exploitation_task = crate::exploitation::scheduler(
        pool.clone(),
        files.clone(),
        crate::exploitation::context(&eff),
        layout.log_dir.clone(),
        eff.config.logging.retention_files,
    );
    let shutdown_notify = Arc::new(tokio::sync::Notify::new());
    let state = AppState {
        integration_rates: Default::default(),
        tls_mode: cfg.server.tls.mode.clone(),
        trusted_proxies: cfg.server.trusted_proxies.clone(),
        hsts_max_age: cfg.server.hsts_max_age,
        terrain_lock: Arc::new(tokio::sync::Mutex::new(())),
        widget_cache: Default::default(),
        files: files.clone(),
        pool: pool.clone(),
        auth,
        secure_cookie: cfg.security.secure_cookie,
        runtime: Arc::new(runtime),
        control_token,
        shutdown_notify: Arc::clone(&shutdown_notify),
        started_at,
        files_max_size_mb: cfg.files.max_size_mb,
    };
    let app = http::router(state, &web_dir);

    info!("IHM web : http://{}:{}", display_host(ip), cfg.server.port);
    if !ip.is_loopback() {
        warn!("Le serveur est accessible depuis le réseau ({addr}) en HTTP non chiffré");
    }

    // Tâche de fond de maintenance préventive (§2.4)
    let scheduler_pool = pool.clone();
    let scheduler_notify = Arc::clone(&shutdown_notify);
    let scheduler_secs = std::env::var("APEX_MAINT_TEST_SCHEDULER_SECONDS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(15 * 60);

    tokio::spawn(async move {
        apex_core::maintenance::process_auto_generation(&scheduler_pool).await;

        let mut interval =
            tokio::time::interval(tokio::time::Duration::from_secs(scheduler_secs.max(1)));
        interval.tick().await;

        loop {
            tokio::select! {
                _ = scheduler_notify.notified() => {
                    break;
                }
                _ = interval.tick() => {
                    apex_core::maintenance::process_auto_generation(&scheduler_pool).await;
                }
            }
        }
    });

    // File SMTP et récapitulatif ; la purge est quotidienne, sans bloquer l'API.
    let pilotage_pool = pool.clone();
    let pilotage_notify = Arc::clone(&shutdown_notify);
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(30));
        let purge_seconds = if cfg!(debug_assertions) {
            std::env::var("APEX_MAINT_TEST_AUDIT_PURGE_SECONDS")
                .ok()
                .and_then(|s| s.parse::<u64>().ok())
                .filter(|n| *n > 0 && *n <= 86400)
                .unwrap_or(86400)
        } else {
            86400
        };
        let mut purge_interval =
            tokio::time::interval(tokio::time::Duration::from_secs(purge_seconds));
        loop {
            tokio::select! {
                _ = pilotage_notify.notified() => break,
                _ = purge_interval.tick() => {
                    if let Err(e) = apex_core::pilotage::audit::purge(&pilotage_pool).await {
                        tracing::error!("Purge du journal d'audit : {e}");
                    }
                }
                _ = interval.tick() => {
                    if let Err(e) = apex_core::pilotage::notifications::digest(&pilotage_pool).await {
                        tracing::error!("Récapitulatif des échéances : {e}");
                    }
                    if let Err(e) = apex_core::pilotage::notifications::send_pending(&pilotage_pool, &files).await {
                        tracing::error!("Traitement de la file d'e-mails : {e}");
                    }
                }
            }
        }
    });

    // 7. Service HTTP et attente des signaux
    on_ready();
    let notify_clone = Arc::clone(&shutdown_notify);
    let shutdown = async move {
        tokio::select! {
            _ = notify_clone.notified() => {},
            _ = wait_os_signal() => {},
            _ = stop.wait_for(|s| *s) => {},
        }
        notify_clone.notify_waiters();
    };
    let redirect_task = redirect_listener.map(|listener| {
        let port = cfg.server.port;
        tokio::spawn(async move {
            let router = axum::Router::new().fallback(
                move |headers: axum::http::HeaderMap, uri: axum::http::Uri| async move {
                    let host = headers
                        .get("host")
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| v.parse::<axum::http::uri::Authority>().ok());
                    match host {
                        Some(host) if !host.as_str().contains('@') => {
                            axum::response::Redirect::permanent(&format!(
                                "https://{}:{port}{uri}",
                                host.host()
                            ))
                            .into_response()
                        }
                        _ => axum::http::StatusCode::BAD_REQUEST.into_response(),
                    }
                },
            );
            axum::serve(listener, router).await
        })
    });
    use axum::response::IntoResponse;
    let mut reload_task = None;
    let served = if let Some((tls_config, cert, key)) = tls {
        let reload_config = tls_config.clone();
        let tls_settings = cfg.server.tls.clone();
        let base = layout.data_dir.clone();
        let bind = cfg.server.bind.clone();
        reload_task = Some(tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(3600));
            interval.tick().await;
            loop {
                interval.tick().await;
                let result = (|| -> Result<_, String> {
                    if tls_settings.mode == "self_signed" {
                        crate::tls::self_signed(&tls_settings, &base, &bind)?;
                    }
                    crate::tls::load(&cert, &key)
                })();
                match result {
                    Ok(config) => reload_config.reload_from_config(config),
                    Err(e) => tracing::error!("{}", e),
                }
            }
        }));
        let handle = axum_server::Handle::new();
        let stopper = handle.clone();
        let shutdown_task = tokio::spawn(async move {
            shutdown.await;
            stopper.graceful_shutdown(Some(std::time::Duration::from_secs(30)));
        });
        let result = axum_server::from_tcp_rustls(
            listener
                .into_std()
                .map_err(|e| StartupError::new(exit::FAILURE, e.to_string()))?,
            tls_config,
        )
        .handle(handle)
        .serve(app.into_make_service_with_connect_info::<SocketAddr>())
        .await;
        shutdown_task.abort();
        result
    } else {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(shutdown)
        .await
    };
    if let Some(task) = reload_task {
        task.abort();
        let _ = task.await;
    }
    if let Some(task) = redirect_task {
        task.abort();
        let _ = task.await;
    }

    // 8. Arrêt propre : suppression du jeton et fermeture propre de la base avec checkpoint WAL
    exploitation_task.abort();
    let _ = exploitation_task.await;
    let _ = std::fs::remove_file(&token_file);
    apex_core::db::close(&pool).await;
    info!("{} arrêté proprement.", apex_config::version::PRODUCT_NAME);

    served.map_err(|e| StartupError::new(exit::FAILURE, format!("serveur web interrompu : {e}")))
}

fn generate_control_token() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    let mut s = String::with_capacity(64);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(s, "{:02x}", b);
    }
    s
}

fn display_host(ip: std::net::IpAddr) -> String {
    if ip.is_unspecified() || ip.is_loopback() {
        "localhost".to_string()
    } else {
        ip.to_string()
    }
}

/// Écrit le jeton de contrôle local avec des permissions restreintes.
fn write_control_token(path: &std::path::Path, token: &str) -> std::io::Result<()> {
    write_private_file(path, token.as_bytes())
}

/// Restaure la clé en préservant les autorisations du fichier existant.
pub(crate) fn restore_private_file(path: &std::path::Path, contents: &[u8]) -> std::io::Result<()> {
    // Conserver le fichier et ses ACL, notamment les droits du compte de service.
    use std::io::Write;
    match std::fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(path)
    {
        Ok(mut file) => file.write_all(contents),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => write_private_file(path, contents),
        Err(e) => Err(e),
    }
}

/// Même restriction d'accès pour le jeton local et la clé de chiffrement persistante.
pub(crate) fn write_private_file(path: &std::path::Path, contents: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        file.write_all(contents)?;
    }

    #[cfg(windows)]
    {
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;
        let path_str = path.display().to_string();
        let username = std::env::var("USERNAME").unwrap_or_default();
        let mut grant_args = vec![
            path_str,
            "/inheritance:r".to_string(),
            "/grant:r".to_string(),
            "*S-1-5-18:(F)".to_string(),     // NT AUTHORITY\SYSTEM
            "*S-1-5-32-544:(F)".to_string(), // Builtin\Administrators
        ];
        if !username.is_empty() {
            grant_args.push(format!("{}:(F)", username));
        }
        let result = std::process::Command::new("icacls")
            .args(&grant_args)
            .output()?;
        if !result.status.success() {
            return Err(std::io::Error::other(format!(
                "Restriction des permissions du fichier privé impossible : {}",
                String::from_utf8_lossy(&result.stderr).trim()
            )));
        }
        use std::io::Write;
        file.write_all(contents)?;
    }
    Ok(())
}

/// Attend Ctrl+C (et SIGTERM sous Linux, fermeture de console sous Windows).
async fn wait_os_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut term = match signal(SignalKind::terminate()) {
            Ok(s) => s,
            Err(_) => {
                let _ = tokio::signal::ctrl_c().await;
                return;
            }
        };
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(windows)]
    {
        use tokio::signal::windows::{ctrl_break, ctrl_close, ctrl_shutdown};
        match (ctrl_break(), ctrl_close(), ctrl_shutdown()) {
            (Ok(mut brk), Ok(mut close), Ok(mut shut)) => {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {}
                    _ = brk.recv() => {}
                    _ = close.recv() => {}
                    _ = shut.recv() => {}
                }
            }
            _ => {
                let _ = tokio::signal::ctrl_c().await;
            }
        }
    }
}

#[cfg(test)]
mod restore_tests {
    #[test]
    fn restauration_cle_conserve_les_autorisations() {
        let temp = tempfile::tempdir().unwrap();
        let key = temp.path().join("secret.key");
        super::write_private_file(&key, &[1; 32]).unwrap();
        #[cfg(windows)]
        let before = {
            // LocalService et un autre principal : aucune ACL existante ne doit disparaître.
            let grant = std::process::Command::new("icacls")
                .arg(&key)
                .args(["/grant", "*S-1-5-19:(R)", "*S-1-5-20:(R)"])
                .output()
                .unwrap();
            assert!(
                grant.status.success(),
                "{}",
                String::from_utf8_lossy(&grant.stderr)
            );
            let output = std::process::Command::new("icacls")
                .arg(&key)
                .output()
                .unwrap();
            assert!(output.status.success());
            output.stdout
        };
        #[cfg(unix)]
        let before = {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o640)).unwrap();
            std::fs::metadata(&key).unwrap().permissions().mode()
        };
        super::restore_private_file(&key, &[2; 32]).unwrap();
        assert_eq!(std::fs::read(&key).unwrap(), [2; 32]);
        #[cfg(windows)]
        {
            let output = std::process::Command::new("icacls")
                .arg(&key)
                .output()
                .unwrap();
            assert!(output.status.success());
            assert_eq!(output.stdout, before);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&key).unwrap().permissions().mode(),
                before
            );
        }
    }
}
