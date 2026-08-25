//! Sincroniza el menú con el estado del editor de la ventana enfocada: la
//! barra nativa (Windows, solo en la raíz) se sondea y sincroniza cada
//! frame; en plataformas sin menú nativo se dibuja la barra de respaldo egui
//! EN CADA ventana, con el estado del editor de ESA ventana.

use eframe::egui;

use super::{AppInner, View, Workspace};

impl AppInner {
    /// Sondear clics del menú nativo (solo existe en la raíz de Windows) y
    /// dibujar la barra de respaldo egui donde haga falta: en toda ventana
    /// de macOS/Linux, y en las ventanas HIJAS de Windows (la raíz conserva
    /// la barra nativa, que no se puede clonar a otras ventanas).
    pub(super) fn sync_and_show_menu(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        ws: &mut Workspace,
        #[allow(unused)] // solo lo usa el cfg de Windows (hijas dibujan fallback)
        is_root: bool,
    ) {
        // Menus nativos (solo raíz en Windows): sondear clics y sincronizar
        // los ítems de editor contra el estado del workspace ENFOCADO.
        while let Some(action) = self.menus.as_ref().and_then(|m| m.poll()) {
            self.handle_menu_action(ws, action, ctx);
        }
        let (editor_open, can_undo, can_redo) = match &ws.view {
            View::Editor(state) => (true, state.can_undo(), state.can_redo()),
            _ => (false, false, false),
        };
        if editor_open != self.menu_mirror.menus_editor_open {
            self.menu_mirror.menus_editor_open = editor_open;
            if let Some(m) = self.menus.as_mut() {
                m.set_editor_enabled(editor_open);
            }
        }
        if (can_undo, can_redo)
            != (
                self.menu_mirror.menus_can_undo,
                self.menu_mirror.menus_can_redo,
            )
        {
            self.menu_mirror.menus_can_undo = can_undo;
            self.menu_mirror.menus_can_redo = can_redo;
            if let Some(m) = self.menus.as_mut() {
                m.set_undo_redo(can_undo, can_redo);
            }
        }

        // Barra de respaldo egui: en plataformas sin menú nativo (todas las
        // ventanas) y en las ventanas hijas de Windows.
        #[cfg(not(windows))]
        let draw_fallback = true;
        #[cfg(windows)]
        let draw_fallback = !is_root;
        if draw_fallback {
            let recents = self.settings.recent_files.clone();
            let action = egui::Panel::top("menu_bar")
                .show(ui, |ui| {
                    crate::menus::menu_bar_ui(ui, editor_open, can_undo, can_redo, &recents)
                })
                .inner;
            if let Some(action) = action {
                self.handle_menu_action(ws, action, ctx);
            }
        }
    }
}
