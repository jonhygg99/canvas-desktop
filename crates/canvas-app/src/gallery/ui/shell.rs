//! Revela un archivo en el gestor de archivos del sistema. Mejor esfuerzo:
//! no hay nada sensato que hacer si falla, asi que no se reporta.

use std::path::Path;

/// Abre el Explorador de Windows con `path` ya seleccionado. Mejor esfuerzo:
/// no hay nada sensato que hacer si falla, así que no se reporta.
#[cfg(windows)]
pub(super) fn reveal_in_explorer(path: &Path) {
    if let Err(e) = std::process::Command::new("explorer")
        .arg("/select,")
        .arg(path)
        .spawn()
    {
        tracing::debug!("no se pudo abrir el Explorador en {}: {e}", path.display());
    }
}

/// Abre Finder con `path` ya seleccionado (macOS).
#[cfg(target_os = "macos")]
pub(super) fn reveal_in_explorer(path: &Path) {
    if let Err(e) = std::process::Command::new("open")
        .arg("-R")
        .arg(path)
        .spawn()
    {
        tracing::debug!("no se pudo abrir Finder en {}: {e}", path.display());
    }
}

/// Abre el gestor de archivos en la carpeta que contiene `path` (Linux).
/// `xdg-open` abre la carpeta en el gestor predeterminado del usuario.
#[cfg(target_os = "linux")]
pub(super) fn reveal_in_explorer(path: &Path) {
    let dir = path.parent().unwrap_or(path);
    if let Err(e) = std::process::Command::new("xdg-open").arg(dir).spawn() {
        tracing::debug!(
            "no se pudo abrir el gestor de archivos en {}: {e}",
            dir.display()
        );
    }
}
