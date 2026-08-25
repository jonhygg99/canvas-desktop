//! El frame de UNA ventana (la raíz o una hija): el mismo código que antes
//! vivía en `impl eframe::App::ui`, pero parametrizado por `Workspace` +
//! `AppInner`. Todo lo que toca el estado de la ventana (view, deck, save,
//! export…) va aquí, y las ventanas hijas reutilizan EXACTAMENTE el mismo
//! camino que la raíz — solo cambia el `ViewportId` sobre el que egui
//! dibuja.

use eframe::egui;

use crate::loader;
use crate::menus;
use crate::watcher;

use super::frame::EditorFrame;
use super::views;
use super::{AppInner, Nav, View, Workspace};

impl AppInner {
    /// Frame completo de un workspace: menú de respaldo, mensajes, la vista
    /// activa y el conmutador. `is_root` marca si esta ventana es la raíz
    /// (única con menús nativos).
    pub(crate) fn ws_frame(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        ws: &mut Workspace,
        ws_idx: usize,
        is_root: bool,
        paste_requested: bool,
    ) {
        let mut open_next: Option<Nav> = None;
        let mut pending_menu_action: Option<menus::MenuAction> = None;

        // Archivos sueltos sobre ESTA ventana.
        self.handle_dropped_files(ws, ctx);

        // Menú nativo (raíz) o barra de respaldo egui (toda ventana sin menú
        // nativo): siempre antes de la vista, igual que antes.
        self.sync_and_show_menu(ui, ctx, ws, is_root);

        match &mut ws.view {
            View::Welcome { error } => {
                let error = error.clone();
                let before_recent = self.settings.recent_files.len();
                let before_pin = self.settings.pinned_folders.len();
                open_next = views::welcome_view_ui(
                    ui,
                    error.as_deref(),
                    &mut self.settings.recent_files,
                    &mut self.settings.pinned_folders,
                    &mut ws.show_settings,
                    &ws.tx,
                    ctx,
                );
                if self.settings.recent_files.len() != before_recent
                    || self.settings.pinned_folders.len() != before_pin
                {
                    self.settings.save_in_background();
                    // El menú nativo (si existe) se entera del cambio vía el
                    // espejo de `App::sync_native_menu` al final del frame.
                }
            }
            View::Loading { path } => {
                views::loading_view_ui(ui, path);
            }
            View::Gallery(g) => {
                open_next = views::gallery_view_ui(
                    g,
                    ui,
                    &mut self.settings,
                    &mut ws.deck_ops.pending_deck,
                    &ws.tx,
                    ctx,
                );
            }
            View::Editor(state) => {
                // La vista del editor necesita el wgpu; eframe con la
                // feature wgpu siempre tiene `RenderState` (el fallback
                // glow ni compila aquí).
                let mut frame = EditorFrame {
                    deck: &mut ws.deck,
                    renderer: &mut self.renderer,
                    surface: &mut ws.surface,
                    tx: &ws.tx,
                    settings: &mut self.settings,
                    show_settings: &mut ws.show_settings,
                    watcher: &mut ws.watcher,
                    ignore_fs_events_until: &mut ws.ignore_fs_events_until,
                    save: &mut ws.save,
                    export: &mut ws.export,
                    deck_ops: &mut ws.deck_ops,
                };
                let (nav, action) =
                    views::editor_view_ui(ui, ctx, &self.rs, state, paste_requested, &mut frame);
                open_next = nav;
                pending_menu_action = action;
            }
        }

        // Resuelve la acción del menú contextual del lienzo capturada más
        // arriba (ver el porqué del aplazamiento en su declaración):
        // reutiliza el mismo camino que ya usa la barra de menú, sin
        // duplicar ninguna lógica.
        if let Some(action) = pending_menu_action {
            self.handle_menu_action(ws, action, ctx);
        }

        self.settings_window_ui(ws, ctx);
        self.about_window_ui(ws, ctx);

        // Cierre de ventana (X de la barra de título o Quit del menú): la
        // retirada real la hace el frame raíz (`finish_root_frame`).
        let close_requested = ctx.input(|i| i.viewport().close_requested());
        if close_requested {
            self.confirm_window_close(ws, ctx, is_root);
        }

        // Conmutador rápido: teclas + paleta en la ventana ENFOCADA.
        if self.focused_ui_matches(ws_idx) {
            // Ctrl/Cmd+N: nueva ventana (workspace) al instante. El foco se
            // difiere a `pending_focus` porque el viewport de la ventana
            // nueva no existe hasta `spawn_child_viewports`.
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::N)) {
                self.new_workspace();
                self.pending_focus = Some(self.workspaces.len() - 1);
            }
            // Ctrl/Cmd+T: nueva ventana abriendo directamente el selector
            // de carpeta (el resultado llega por el canal del workspace
            // nuevo, cuya ventana nace al final de este frame).
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::T)) {
                let new_ws = self.new_workspace();
                let idx = self.workspaces.len() - 1;
                self.pending_focus = Some(idx);
                let tx = new_ws.lock().unwrap().tx.clone();
                loader::spawn_pick_folder(tx, ctx.clone());
            }
            let now = ctx.input(|i| i.time);
            if let Some(target) =
                self.switcher
                    .handle_keys(ctx, self.workspaces.len(), self.focused, now)
            {
                self.focus_workspace(target, ctx);
            }
            super::switcher::switcher_overlay(
                ctx,
                &mut self.switcher,
                &self.workspaces,
                ws_idx,
                ws,
            );
        }

        if let Some(nav) = open_next {
            self.navigate(ws, nav, ctx);
        }

        // Mantén el watcher `notify` apuntando al archivo abierto (si lo hay).
        let desired = match &ws.view {
            View::Editor(state) => state.doc.source_path.clone(),
            _ => None,
        };
        if ws.watcher.as_ref().map(|w| w.path.as_path()) != desired.as_deref() {
            ws.watcher = desired.and_then(|p| watcher::watch(&p, ws.tx.clone(), ctx.clone()));
        }

        self.sync_title(ctx, ws);

        // Geometría de la ventana (persistencia del workspace).
        if let Some(rect) = ctx.input(|i| i.viewport().outer_rect.or(i.viewport().inner_rect)) {
            ws.geometry = Some((rect.min, rect.size()));
        }
    }

    /// ¿Es el workspace `ws_idx` la ventana enfocada? (para el conmutador).
    /// Por índice a propósito: este método se llama con el lock del
    /// workspace YA tomado (`std::sync::Mutex` no es reentrante).
    fn focused_ui_matches(&self, ws_idx: usize) -> bool {
        self.focused == ws_idx
    }
}
