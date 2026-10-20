//! Opérations lentes déléguées au binaire, avec résultat transmis au fil graphique.
use eframe::egui;
use std::{
    path::{Path, PathBuf},
    sync::mpsc,
};

pub struct Operations {
    tx: mpsc::Sender<String>,
    rx: mpsc::Receiver<String>,
    busy: bool,
    report: String,
    archive: String,
    password: String,
    backup_dir: Option<Result<PathBuf, String>>,
    backup_dir_rx: Option<mpsc::Receiver<Result<PathBuf, String>>>,
    from: String,
    to: String,
    confirm: bool,
    include_files: String,
    secret: bool,
    settings: String,
    editing_settings: bool,
    account: String,
}
impl Default for Operations {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            tx,
            rx,
            busy: false,
            report: String::new(),
            archive: String::new(),
            password: String::new(),
            backup_dir: None,
            backup_dir_rx: None,
            from: String::new(),
            to: String::new(),
            confirm: false,
            include_files: "folder_only".into(),
            secret: false,
            settings: String::new(),
            editing_settings: false,
            account: String::new(),
        }
    }
}
impl Operations {
    pub fn service(&mut self, ui: &mut egui::Ui, config: &Path) {
        self.poll();
        ui.heading("Service système");
        for (label, action) in [
            ("Installer", "install"),
            ("Désinstaller", "uninstall"),
            ("Démarrer", "start"),
            ("Arrêter", "stop"),
            ("Redémarrer", "restart"),
        ] {
            if ui
                .add_enabled(!self.busy, egui::Button::new(label))
                .clicked()
            {
                self.run(
                    ui,
                    config,
                    vec!["service".into(), action.into()],
                    action == "uninstall",
                );
            }
        }
        ui.checkbox(
            &mut self.confirm,
            "Je confirme la désinstallation du service",
        );
        ui.label("Compte Windows dédié (vide : LocalService à la création, compte conservé à la mise à jour)");
        ui.text_edit_singleline(&mut self.account);
        if cfg!(windows)
            && ui
                .button("Choisir le compte dans un terminal sécurisé")
                .clicked()
        {
            self.report = crate::control::service_account_terminal(config, &self.account)
                .unwrap_or_else(|e| e);
        }
        ui.label("Un partage UNC exige un compte autorisé sur le partage. Lancez l'utilitaire en administrateur pour installer le service.");
        self.report(ui);
    }
    fn run(&mut self, ui: &egui::Ui, config: &Path, args: Vec<String>, confirmation: bool) {
        if confirmation && !self.confirm {
            self.report = "Cochez la confirmation avant cette opération.".into();
            return;
        }
        if self.busy {
            return;
        }
        self.busy = true;
        self.confirm = false;
        let tx = self.tx.clone();
        let config = config.to_path_buf();
        let ctx = ui.ctx().clone();
        let password =
            if args.first().is_some_and(|arg| arg == "restore") && !self.password.is_empty() {
                Some(std::mem::take(&mut self.password))
            } else {
                None
            };
        std::thread::spawn(move || {
            let output = crate::control::engine_command(&config, &args, password.as_deref());
            let _ = tx.send(output.unwrap_or_else(|e| e));
            ctx.request_repaint();
        });
    }
    fn poll(&mut self) {
        if let Ok(report) = self.rx.try_recv() {
            self.report = report;
            self.busy = false;
        }
    }
    pub fn maintenance(&mut self, ui: &mut egui::Ui, config: &Path) {
        self.poll();
        ui.heading("Maintenance des données et fichiers");
        ui.label("La purge réelle effectue d'abord une sauvegarde. Les orphelins sont seulement signalés.");
        if ui
            .add_enabled(!self.busy, egui::Button::new("Aperçu de la purge"))
            .clicked()
        {
            self.run(ui, config, vec!["purge".into(), "--dry-run".into()], false);
        }
        ui.checkbox(&mut self.confirm, "Je confirme l'opération de maintenance");
        if ui
            .add_enabled(!self.busy, egui::Button::new("Exécuter la purge"))
            .clicked()
        {
            self.run(ui, config, vec!["purge".into()], true);
        }
        if ui
            .add_enabled(!self.busy, egui::Button::new("Vérifier les fichiers"))
            .clicked()
        {
            self.run(ui, config, vec!["files".into(), "verify".into()], false);
        }
        ui.label("Identifiant du stockage source");
        ui.text_edit_singleline(&mut self.from);
        ui.label("Identifiant du stockage cible");
        ui.text_edit_singleline(&mut self.to);
        for (label, dry) in [
            ("Aperçu de migration", true),
            ("Migrer les fichiers", false),
        ] {
            if ui
                .add_enabled(!self.busy, egui::Button::new(label))
                .clicked()
            {
                let mut args = vec![
                    "files".into(),
                    "migrate".into(),
                    "--from".into(),
                    self.from.clone(),
                    "--to".into(),
                    self.to.clone(),
                ];
                if dry {
                    args.push("--dry-run".into());
                }
                self.run(ui, config, args, !dry);
            }
        }
        self.report(ui);
    }
    pub fn backups(&mut self, ui: &mut egui::Ui, config: &Path, data: &Path) {
        self.poll();
        if self.editing_settings && !self.busy {
            self.settings = self.report.clone();
            self.editing_settings = false;
        }
        ui.heading("Sauvegardes complètes");
        ui.label("Sans secret.key, les secrets S3 et SMTP devront être ressaisis après restauration. Sur un partage UNC, utilisez un compte de service ayant accès au partage.");
        egui::ComboBox::from_id_salt("backup_files")
            .selected_text(&self.include_files)
            .show_ui(ui, |ui| {
                for value in ["folder_only", "all", "none"] {
                    ui.selectable_value(&mut self.include_files, value.into(), value);
                }
            });
        ui.checkbox(&mut self.secret, "Inclure secret.key (archive sensible)");
        if ui
            .add_enabled(!self.busy, egui::Button::new("Sauvegarder maintenant"))
            .clicked()
        {
            let mut args = vec![
                "backup".into(),
                "--include-files".into(),
                self.include_files.clone(),
            ];
            if self.secret {
                args.push("--with-secret-key".into());
            }
            self.run(ui, config, args, false);
        }
        if let Some(rx) = &self.backup_dir_rx {
            if let Ok(result) = rx.try_recv() {
                self.backup_dir = Some(result);
                self.backup_dir_rx = None;
            }
        }
        if (self.backup_dir.is_none()
            || ui.button("Actualiser le dossier des sauvegardes").clicked())
            && self.backup_dir_rx.is_none()
        {
            let (tx, rx) = mpsc::channel();
            self.backup_dir_rx = Some(rx);
            let config = config.to_path_buf();
            let data = data.to_path_buf();
            let ctx = ui.ctx().clone();
            std::thread::spawn(move || {
                let _ = tx.send(resolve_backup_dir(&config, &data));
                ctx.request_repaint();
            });
        }
        if let Some(Err(error)) = &self.backup_dir {
            ui.label(error);
        }
        let directory = self.backup_dir.as_ref().and_then(|r| r.as_ref().ok());
        if ui
            .add_enabled(
                directory.is_some(),
                egui::Button::new("Ouvrir le dossier des sauvegardes"),
            )
            .clicked()
        {
            let _ = crate::control::open_path(directory.unwrap());
        }
        if let Some(Ok(entries)) = directory.map(std::fs::read_dir) {
            let mut list: Vec<PathBuf> = entries
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|s| s == "zip"))
                .collect();
            list.sort();
            for path in list.iter().rev() {
                if ui
                    .selectable_label(
                        self.archive == path.display().to_string(),
                        format!(
                            "{} — {}",
                            path.file_name().unwrap().to_string_lossy(),
                            crate::diag::human_size(path.metadata().map(|m| m.len()).unwrap_or(0))
                        ),
                    )
                    .clicked()
                {
                    self.archive = path.display().to_string();
                }
            }
        }
        ui.label("Archive à restaurer (moteur arrêté)");
        ui.text_edit_singleline(&mut self.archive);
        ui.label("Mot de passe de l'archive (vide si non chiffrée)");
        ui.add(egui::TextEdit::singleline(&mut self.password).password(true));
        if ui
            .add_enabled(
                !self.busy,
                egui::Button::new("Voir le contenu de l'archive"),
            )
            .clicked()
        {
            self.busy = true;
            let tx = self.tx.clone();
            let path = self.archive.clone();
            let ctx = ui.ctx().clone();
            std::thread::spawn(move || {
                let result = (|| -> Result<String, String> {
                    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
                    let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
                    let mut text = String::new();
                    for index in 0..zip.len() {
                        let entry = zip.by_index_raw(index).map_err(|e| e.to_string())?;
                        text.push_str(&format!(
                            "{} — {}\n",
                            entry.name(),
                            crate::diag::human_size(entry.size())
                        ));
                    }
                    Ok(text)
                })();
                let _ = tx.send(result.unwrap_or_else(|e| e));
                ctx.request_repaint();
            });
        }
        ui.checkbox(
            &mut self.confirm,
            "Je confirme le remplacement de la base, de la configuration et des fichiers",
        );
        if ui
            .add_enabled(!self.busy, egui::Button::new("Restaurer"))
            .clicked()
        {
            let mut args = vec!["restore".into(), self.archive.clone(), "--confirmed".into()];
            if !self.password.is_empty() {
                args.extend(["--password-file".into(), "-".into()]);
            }
            self.run(ui, config, args, true);
        }
        self.report(ui);
        ui.collapsing("Planification et rétention (API locale)", |ui| {
            if ui
                .add_enabled(!self.busy, egui::Button::new("Charger les réglages"))
                .clicked()
            {
                self.settings_request(ui, config, None);
            }
            ui.add(
                egui::TextEdit::multiline(&mut self.settings)
                    .desired_rows(12)
                    .code_editor(),
            );
            if ui
                .add_enabled(!self.busy, egui::Button::new("Enregistrer les réglages"))
                .clicked()
            {
                self.settings_request(ui, config, Some(self.settings.clone()));
            }
        });
    }
    fn report(&self, ui: &mut egui::Ui) {
        if self.busy {
            ui.spinner();
        }
        egui::ScrollArea::vertical()
            .id_salt("operation_report")
            .max_height(350.0)
            .show(ui, |ui| {
                ui.monospace(&self.report);
            });
    }
    fn settings_request(&mut self, ui: &egui::Ui, config: &Path, patch: Option<String>) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.editing_settings = true;
        let tx = self.tx.clone();
        let config = config.to_path_buf();
        let ctx = ui.ctx().clone();
        std::thread::spawn(move || {
            let result = crate::probe::exploitation_settings(&config, patch.as_deref())
                .unwrap_or_else(|e| e);
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }
}

/// Lecture seule, également disponible lorsque le moteur est arrêté pour restauration.
fn resolve_backup_dir(config: &Path, data: &Path) -> Result<PathBuf, String> {
    use sqlx::Connection;
    let cfg = apex_config::store::load(config)
        .map_err(|e| e.to_string())?
        .config;
    let layout = apex_config::Layout::for_config_file(config).with_overrides(&cfg);
    let database = layout.database_file(&cfg);
    if !database.exists() {
        return Ok(data.join("backups"));
    }
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?
        .block_on(async {
            let mut conn = sqlx::SqliteConnection::connect_with(
                &sqlx::sqlite::SqliteConnectOptions::new()
                    .filename(database)
                    .read_only(true),
            )
            .await
            .map_err(|e| e.to_string())?;
            let value: Option<String> =
                sqlx::query_scalar("SELECT value FROM app_settings WHERE key='backup.dir'")
                    .fetch_optional(&mut conn)
                    .await
                    .map_err(|e| e.to_string())?;
            conn.close().await.map_err(|e| e.to_string())?;
            Ok(value
                .filter(|s| !s.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| data.join("backups")))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dossier_sauvegardes_configure_et_repli_moteur_arrete() {
        use sqlx::Connection;
        let temp = tempfile::tempdir().unwrap();
        let config = temp.path().join("apex-maint.toml");
        let data = temp.path().join("data");
        std::fs::create_dir_all(&data).unwrap();
        let cfg = apex_config::AppConfig::default();
        std::fs::write(&config, apex_config::store::render(&cfg)).unwrap();
        let database = apex_config::Layout::for_config_file(&config)
            .with_overrides(&cfg)
            .database_file(&cfg);
        assert_eq!(
            resolve_backup_dir(&config, &data).unwrap(),
            data.join("backups")
        );
        let custom = temp.path().join("archives");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let mut conn = sqlx::SqliteConnection::connect_with(
                &sqlx::sqlite::SqliteConnectOptions::new()
                    .filename(&database)
                    .create_if_missing(true),
            )
            .await
            .unwrap();
            sqlx::query("CREATE TABLE app_settings(key TEXT, value TEXT)")
                .execute(&mut conn)
                .await
                .unwrap();
            sqlx::query("INSERT INTO app_settings VALUES('backup.dir',?)")
                .bind(custom.to_string_lossy().as_ref())
                .execute(&mut conn)
                .await
                .unwrap();
            conn.close().await.unwrap();
        });
        assert_eq!(resolve_backup_dir(&config, &data).unwrap(), custom);
        runtime.block_on(async {
            let mut conn = sqlx::SqliteConnection::connect_with(
                &sqlx::sqlite::SqliteConnectOptions::new().filename(&database),
            )
            .await
            .unwrap();
            sqlx::query("UPDATE app_settings SET value=''")
                .execute(&mut conn)
                .await
                .unwrap();
            conn.close().await.unwrap();
        });
        assert_eq!(
            resolve_backup_dir(&config, &data).unwrap(),
            data.join("backups")
        );
    }
}
