//! Renderizado egui de la galería: panel de carpetas, cuadrícula de
//! miniaturas y sus celdas (con renombrado in-place y menú contextual).
//!
//! Reparto: `shortcuts` (atajos globales atendidos antes del pintado),
//! `folder_panel` (navegación de carpetas), `central` (cabecera, estados
//! de escaneo y cuadrícula), `cell` (cada celda) y `shell` (revelar en el
//! gestor de archivos del SO). `show` es el orquestador: el orden de sus
//! llamadas es el orden de pintado — los atajos leen el input crudo antes
//! de que los paneles lo consuman — y no debe alterarse.

use eframe::egui;

use super::{GalleryAction, GalleryState};

mod cell;
mod central;
mod folder_panel;
mod shell;
mod shortcuts;

pub use folder_panel::next_folder_panel_side;

// Nombre que solo usan los tests (`gallery/tests.rs`); el código de pintado
// lo toma directo de `cell`.
#[cfg(test)]
pub(super) use cell::gallery_cell_size;
use folder_panel::show_folder_panel;

pub fn show(state: &mut GalleryState, ui: &mut egui::Ui) -> Option<GalleryAction> {
    let mut action = shortcuts::handle(state, ui);

    if let Some(panel_action) = show_folder_panel(state, ui) {
        action = Some(panel_action);
    }

    if let Some(central_action) = central::show(state, ui) {
        action = Some(central_action);
    }

    action
}
