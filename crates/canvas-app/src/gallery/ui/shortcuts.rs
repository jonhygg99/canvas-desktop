//! Atajos globales de la galería, atendidos ANTES de pintar los paneles:
//! reintento de permisos al recuperar el foco (macOS), navegación con
//! `Alt+←/→/↑` y copiar/pegar un diseño entre carpetas.
//!
//! Copiar/pegar no puede ir por `consume_shortcut`: winit intercepta
//! Ctrl+C/V para el portapapeles del SO y egui los entrega como
//! `Event::Copy`/`Event::Paste(texto)`, así que hay que mirar los eventos
//! crudos. Se ignoran mientras se edita texto (p. ej. un renombrado en
//! curso) para no robarle el atajo al campo — mismo guard que
//! `EditorState::handle_shortcuts`.

use eframe::egui;

use super::super::{copy_to_slot, slot_contents, GalleryAction, GalleryState};

/// Los tres bloques de atajos, en orden. Igual que el `action` único que
/// sustituyen: el ÚLTIMO bloque que produce acción gana (no `or_else`,
/// que devolvería el primero).
pub(super) fn handle(state: &mut GalleryState, ui: &egui::Ui) -> Option<GalleryAction> {
    let mut action = None;

    // Reintento único tras dar permiso en Ajustes (macOS): al volver el
    // foco a la ventana relanzamos la apertura de la misma carpeta.
    let focused_now = ui.ctx().input(|i| i.viewport().focused.unwrap_or(true));
    if state.take_permission_retry_if_due(focused_now) {
        state.refresh_folder_lists();
        action = Some(GalleryAction::RetryScan);
    }

    if let Some(nav_action) = history_shortcuts(state, ui) {
        action = Some(nav_action);
    }

    if let Some(clip_action) = clipboard_shortcuts(state, ui) {
        action = Some(clip_action);
    }

    action
}

/// `Alt+←` atrás, `Alt+→` adelante, `Alt+↑` carpeta padre.
fn history_shortcuts(state: &mut GalleryState, ui: &egui::Ui) -> Option<GalleryAction> {
    if ui.ctx().text_edit_focused() {
        return None;
    }
    let (back, forward, parent) = ui.ctx().input(|i| {
        (
            i.modifiers.alt && i.key_pressed(egui::Key::ArrowLeft),
            i.modifiers.alt && i.key_pressed(egui::Key::ArrowRight),
            i.modifiers.alt && i.key_pressed(egui::Key::ArrowUp),
        )
    });
    if back && state.navigation.can_back() {
        Some(GalleryAction::Back)
    } else if forward && state.navigation.can_forward() {
        Some(GalleryAction::Forward)
    } else if parent {
        state
            .folder
            .parent()
            .map(|folder| GalleryAction::OpenFolder(folder.to_owned()))
    } else {
        None
    }
}

/// Ctrl+C copia el diseño seleccionado al slot interno de la app;
/// Ctrl+V pega en la carpeta actual.
fn clipboard_shortcuts(state: &mut GalleryState, ui: &egui::Ui) -> Option<GalleryAction> {
    if ui.ctx().text_edit_focused() {
        return None;
    }
    let (want_copy, want_paste) = ui.ctx().input(|i| {
        let mut copy = false;
        let mut paste = false;
        for ev in &i.events {
            match ev {
                egui::Event::Copy => copy = true,
                egui::Event::Paste(_) => paste = true,
                _ => {}
            }
        }
        (copy, paste)
    });
    if want_copy {
        if let Some(path) = state.selected.clone() {
            copy_to_slot(path);
        }
    }
    if want_paste {
        if let Some(path) = slot_contents() {
            return Some(GalleryAction::PasteHere(path));
        }
    }
    None
}
