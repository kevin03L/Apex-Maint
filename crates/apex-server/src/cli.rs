//! Ligne de commande de `apex-maint` (analyse manuelle, sans dépendance supplémentaire).

use std::path::PathBuf;

pub const USAGE: &str = "\
Apex Maint — GMAO, moteur et serveur web

UTILISATION :
  apex-maint [run] [--config <fichier>]
        Démarre le moteur au premier plan (Ctrl+C pour un arrêt propre).

  apex-maint config check [--config <fichier>]
        Valide la configuration et vérifie la disponibilité du port.
  apex-maint config show [--config <fichier>]
        Affiche la configuration effective et les emplacements utilisés.
  apex-maint config init [--portable] [--demo] [--force] [--config <fichier>]
                         [--name <nom>] [--bind <ip>] [--port <n>]
        Crée un fichier de configuration (refusé s'il existe, sauf --force avec sauvegarde).
  apex-maint config migrate [--config <fichier>]
        Réécrit la configuration au format courant (valeurs conservées, sauvegarde préalable).

  apex-maint version      Affiche la version.
  apex-maint service install|uninstall|start|stop|restart|status|enable|disable
        Administration du service (Windows : install accepte --account <compte>).
  apex-maint backup [--output <dossier>] [--include-files folder_only|all|none]
                    [--with-secret-key] [--password-file <fichier>]
        Sauvegarde cohérente à chaud.
  apex-maint restore <archive> [--password-file <fichier>] [--force]
        Restaure une archive, moteur arrêté ; confirmation et sauvegarde préalable par défaut.
  apex-maint purge [--dry-run] [--older-than <années>]
        Purge les données supprimées anciennes (2 ans minimum).
  apex-maint files verify [--storage <id>] [--sample <n>]
  apex-maint files migrate --from <id> --to <id> [--dry-run]
        Vérifie ou migre les fichiers joints, sans supprimer les sources.
  apex-maint help         Affiche cette aide.

CODES DE SORTIE :
  0 succès · 1 erreur · 2 usage · 3 configuration invalide · 4 instance déjà active
  5 adresse ou port indisponible · 6 base de données inaccessible
";

/// Codes de sortie documentés (exploités par l'utilitaire desktop et les scripts).
pub use apex_config::exit;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct InitOptions {
    pub portable: bool,
    pub demo: bool,
    pub force: bool,
    pub name: Option<String>,
    pub bind: Option<String>,
    pub port: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Exploitation(Vec<String>),
    Service(String, Option<String>),
    Run,
    ConfigCheck,
    ConfigShow,
    ConfigInit(InitOptions),
    ConfigMigrate,
    Version,
    Help,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cli {
    pub command: Command,
    pub config: Option<PathBuf>,
}

pub fn parse(args: &[String]) -> Result<Cli, String> {
    if args
        .first()
        .is_some_and(|s| ["backup", "restore", "purge", "files", "service"].contains(&s.as_str()))
    {
        let mut words = Vec::new();
        let mut config = None;
        let mut it = args.iter();
        while let Some(word) = it.next() {
            if word == "--config" || word == "-c" {
                config = Some(PathBuf::from(
                    it.next().ok_or("L'option --config attend un fichier")?,
                ));
            } else {
                words.push(word.clone());
            }
        }
        let command = if words[0] == "service" {
            let account = if words.len() == 4 && words[1] == "install" && words[2] == "--account" {
                Some(words[3].clone())
            } else {
                None
            };
            if (words.len() != 2 && account.is_none())
                || ![
                    "install",
                    "uninstall",
                    "start",
                    "stop",
                    "restart",
                    "status",
                    "enable",
                    "disable",
                    "permissions",
                    "run",
                ]
                .contains(&words[1].as_str())
            {
                return Err("Action de service invalide".into());
            }
            Command::Service(words[1].clone(), account)
        } else {
            Command::Exploitation(words)
        };
        return Ok(Cli { command, config });
    }
    let mut config: Option<PathBuf> = None;
    let mut positional: Vec<String> = Vec::new();
    let mut init = InitOptions::default();
    let mut force = false;

    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = |flag: &str| -> Result<String, String> {
            it.next()
                .cloned()
                .ok_or_else(|| format!("l'option {flag} attend une valeur"))
        };
        match arg.as_str() {
            "--config" | "-c" => config = Some(PathBuf::from(value("--config")?)),
            "--name" => init.name = Some(value("--name")?),
            "--bind" => init.bind = Some(value("--bind")?),
            "--port" => {
                let raw = value("--port")?;
                init.port = Some(
                    raw.parse::<u16>()
                        .map_err(|_| format!("port invalide « {raw} »"))?,
                )
            }
            "--portable" => init.portable = true,
            "--demo" => init.demo = true,
            "--force" => force = true,
            "-h" | "--help" => positional.push("help".into()),
            "-V" | "--version" => positional.push("version".into()),
            other if other.starts_with('-') => return Err(format!("option inconnue « {other} »")),
            other => positional.push(other.to_string()),
        }
    }
    init.force = force;

    let words: Vec<&str> = positional.iter().map(String::as_str).collect();
    let command = match words.as_slice() {
        [] | ["run"] => Command::Run,
        ["config", "check"] => Command::ConfigCheck,
        ["config", "show"] => Command::ConfigShow,
        ["config", "init"] => Command::ConfigInit(init),
        ["config", "migrate"] => Command::ConfigMigrate,
        ["version"] => Command::Version,
        ["help"] => Command::Help,
        _ => return Err(format!("commande inconnue « {} »", words.join(" "))),
    };
    Ok(Cli { command, config })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Result<Cli, String> {
        parse(&s.split_whitespace().map(String::from).collect::<Vec<_>>())
    }

    #[test]
    fn default_is_run() {
        assert_eq!(p("").unwrap().command, Command::Run);
        assert_eq!(
            p("run --config /etc/x.toml").unwrap().config,
            Some(PathBuf::from("/etc/x.toml"))
        );
    }

    #[test]
    fn config_init_options() {
        let cli = p("config init --portable --port 3100 --bind 0.0.0.0 --demo --force").unwrap();
        let Command::ConfigInit(o) = cli.command else {
            panic!()
        };
        assert!(o.portable && o.demo && o.force);
        assert_eq!(o.port, Some(3100));
        assert_eq!(o.bind.as_deref(), Some("0.0.0.0"));
    }

    #[test]
    fn errors_are_explicit() {
        assert!(p("config init --port abc")
            .unwrap_err()
            .contains("port invalide"));
        assert!(p("--inconnue").unwrap_err().contains("option inconnue"));
        assert!(p("demarrer").unwrap_err().contains("commande inconnue"));
    }

    #[test]
    fn version_and_help() {
        assert_eq!(p("--version").unwrap().command, Command::Version);
        assert_eq!(p("-h").unwrap().command, Command::Help);
    }
}
