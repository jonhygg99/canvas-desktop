//! La barra de respaldo egui, dibujada EN CADA ventana que no tiene menú
//! nativo: toda ventana de macOS/Linux y las ventanas HIJAS de Windows (la
//! raíz conserva la barra nativa, que no se puede clonar a otras ventanas).
//! El menú nativo en sí (sondeo + sincronización) vive en `App` (ver
//! `App::poll_native_menu` / `App::sync_native_menu`): sus `Rc` internos de
//! muda romperían `Send`/`Sync` si estuvieran en el `AppInner` compartido.

use eframe::egui;

use super::{AppInner, View, Workspace};

impl AppInner {
    /// Dibuja la barra de respaldo egui donde haga falta, con el estado del
    /// editor de ESTA ventana: en toda ventana de macOS/Linux y en las
    /// ventanas hijas de Windows (la raíz de Windows usa el menú nativo).
    pub(super) fn sync_and_show_menu(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        ws: &mut Workspace,
        #[allow(unused)] // solo lo usa el cfg de Windows (hijas dibujan fallback)
        is_root: bool,
    ) {
        let (editor_open, can_undo, can_redo) = match &ws.view {
            View::Editor(state) => (true, state.can_undo(), state.can_redo()),
            _ => (false, false, false),
        };
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
