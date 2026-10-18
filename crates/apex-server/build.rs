//! Ressource Windows de `apex-maint.exe` : icône et VERSIONINFO (sans effet sur Linux).

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    apex_build::ressources_windows(apex_build::Executable {
        nom: "apex-maint",
        description: "Apex Maint — GMAO, moteur et serveur web",
    });
}
