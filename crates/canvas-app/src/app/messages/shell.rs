//! Mensajes que vienen del sistema o de una segunda instancia: traer la
//! ventana al frente, abrir una ruta, y el resultado del registro de la
//! integracion con el Explorador.

use std::path::PathBuf;

use eframe::egui;

use super::super::{App, Nav};

impl App {
    pub(super) fn on_focus_window(&mut self, ctx: &egui::Context) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    pub(super) fn on_shell_integration_done(&mut self, result: Result<String, String>) {
        self.shell_status = match result {
            Ok(msg) => msg,
            Err(e) => format!("Failed: {e}"),
        };
    }

    pub(super) fn on_open_path_external(&mut self, path: PathBuf, ctx: &egui::Context) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        // Pregunta si hay un editor con cambios sin guardar.
        self.request_nav(Nav::Open(path), ctx);
    }
}
