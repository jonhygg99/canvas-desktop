//! Respuestas del camino de exportacion: la ruta elegida y el resultado.

use std::path::PathBuf;

use super::super::{App, View};

impl App {
    pub(super) fn on_export_path_picked(&mut self, path: Option<PathBuf>) {
        if let (Some(path), Some(settings)) = (path, self.export.pending_export_settings.take()) {
            self.export.pending_export = Some((path, settings));
        } else {
            self.export.pending_export_settings = None;
        }
    }

    pub(super) fn on_exported(&mut self, path: PathBuf, result: Result<(), String>) {
        if let View::Editor(state) = &mut self.view {
            state.exporting = false;
            match result {
                Ok(()) => tracing::info!("exportado OK: {}", path.display()),
                Err(e) => {
                    state.save_error = Some(format!("Could not export: {e}"));
                }
            }
        }
    }
}
