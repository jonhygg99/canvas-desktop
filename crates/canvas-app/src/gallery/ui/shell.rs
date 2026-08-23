//! Revelar un archivo en el gestor de archivos del sistema. Mejor esfuerzo:
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

#[cfg(not(windows))]
pub(super) fn reveal_in_explorer(_path: &Path) {}
