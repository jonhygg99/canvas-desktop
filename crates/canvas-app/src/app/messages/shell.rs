//! Mensajes que vienen del sistema o de una segunda instancia: traer una
//! ventana al frente y el resultado del registro de la integración con el
//! Explorador.

use eframe::egui;

use crate::lock::LockExt;

use super::super::AppInner;

impl AppInner {
    pub(super) fn on_focus_window(&mut self, ctx: &egui::Context) {
        // La ventana enfocada es la que manda: traerla al frente.
        let idx = self.focused.min(self.workspaces.len().saturating_sub(1));
        if let Some(ws_arc) = self.workspaces.get(idx) {
            let viewport = ws_arc.lock_ok().viewport;
            ctx.send_viewport_cmd_to(viewport, egui::ViewportCommand::Focus);
        }
    }

    pub(super) fn on_shell_integration_done(&mut self, result: Result<String, String>) {
        self.shell_status = match result {
            Ok(msg) => msg,
            Err(e) => format!("Failed: {e}"),
        };
    }

    /// Una segunda apertura (o "Open with" del Explorador) pide abrir una
    /// ruta: se abre en la ventana ENFOCADA, como siempre.
    pub(super) fn on_open_path_external(&mut self, ctx: &egui::Context, path: std::path::PathBuf) {
        self.on_focus_window(ctx);
        let idx = self.focused.min(self.workspaces.len().saturating_sub(1));
        if let Some(ws_arc) = self.workspaces.get(idx).cloned() {
            let mut ws = ws_arc.lock_ok();
            // Pregunta si hay un editor con cambios sin guardar.
            self.request_nav(&mut ws, super::super::Nav::Open(path), ctx);
        }
    }
}
