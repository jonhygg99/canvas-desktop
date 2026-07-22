//! Trait de integración con el shell del sistema (asociaciones de archivo,
//! menú contextual, lista de recientes del SO). Cada plataforma lo implementa
//! en su módulo tras `#[cfg]`; el resto de la app solo ve este trait.

use std::path::{Path, PathBuf};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ShellError {
    #[error("Explorer integration failed: {0}")]
    Registry(String),
    #[error("This feature is not implemented on this platform yet")]
    NotImplemented,
}

pub trait ShellIntegration {
    /// Registra la app como opción de «Abrir con» para los formatos
    /// soportados y añade el menú contextual de carpetas. `exe` es la ruta
    /// del ejecutable que debe recibir el archivo por argv.
    fn register_file_associations(&self, exe: &Path) -> Result<(), ShellError>;

    /// Borra exactamente lo que `register_file_associations` creó.
    fn unregister_file_associations(&self) -> Result<(), ShellError>;

    /// Publica los archivos recientes en el mecanismo del SO (Jump List de
    /// Windows, Dock de macOS). Mejor esfuerzo.
    fn update_jump_list(&self, recents: &[PathBuf]) -> Result<(), ShellError>;
}

/// La implementación de la plataforma actual.
pub fn platform() -> impl ShellIntegration {
    #[cfg(target_os = "windows")]
    {
        crate::windows::WindowsShell
    }
    #[cfg(target_os = "macos")]
    {
        crate::macos::MacShell
    }
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        crate::linux::LinuxShell
    }
}
