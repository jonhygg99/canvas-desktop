//! Stub de Linux (decisión de alcance: Windows primero). La integración real
//! instalará un `.desktop` con `MimeType=...` en
//! `~/.local/share/applications/` y ejecutará `update-desktop-database`.

use std::path::{Path, PathBuf};

use crate::integration::{ShellError, ShellIntegration};

pub struct LinuxShell;

impl ShellIntegration for LinuxShell {
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
