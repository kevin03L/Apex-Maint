//! Lecture, migration, écriture atomique, sauvegarde et restauration du fichier de configuration.
//!
//! Règles d'écriture :
//! - la configuration est **validée** avant d'être écrite (aucune erreur bloquante) ;
//! - un **verrou** (`apex-maint.toml.lock`) empêche deux écritures simultanées ;
//! - si le fichier a changé sur disque depuis sa lecture, l'écriture est **refusée** (conflit) ;
//! - le fichier existant est **sauvegardé** (horodaté) avant d'être remplacé ;
//! - l'écriture est **atomique** : fichier temporaire, synchronisation disque, puis renommage.

use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use thiserror::Error;

use crate::{
    model::{fnv1a64, has_errors, AppConfig, Issue, CONFIG_VERSION},
    paths::{Layout, LayoutKind},
};

/// Nombre de sauvegardes conservées.
pub const MAX_BACKUPS: usize = 20;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("lecture de « {path} » impossible : {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("écriture de « {path} » impossible : {source}")]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("syntaxe TOML invalide dans « {path} » : {message}")]
    Syntax { path: PathBuf, message: String },
    #[error("paramètre invalide dans « {path} » : {message}")]
    Schema { path: PathBuf, message: String },
    #[error("format de configuration {found} plus récent que celui pris en charge ({supported}) : mettez à jour le logiciel")]
    TooNew { found: u32, supported: u32 },
    #[error("configuration refusée : {}", .0.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(" ; "))]
    Invalid(Vec<Issue>),
    #[error("le fichier de configuration est en cours de modification par un autre programme")]
    Locked,
    #[error("le fichier de configuration a été modifié par ailleurs depuis sa lecture : rechargez-le avant d'enregistrer")]
    Conflict,
    #[error("logiciel non installé : le dossier « {dir} » est créé par l'installateur (pour une instance portable, utilisez --portable ou --config <fichier>)")]
    NotInstalled { dir: PathBuf },
}

/// Résultat d'une lecture.
#[derive(Debug, Clone)]
pub struct Loaded {
    pub config: AppConfig,
    /// Version d'origine si une migration a été appliquée en mémoire.
    pub migrated_from: Option<u32>,
    /// Résultat de la validation (erreurs et avertissements).
    pub issues: Vec<Issue>,
    /// Empreinte du contenu brut du fichier (détection des modifications concurrentes).
    /// `None` si le fichier n'existait pas.
    pub file_hash: Option<u64>,
}

/// Lit et migre le fichier de configuration (sans le réécrire).
pub fn load(path: &Path) -> Result<Loaded, ConfigError> {
    let raw = fs::read_to_string(path).map_err(|source| ConfigError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    let (config, migrated_from) = parse(&raw, path)?;
    let issues = config.validate();
    Ok(Loaded {
        config,
        migrated_from,
        issues,
        file_hash: Some(fnv1a64(raw.as_bytes())),
    })
}

/// Comme [`load`], mais renvoie `default` si le fichier n'existe pas.
pub fn load_or(path: &Path, default: AppConfig) -> Result<Loaded, ConfigError> {
    if !path.exists() {
        let issues = default.validate();
        return Ok(Loaded {
            config: default,
            migrated_from: None,
            issues,
            file_hash: None,
        });
    }
    load(path)
}

fn parse(raw: &str, path: &Path) -> Result<(AppConfig, Option<u32>), ConfigError> {
    let value: toml::Value = toml::from_str(raw).map_err(|e| ConfigError::Syntax {
        path: path.to_path_buf(),
        message: e.message().to_string(),
    })?;
    let (value, migrated_from) = crate::migrate::migrate(value).map_err(|e| match e {
        ConfigError::Schema { path: p, message } if p.as_os_str().is_empty() => {
            ConfigError::Schema {
                path: path.to_path_buf(),
                message,
            }
        }
        other => other,
    })?;
    let config: AppConfig = value
        .try_into()
        .map_err(|e: toml::de::Error| ConfigError::Schema {
            path: path.to_path_buf(),
            message: describe_schema_error(raw, e.message()),
        })?;
    Ok((config, migrated_from))
}

/// Message de schéma lisible : traduction des messages courants de serde et, quand c'est
/// possible, numéro de la ligne concernée (l'erreur levée après migration ne la donne pas).
fn describe_schema_error(raw: &str, message: &str) -> String {
    // Texte entre accents graves : `x`
    fn quoted(s: &str) -> Vec<&str> {
        s.split('`').skip(1).step_by(2).collect()
    }
    // Première ligne « clé = … » (ou contenant le motif) hors commentaire
    fn line_of(raw: &str, pred: impl Fn(&str) -> bool) -> Option<usize> {
        raw.lines()
            .position(|l| {
                let l = l.trim_start();
                !l.starts_with('#') && pred(l)
            })
            .map(|i| i + 1)
    }
    let at = |line: Option<usize>| line.map(|n| format!(" (ligne {n})")).unwrap_or_default();

    if let Some(rest) = message.strip_prefix("unknown field ") {
        let names = quoted(rest);
        if let Some((field, expected)) = names.split_first() {
            let line = line_of(raw, |l| {
                l.split('=').next().is_some_and(|k| k.trim() == *field)
            });
            return format!(
                "paramètre inconnu « {field} »{} — paramètres possibles dans cette section : {}",
                at(line),
                expected.join(", ")
            );
        }
    }

    if let Some(rest) = message.strip_prefix("missing field ") {
        let names = quoted(rest);
        if let Some(field) = names.first() {
            return format!("paramètre obligatoire manquant « {field} »");
        }
    }

    if let Some(rest) = message.strip_prefix("invalid value: integer `") {
        if let Some((val, rest)) = rest.split_once('`') {
            let line = line_of(raw, |l| l.contains(val));
            if rest.contains("expected u16") {
                return format!(
                    "valeur {val} invalide{} : un entier de 0 à 65535 attendu",
                    at(line)
                );
            }
            if rest.contains("expected u32") {
                return format!(
                    "valeur {val} invalide{} : un entier positif attendu",
                    at(line)
                );
            }
        }
    }

    if let Some(rest) = message.strip_prefix("invalid type: string ") {
        if let Some((val, rest)) = rest.split_once(',') {
            let line = line_of(raw, |l| l.contains(val.trim()));
            if rest.contains("expected u16") {
                return format!(
                    "type invalide : texte {val}{} (un entier de 0 à 65535 attendu)",
                    at(line)
                );
            }
            if rest.contains("expected u32") {
                return format!(
                    "type invalide : texte {val}{} (un entier positif attendu)",
                    at(line)
                );
            }
            if rest.contains("expected a boolean") {
                return format!(
                    "type invalide : texte {val}{} (true ou false attendu)",
                    at(line)
                );
            }
        }
    }

    if let Some(rest) = message.strip_prefix("invalid type: integer `") {
        if let Some((val, rest)) = rest.split_once('`') {
            let line = line_of(raw, |l| l.contains(val));
            if rest.contains("expected a string") {
                return format!(
                    "type invalide : nombre {val}{} (texte entre guillemets attendu)",
                    at(line)
                );
            }
        }
    }

    message.to_string()
}

/// Résultat d'un enregistrement.
#[derive(Debug, Clone)]
pub struct SaveReport {
    /// Chemin du fichier de sauvegarde de l'ancienne configuration, si le fichier existait déjà.
    pub backup: Option<PathBuf>,
    /// Empreinte du nouveau contenu écrit sur disque.
    pub file_hash: u64,
}

/// Écrit la configuration de façon atomique après validation.
///
/// Si `expected_hash` est fourni, l'écriture est refusée si le fichier sur disque ne correspond
/// plus à cette empreinte (conflit d'édition).
pub fn save(
    layout: &Layout,
    config: &AppConfig,
    expected_hash: Option<u64>,
) -> Result<SaveReport, ConfigError> {
    let issues = config.validate();
    if has_errors(&issues) {
        return Err(ConfigError::Invalid(issues));
    }

    let path = &layout.config_file;
    let dir = match path.parent() {
        Some(d) if !d.as_os_str().is_empty() => d,
        _ => Path::new("."),
    };

    if layout.kind == LayoutKind::System && !dir.is_dir() {
        return Err(ConfigError::NotInstalled {
            dir: dir.to_path_buf(),
        });
    }

    fs::create_dir_all(dir).map_err(|source| ConfigError::Write {
        path: dir.to_path_buf(),
        source,
    })?;

    let lock_path = layout.config_lock_file();
    let lock = open_lock(&lock_path).map_err(|source| ConfigError::Write {
        path: lock_path.clone(),
        source,
    })?;
    lock.try_lock().map_err(|_| ConfigError::Locked)?;

    let current_raw = match fs::read(path) {
        Ok(bytes) => Some(bytes),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(source) => {
            return Err(ConfigError::Read {
                path: path.clone(),
                source,
            })
        }
    };
    let current_hash = current_raw.as_deref().map(fnv1a64);
    if current_hash != expected_hash {
        return Err(ConfigError::Conflict);
    }

    let backup = match &current_raw {
        Some(raw) => Some(write_backup(&layout.backup_dir, raw)?),
        None => None,
    };

    let content = render(config);
    let (tmp, mut file) = create_temp(dir, path).map_err(|source| ConfigError::Write {
        path: dir.to_path_buf(),
        source,
    })?;
    let write_tmp = |f: &mut File| -> std::io::Result<()> {
        f.write_all(content.as_bytes())?;
        // Le nouveau fichier reprend le mode et le groupe de l'ancien (Linux : 0660 root:apex-maint)
        #[cfg(unix)]
        if let Ok(meta) = fs::metadata(path) {
            use std::os::unix::fs::MetadataExt;
            let _ = std::os::unix::fs::chown(&tmp, None, Some(meta.gid()));
            let _ = fs::set_permissions(&tmp, meta.permissions());
        }
        f.sync_all()
    };
    if let Err(source) = write_tmp(&mut file) {
        drop(file);
        let _ = fs::remove_file(&tmp);
        return Err(ConfigError::Write {
            path: tmp.clone(),
            source,
        });
    }
    drop(file);
    fs::rename(&tmp, path).map_err(|source| {
        let _ = fs::remove_file(&tmp);
        ConfigError::Write {
            path: path.clone(),
            source,
        }
    })?;
    prune_backups(&layout.backup_dir, backup.as_deref());
    drop(lock);

    Ok(SaveReport {
        backup,
        file_hash: fnv1a64(content.as_bytes()),
    })
}

fn open_lock(lock_path: &Path) -> std::io::Result<File> {
    if let Ok(f) = OpenOptions::new().read(true).open(lock_path) {
        return Ok(f);
    }
    let mut opts = OpenOptions::new();
    opts.create(true).truncate(false).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o660);
    }
    opts.open(lock_path)
}

fn create_temp(dir: &Path, target: &Path) -> std::io::Result<(PathBuf, File)> {
    let base = target
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("config");
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut last_err = None;
    for attempt in 0..16u32 {
        let seed = format!("{}-{}-{}", std::process::id(), nanos, attempt);
        let tmp = dir.join(format!(".{base}.tmp-{:016x}", fnv1a64(seed.as_bytes())));
        let mut opts = OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o660);
        }
        match opts.open(&tmp) {
            Ok(f) => return Ok((tmp, f)),
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or_else(|| std::io::Error::other("fichier temporaire impossible")))
}

fn write_backup(backup_dir: &Path, raw: &[u8]) -> Result<PathBuf, ConfigError> {
    fs::create_dir_all(backup_dir).map_err(|source| ConfigError::Write {
        path: backup_dir.to_path_buf(),
        source,
    })?;
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S%.3f");
    let target = backup_dir.join(format!("apex-maint-{}.toml", stamp));
    fs::write(&target, raw).map_err(|source| ConfigError::Write {
        path: target.clone(),
        source,
    })?;
    Ok(target)
}

pub fn list_backups(backup_dir: &Path) -> Vec<PathBuf> {
    let mut list: Vec<PathBuf> = fs::read_dir(backup_dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with("apex-maint-") && n.ends_with(".toml"))
                })
                .collect()
        })
        .unwrap_or_default();
    list.sort();
    list.reverse();
    list
}

fn prune_backups(backup_dir: &Path, keep: Option<&Path>) {
    let candidates: Vec<PathBuf> = list_backups(backup_dir)
        .into_iter()
        .filter(|p| Some(p.as_path()) != keep)
        .collect();
    let room = MAX_BACKUPS.saturating_sub(usize::from(keep.is_some()));
    for old in candidates.into_iter().skip(room) {
        let _ = fs::remove_file(old);
    }
}

pub fn restore_backup(
    layout: &Layout,
    backup: &Path,
    expected_hash: Option<u64>,
) -> Result<SaveReport, ConfigError> {
    let loaded = load(backup)?;
    save(layout, &loaded.config, expected_hash)
}

pub fn file_hash(path: &Path) -> Option<u64> {
    fs::read(path).ok().map(|r| fnv1a64(&r))
}

fn s(v: &str) -> String {
    toml::Value::String(v.to_string()).to_string()
}

/// Rendu TOML commenté, lisible et modifiable à la main par un administrateur.
pub fn render(c: &AppConfig) -> String {
    format!(
        r#"# Configuration d'Apex Maint — généré par {product} {version}
# Toute modification de ce fichier nécessite un redémarrage du moteur.
# Les réglages métier modifiables à chaud se gèrent dans l'IHM web.

# Version du format de ce fichier (ne pas modifier : gérée par les migrations).
config_version = {config_version}

[instance]
# Nom affiché de l'instance (IHM, journaux, utilitaire desktop).
name = {name}

[server]
# Adresse d'écoute : "127.0.0.1" = poste local uniquement ; "0.0.0.0" = tout le réseau.
# Attention : le serveur parle HTTP non chiffré. Pour un accès réseau, utilisez un proxy HTTPS.
bind = {bind}
# Port HTTP de l'IHM web et de l'API (défaut 3100).
port = {port}

[paths]
# Répertoires personnalisés (chemins absolus). Vide = emplacement standard du système.
data_dir = {data_dir}
log_dir = {log_dir}
web_dir = {web_dir}
# Répertoire des fichiers joints. Vide = sous-dossier files/ du répertoire des données.
files_dir = {files_dir}

[database]
# Fichier SQLite (données GMAO, audit, comptes), relatif à data_dir ou absolu.
file = {db_file}

[security]
# Expiration d'une session inactive, en heures (1 à 720).
session_ttl_hours = {session_ttl}
# Cookie sécurisé (Secure) derrière un proxy HTTPS.
secure_cookie = {secure_cookie}

[files]
# Taille maximale d'un fichier téléversé, en mégaoctets (1 à 1024).
max_size_mb = {max_size_mb}

[logging]
# Niveau : "error", "warn", "info", "debug" ou "trace".
level = {level}
# Journaux dans des fichiers (rotation quotidienne) en plus de la sortie standard.
file_enabled = {file_enabled}
# Nombre de fichiers journaliers conservés (1 à 365).
retention_files = {retention_files}

[demo]
# Jeu de données et comptes de démonstration.
enabled = {demo_enabled}
"#,
        product = crate::version::PRODUCT_NAME,
        version = crate::version::VERSION,
        config_version = CONFIG_VERSION,
        name = s(&c.instance.name),
        bind = s(&c.server.bind),
        port = c.server.port,
        data_dir = s(&c.paths.data_dir),
        log_dir = s(&c.paths.log_dir),
        web_dir = s(&c.paths.web_dir),
        files_dir = s(&c.paths.files_dir),
        db_file = s(&c.database.file),
        session_ttl = c.security.session_ttl_hours,
        secure_cookie = c.security.secure_cookie,
        max_size_mb = c.files.max_size_mb,
        level = s(&c.logging.level),
        file_enabled = c.logging.file_enabled,
        retention_files = c.logging.retention_files,
        demo_enabled = c.demo.enabled,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Severity;

    #[test]
    fn schema_errors_are_explained_in_french_with_their_line() {
        let raw = "config_version = 1\n[server]\nport = 99999\n[demo]\ninconnu = 1\n";
        let msg = |m: &str| describe_schema_error(raw, m);
        assert_eq!(
            msg("invalid value: integer `99999`, expected u16"),
            "valeur 99999 invalide (ligne 3) : un entier de 0 à 65535 attendu"
        );
        let unknown = msg("unknown field `inconnu`, expected `enabled`");
        assert!(
            unknown.starts_with("paramètre inconnu « inconnu » (ligne 5)"),
            "{unknown}"
        );
        assert!(unknown.contains("enabled"), "{unknown}");
        assert_eq!(
            msg("invalid type: string \"abc\", expected u16"),
            "type invalide : texte \"abc\" (un entier de 0 à 65535 attendu)"
        );
        // Message inconnu : repris tel quel
        assert_eq!(msg("autre chose"), "autre chose");
    }

    fn temp_layout(name: &str) -> Layout {
        let base = std::env::temp_dir().join(format!("apex-store-{}-{}", name, std::process::id()));
        let _ = fs::remove_dir_all(&base);
        Layout::portable(&base)
    }

    #[test]
    fn render_roundtrip_is_lossless() {
        let mut cfg = AppConfig::portable_default();
        cfg.instance.name = "Site \"Nord\" — Ligne 1".into();
        cfg.server.port = 8443;
        cfg.paths.data_dir = r"D:\données\apex".into();
        cfg.paths.files_dir = r"D:\données\apex\files".into();
        cfg.files.max_size_mb = 100;
        let raw = render(&cfg);
        let (parsed, migrated) = parse(&raw, Path::new("test.toml")).unwrap();
        assert_eq!(parsed, cfg);
        assert!(migrated.is_none());
    }

    #[test]
    fn save_creates_backup_and_detects_conflicts() {
        let layout = temp_layout("save");
        let cfg = AppConfig::default();
        let first = save(&layout, &cfg, None).unwrap();
        assert!(first.backup.is_none(), "pas de sauvegarde à la création");

        let mut cfg2 = cfg.clone();
        cfg2.server.port = 3200;
        let second = save(&layout, &cfg2, Some(first.file_hash)).unwrap();
        assert!(second.backup.as_ref().unwrap().is_file());
        assert_eq!(load(&layout.config_file).unwrap().config.server.port, 3200);

        // Écriture basée sur une lecture périmée → conflit, fichier inchangé
        let mut cfg3 = cfg.clone();
        cfg3.server.port = 3300;
        assert!(matches!(
            save(&layout, &cfg3, Some(first.file_hash)),
            Err(ConfigError::Conflict)
        ));
        assert_eq!(load(&layout.config_file).unwrap().config.server.port, 3200);
        let _ = fs::remove_dir_all(layout.config_file.parent().unwrap());
    }

    #[test]
    fn invalid_config_is_never_written() {
        let layout = temp_layout("invalid");
        let mut cfg = AppConfig::default();
        cfg.server.bind = "pas-une-ip".into();
        assert!(matches!(
            save(&layout, &cfg, None),
            Err(ConfigError::Invalid(_))
        ));
        assert!(!layout.config_file.exists());
    }

    #[test]
    fn restore_previous_backup() {
        let layout = temp_layout("restore");
        let a = save(&layout, &AppConfig::default(), None).unwrap();
        let mut cfg = AppConfig::default();
        cfg.instance.name = "Modifiée".into();
        let b = save(&layout, &cfg, Some(a.file_hash)).unwrap();
        let backups = list_backups(&layout.backup_dir);
        assert_eq!(backups.len(), 1);
        restore_backup(&layout, &backups[0], Some(b.file_hash)).unwrap();
        assert_eq!(
            load(&layout.config_file).unwrap().config.instance.name,
            "Apex Maint"
        );
        assert_eq!(
            list_backups(&layout.backup_dir).len(),
            2,
            "la version remplacée est sauvegardée"
        );
        let _ = fs::remove_dir_all(layout.config_file.parent().unwrap());
    }

    #[test]
    fn non_utf8_file_is_backed_up_before_replacement() {
        let layout = temp_layout("latin1");
        fs::create_dir_all(layout.config_file.parent().unwrap()).unwrap();
        // « Bâtiment » en Latin-1 (0xE2), port 4321 : fichier existant mais illisible en UTF-8
        let original: &[u8] =
            b"config_version = 1\n[instance]\nname = \"B\xe2timent\"\n[server]\nport = 4321\n";
        fs::write(&layout.config_file, original).unwrap();
        assert!(load(&layout.config_file).is_err());
        let hash = file_hash(&layout.config_file);
        assert!(hash.is_some(), "un fichier non UTF-8 existe bel et bien");
        assert!(matches!(
            save(&layout, &AppConfig::default(), None),
            Err(ConfigError::Conflict)
        ));
        let report = save(&layout, &AppConfig::default(), hash).unwrap();
        assert_eq!(fs::read(report.backup.unwrap()).unwrap(), original);
        let _ = fs::remove_dir_all(layout.config_file.parent().unwrap());
    }

    #[test]
    fn prune_never_removes_the_new_backup() {
        let layout = temp_layout("prune");
        fs::create_dir_all(&layout.backup_dir).unwrap();
        for i in 0..MAX_BACKUPS {
            fs::write(
                layout
                    .backup_dir
                    .join(format!("apex-maint-2099{:04}-000000.000.toml", i + 101)),
                "x",
            )
            .unwrap();
        }
        let first = save(&layout, &AppConfig::default(), None).unwrap();
        let mut cfg = AppConfig::default();
        cfg.server.port = 3200;
        let report = save(&layout, &cfg, Some(first.file_hash)).unwrap();
        assert!(
            report.backup.unwrap().is_file(),
            "la nouvelle sauvegarde est conservée"
        );
        assert_eq!(list_backups(&layout.backup_dir).len(), MAX_BACKUPS);
        let _ = fs::remove_dir_all(layout.config_file.parent().unwrap());
    }

    #[test]
    fn system_layout_is_never_created_by_save() {
        let base = std::env::temp_dir().join(format!("apex-store-sys-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let mut layout = Layout::portable(&base.join("config"));
        layout.kind = LayoutKind::System;
        assert!(matches!(
            save(&layout, &AppConfig::default(), None),
            Err(ConfigError::NotInstalled { .. })
        ));
        assert!(!base.exists(), "aucun dossier créé");
    }

    #[test]
    fn readable_errors_for_bad_files() {
        let layout = temp_layout("bad");
        fs::create_dir_all(layout.config_file.parent().unwrap()).unwrap();
        fs::write(
            &layout.config_file,
            "config_version = 1\n[server]\nport = \"trois mille\"\n",
        )
        .unwrap();
        let err = load(&layout.config_file).unwrap_err().to_string();
        assert!(err.contains("paramètre invalide"), "{err}");
        fs::write(
            &layout.config_file,
            "config_version = 1\n[serveur]\nport = 3100\n",
        )
        .unwrap();
        let err = load(&layout.config_file).unwrap_err().to_string();
        assert!(
            err.contains("serveur"),
            "une section inconnue est signalée : {err}"
        );
        fs::write(
            &layout.config_file,
            "config_version = 1\n[server]\nbind = \"0.0.0.0\"\n",
        )
        .unwrap();
        let loaded = load(&layout.config_file).unwrap();
        assert!(loaded
            .issues
            .iter()
            .any(|i| i.severity == Severity::Warning));
        let _ = fs::remove_dir_all(layout.config_file.parent().unwrap());
    }
}
