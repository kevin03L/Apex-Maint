//! Adaptation du service de Supervision ; aucune action système dans les tests.
#[cfg(windows)]
#[path = "service_windows.rs"]
mod windows;
#[cfg(windows)]
pub use windows::manage;

#[cfg(not(windows))]
pub fn manage(
    action: &str,
    _config: Option<std::path::PathBuf>,
    _account: Option<String>,
) -> Result<String, String> {
    if !["start", "stop", "restart", "status", "enable", "disable"].contains(&action) {
        return Err("Sous Linux, installation et désinstallation passent par le paquet DEB ; le service lance « run ».".into());
    }
    let output = std::process::Command::new("systemctl")
        .args([action, "apex-maint.service"])
        .output()
        .map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).into_owned())
    }
}
