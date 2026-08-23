//! Sincroniza el menú nativo (Windows) con el estado del editor y, en
//! plataformas sin menú nativo, dibuja la barra de menús de respaldo en egui.
//! Se llama al principio de cada frame, antes de resolver la vista activa.

use eframe::egui;

use super::{App, View};

impl App {
    pub(super) fn sync_and_show_menu(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        // Menú nativo: sondear clics y sincronizar los ítems de editor.
        while let Some(action) = self.menus.as_ref().and_then(|m| m.poll()) {
            self.handle_menu_action(action, ctx);
        }
        let editor_open = matches!(self.view, View::Editor(_));
        if editor_open != self.menu_mirror.menus_editor_open {
            self.menu_mirror.menus_editor_open = editor_open;
            if let Some(m) = self.menus.as_mut() {
                m.set_editor_enabled(editor_open);
            }
        }
        // Estado real del historial del editor activo (`false` en cualquier
        // otra vista): sincroniza los ítems Undo/Redo del menú, tanto el
        // nativo como el de respaldo — mismo criterio que `menus_editor_open`
        // de arriba.
        let (can_undo, can_redo) = match &self.view {
            View::Editor(state) => (state.can_undo(), state.can_redo()),
            _ => (false, false),
        };
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

        // Fallback sin menú nativo (macOS/Linux): barra de menús egui.
        #[cfg(not(windows))]
        {
            let recents = self.settings.recent_files.clone();
            let action = egui::Panel::top("menu_bar")
                .show(ui, |ui| {
                    crate::menus::menu_bar_ui(ui, editor_open, can_undo, can_redo, &recents)
                })
                .inner;
            if let Some(action) = action {
                self.handle_menu_action(action, ctx);
            }
        }
        #[cfg(windows)]
        let _ = ui;
    }
}
