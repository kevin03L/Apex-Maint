//! Ressource Windows de `apex-maint-admin.exe` : icône et VERSIONINFO (sans effet sur Linux).

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    apex_build::ressources_windows(apex_build::Executable {
        nom: "apex-maint-admin",
        description: "Apex Maint — utilitaire d'administration",
    });
}
