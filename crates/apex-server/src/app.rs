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

    let shutdown_notify = Arc::new(tokio::sync::Notify::new());
    let state = AppState {
        runtime: Arc::new(runtime),
        control_token,
        shutdown_notify: Arc::clone(&shutdown_notify),
        started_at,
    };
    let app = http::router(state, &web_dir);

    info!("IHM web : http://{}:{}", display_host(ip), cfg.server.port);
    if !ip.is_loopback() {
        warn!("Le serveur est accessible depuis le réseau ({addr}) en HTTP non chiffré");
    }

    // 7. Service HTTP et attente des signaux
    on_ready();
    let notify_clone = Arc::clone(&shutdown_notify);
    let served = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        tokio::select! {
            _ = notify_clone.notified() => info!("Arrêt demandé depuis l'utilitaire local ou l'API"),
            _ = wait_os_signal() => info!("Signal d'arrêt du système reçu (SIGINT / SIGTERM)"),
            _ = stop.wait_for(|s| *s) => info!("Arrêt demandé par le gestionnaire de services"),
        }
        notify_clone.notify_waiters();
        info!("Arrêt propre en cours : fermeture du serveur web...");
    })
    .await;

    // 8. Arrêt propre : suppression du jeton et fermeture propre de la base avec checkpoint WAL
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
            .mode(0o640)
            .open(path)?;
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o640))?;
        file.write_all(token.as_bytes())?;
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
            return Err(std::io::Error::other(
                "Restriction des permissions du jeton impossible",
            ));
        }
        use std::io::Write;
        file.write_all(token.as_bytes())?;
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
