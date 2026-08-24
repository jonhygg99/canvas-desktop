//! Vista del editor. `editor_view_ui` es una funcion libre, no un metodo de
//! `App`: se llama mientras `state` sigue prestado de `self.view`, asi que
//! recibe el resto del estado de la ventana en un `EditorFrame`.
//!
//! Este archivo es SOLO orquestacion: llama a los submodulos EN EL MISMO orden
//! en que estaban sus bloques dentro de la funcion original. Ese orden es
//! significativo (los comentarios del codigo movido lo dicen explicitamente) y
//! no debe cambiarse al tocar este archivo.

use eframe::egui;
use eframe::egui_wgpu::RenderState;

use crate::{editor, menus};

use super::super::frame::EditorFrame;
use super::super::Nav;

mod deck_nav;
mod file_ops;
mod modals;
mod panels;
mod save_flow;

/// Vista de editor: baraja + panel de capas/propiedades + lienzo, y toda la
/// orquestación de guardado, exportación, navegación de la baraja y
/// deshacer/rehacer global de ese frame. `rs` ya viene resuelto por el
/// llamador (si `frame.wgpu_render_state()` fuera `None`, el llamador corta
/// el frame entero antes de entrar aquí, no solo esta vista).
pub(in crate::app) fn editor_view_ui(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    rs: &RenderState,
    state: &mut editor::EditorState,
    paste_requested: bool,
    f: &mut EditorFrame<'_>,
) -> (Option<Nav>, Option<menus::MenuAction>) {
    let mut open_next: Option<Nav> = None;
    // Acción del menú contextual del lienzo (clic derecho): se resuelve por
    // el llamador, una vez liberado el préstamo de `state`.
    let mut pending_menu_action: Option<menus::MenuAction> = None;

    // El deshacer/rehacer global (`push_undo_step`/`undo`/`redo` en
    // `editor.rs`) etiqueta cada paso con esta id: hay que tenerla al día
    // ANTES de `handle_shortcuts` (que puede disparar un Ctrl+Z ese mismo
    // frame) y de cualquier edición que ocurra más abajo en `canvas_ui`.
    // Barato de refrescar cada frame; más simple que perseguir cada sitio
    // donde `f.deck.active`/`self.view` pueden cambiar.
    state.active_slot_id = f.deck.slots.get(f.deck.active).map_or(0, |s| s.id);
    state.handle_shortcuts(ctx, paste_requested, f.deck.rename_edit.is_some());

    // Recarga pedida desde el banner de «cambió en disco».
    if std::mem::take(&mut state.reload_requested) {
        match state.doc.source_path.clone() {
            Some(path) => open_next = Some(Nav::Open(path)),
            None => state.external_change = false,
        }
    }

    // Volver a la galería (preguntando si hay cambios sin guardar).
    if state.return_requested {
        state.return_requested = false;
        if let Some(folder) = state.from_gallery.clone() {
            if !state.is_dirty() {
                open_next = Some(Nav::Open(folder));
            } else {
                let choice = rfd::MessageDialog::new()
                    .set_level(rfd::MessageLevel::Warning)
                    .set_title("Unsaved changes")
                    .set_description(format!(
                        "\"{}\" has unsaved changes.\nSave them before going back to the gallery? (\"No\" discards them.)",
                        state.file_name()
                    ))
                    .set_buttons(rfd::MessageButtons::YesNoCancelCustom(
                        "Save".to_owned(),
                        "Discard".to_owned(),
                        "Cancel".to_owned(),
                    ))
                    .show();
                // Igual que en confirm_close: en Windows el resultado llega
                // como Yes/No/Cancel, no Custom.
                match choice {
                    rfd::MessageDialogResult::Yes => {
                        f.save.save_requested = true;
                        f.save.after_save = Some(Nav::Open(folder));
                    }
                    rfd::MessageDialogResult::Custom(c) if c == "Save" => {
                        f.save.save_requested = true;
                        f.save.after_save = Some(Nav::Open(folder));
                    }
                    rfd::MessageDialogResult::No => {
                        open_next = Some(Nav::Open(folder));
                    }
                    rfd::MessageDialogResult::Custom(c) if c == "Discard" => {
                        open_next = Some(Nav::Open(folder));
                    }
                    _ => {}
                }
            }
        }
    }

    file_ops::handle_file_ops(state, ctx, f);
    save_flow::handle_save(state, ctx, rs, f, &mut open_next);
    modals::show_modals(state, ctx, rs, f);
    let (strip_action, canvas_action) = panels::show_panels(state, ui, rs, f);
    deck_nav::resolve(
        state,
        ctx,
        f,
        strip_action,
        canvas_action,
        &mut pending_menu_action,
    );

    if std::mem::take(&mut state.settings_clicked) {
        *f.show_settings = true;
    }
    if std::mem::take(&mut state.layers_panel_toggle) {
        f.settings.layers_collapsed = !f.settings.layers_collapsed;
        f.settings.save_in_background();
    }
    // El checkbox del sidecar en el editor ES el valor por defecto
    // persistido: cambiarlo ahí lo recuerda para el futuro. En un diseño el
    // checkbox ni se muestra: no debe tocar el ajuste.
    if !state.is_design && state.sidecar_enabled != f.settings.sidecar_default {
        f.settings.sidecar_default = state.sidecar_enabled;
        f.settings.save_in_background();
    }

    (open_next, pending_menu_action)
}
