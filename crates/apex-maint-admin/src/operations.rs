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
        std::thread::spawn(move || {
            let output = crate::control::engine_command(&config, &args);
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
        if ui.button("Ouvrir le dossier par défaut").clicked() {
            let _ = crate::control::open_path(&data.join("backups"));
        }
        if let Ok(entries) = std::fs::read_dir(data.join("backups")) {
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
            self.run(
                ui,
                config,
                vec!["restore".into(), self.archive.clone(), "--confirmed".into()],
                true,
            );
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
