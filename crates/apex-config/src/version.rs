//! Identité et version du produit, partagées par tous les composants.

/// Nom commercial du produit (dossiers système, raccourcis, IHM).
pub const PRODUCT_NAME: &str = "Apex Maint";

/// Version unique du produit (définie dans `[workspace.package]` du `Cargo.toml` racine).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Nom du binaire moteur + serveur web.
pub const ENGINE_BIN: &str = "apex-maint";

/// Nom du binaire de l'utilitaire desktop.
pub const ADMIN_BIN: &str = "apex-maint-admin";

/// Nom du service Windows / de l'unité systemd (`apex-maint.service`).
#[cfg(windows)]
pub const SERVICE_NAME: &str = "ApexMaint";
#[cfg(not(windows))]
pub const SERVICE_NAME: &str = "apex-maint";

/// Commit Git de compilation (ou « inconnu » hors d'un dépôt).
pub const GIT_HASH: &str = env!("APEX_GIT_HASH");

/// Cible de compilation (ex. `x86_64-pc-windows-msvc`).
pub const TARGET: &str = env!("APEX_TARGET");

/// Indique une version de pré-production (alpha, bêta, rc).
pub fn is_prerelease() -> bool {
    VERSION.contains('-')
}

/// Version complète affichée à l'utilisateur, ex. `0.1.0-alpha.1 (a1b2c3d, x86_64-unknown-linux-gnu)`.
pub fn full_version() -> String {
    format!("{} ({}, {})", VERSION, GIT_HASH, TARGET)
}
