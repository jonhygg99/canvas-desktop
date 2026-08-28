//! Respuestas del camino de exportacion: la ruta elegida y el resultado.

use std::path::PathBuf;

use super::super::{AppInner, View, Workspace};

impl AppInner {
    pub(super) fn on_export_path_picked(&mut self, ws: &mut Workspace, path: Option<PathBuf>) {
        if let (Some(path), Some(settings)) = (path, ws.export.pending_export_settings.take()) {
            ws.export.pending_export = Some((path, settings));
        } else {
            ws.export.pending_export_settings = None;
        }
    }

    pub(super) fn on_exported(
        &mut self,
        ws: &mut Workspace,
        path: PathBuf,
        result: Result<(), canvas_io::IoError>,
    ) {
        if let View::Editor(state) = &mut ws.view {
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
