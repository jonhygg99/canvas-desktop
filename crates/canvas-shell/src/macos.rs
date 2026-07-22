//! Stub de macOS (decisión de alcance: Windows primero). Las asociaciones en
//! macOS van por `CFBundleDocumentTypes` en el Info.plist del bundle y las
//! rutas llegan por `application:openURLs:`, no por argv; se implementará
//! cuando el proyecto se verifique en esa plataforma.

use std::path::{Path, PathBuf};

use crate::integration::{ShellError, ShellIntegration};

pub struct MacShell;

impl ShellIntegration for MacShell {
    fn register_file_associations(&self, _exe: &Path) -> Result<(), ShellError> {
        Err(ShellError::NotImplemented)
    }

    fn unregister_file_associations(&self) -> Result<(), ShellError> {
        Err(ShellError::NotImplemented)
    }

    fn update_jump_list(&self, _recents: &[PathBuf]) -> Result<(), ShellError> {
        Err(ShellError::NotImplemented)
    }
}
