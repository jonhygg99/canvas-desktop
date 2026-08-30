//! El panel central de la galería: cabecera (título de carpeta con
//! renombrado in-place, orden y densidad de columnas, botón «New design»),
//! banner de error de operación, estados del escaneo (cargando / error /
//! carpeta vacía) y la cuadrícula de celdas. `show` orquesta; el orden de
//! sus bloques es el orden de pintado y no debe alterarse.

use eframe::egui;

use super::super::{GalleryAction, GalleryState};
use super::cell::{gallery_add_cell, gallery_cell, gallery_cell_size, CELL_GAP, ROW_GAP};
use crate::app_icons::{
    draw_close_icon, draw_minus_icon, draw_plus_icon, icon_button_ui, icon_text_button_ui,
};
use crate::settings::GallerySort;

pub(super) fn show(state: &mut GalleryState, ui: &mut egui::Ui) -> Option<GalleryAction> {
    let mut action = None;
    egui::CentralPanel::default().show(ui, |ui| {
        ui.add_space(6.0);
        header_ui(state, ui, &mut action);
        op_error_ui(state, ui);
        ui.add_space(4.0);
        ui.separator();
        if scan_status_ui(state, ui, &mut action) {
            grid_ui(state, ui, &mut action);
        }
    });
    action
}

/// Cabecera de la carpeta: título con renombrado in-place, contador de
/// elementos y toolbar de orden/densidad/nuevo diseño — todo en una fila.
fn header_ui(state: &mut GalleryState, ui: &mut egui::Ui, action: &mut Option<GalleryAction>) {
    ui.horizontal(|ui| {
        // false = rama defensiva del renombrado (estado imposible en la
        // práctica): salta la toolbar, igual que el `return` del original.
        if folder_heading_ui(state, ui, action) {
            toolbar_ui(state, ui, action);
        }
    });
}

/// Título de la carpeta (clic para renombrar in-place) y contador. Devuelve
/// `false` solo por la rama defensiva que ya pintó la cabecera y debe
/// saltarse la toolbar.
fn folder_heading_ui(
    state: &mut GalleryState,
    ui: &mut egui::Ui,
    action: &mut Option<GalleryAction>,
) -> bool {
    let current_name = state
        .folder
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| state.folder.display().to_string());
    let renaming = state
        .folder_rename_edit
        .as_ref()
        .is_some_and(|(p, _)| p == &state.folder);
    if renaming {
        let Some((_, text)) = state.folder_rename_edit.as_mut() else {
            ui.heading(&current_name);
            ui.weak(format!("— {} items", state.items.len()));
            return false;
        };
        let id = egui::Id::new(("gallery_folder_rename_heading", &state.folder));
        let response = ui.add(
            egui::TextEdit::singleline(text)
                .id(id)
                .desired_width(200.0)
                .font(egui::TextStyle::Heading),
        );
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            state.folder_rename_edit = None;
        } else if response.lost_focus() {
            let trimmed = text.trim().to_owned();
            if !trimmed.is_empty() && trimmed != current_name {
                *action = Some(GalleryAction::RenameFolder(state.folder.clone(), trimmed));
            }
            state.folder_rename_edit = None;
        }
    } else {
        let btn = egui::Button::new(egui::RichText::new(&current_name).heading())
            .fill(egui::Color32::TRANSPARENT);
        if ui.add(btn).clicked() {
            state.folder_rename_edit = Some((state.folder.clone(), current_name.clone()));
            ui.ctx().memory_mut(|m| {
                m.request_focus(egui::Id::new((
                    "gallery_folder_rename_heading",
                    &state.folder,
                )));
            });
        }
    }
    ui.weak(format!("— {} items", state.items.len()));
    true
}

/// Orden de la galería (persistido), densidad de columnas y «New design».
fn toolbar_ui(state: &mut GalleryState, ui: &mut egui::Ui, action: &mut Option<GalleryAction>) {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let mut sort = state.sort;
        egui::ComboBox::from_id_salt("gallery_sort")
            .selected_text(sort.label())
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut sort, GallerySort::Name, GallerySort::Name.label());
                ui.selectable_value(
                    &mut sort,
                    GallerySort::DateModified,
                    GallerySort::DateModified.label(),
                );
            });
        ui.label("Sort by:");
        if sort != state.sort {
            state.sort = sort;
            state.apply_sort();
            *action = Some(GalleryAction::SortChanged(sort));
        }
        ui.add_space(12.0);
        if icon_button_ui(ui, 16.0, true, draw_plus_icon)
            .on_hover_text("Show more designs per line")
            .clicked()
        {
            state.gallery_columns = (state.gallery_columns + 1).min(12);
        }
        ui.label(format!("{} por línea", state.gallery_columns));
        if icon_button_ui(ui, 16.0, true, draw_minus_icon)
            .on_hover_text("Show fewer designs per line")
            .clicked()
        {
            state.gallery_columns = state.gallery_columns.saturating_sub(1).max(1);
        }
        if icon_text_button_ui(
            ui,
            true,
            draw_plus_icon,
            "New design",
            None,
            egui::Vec2::ZERO,
        )
        .clicked()
        {
            *action = Some(GalleryAction::NewDesign);
        }
    });
}

/// Banner del último error de operación de archivos (crear/duplicar/borrar),
/// con su botón de descarte.
fn op_error_ui(state: &mut GalleryState, ui: &mut egui::Ui) {
    if let Some(error) = state.op_error.clone() {
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(ui.visuals().error_fg_color, &error);
            if icon_button_ui(ui, 16.0, true, draw_close_icon).clicked() {
                state.op_error = None;
            }
        });
    }
}

/// Estados previos a la cuadrícula: escaneando, error de escaneo (con
/// reintento y, en macOS, acceso directo al panel de Full Disk Access) o
/// carpeta vacía (mensaje — la cuadrícula pinta igualmente la celda «+»).
/// Devuelve `true` cuando hay que pintar la cuadrícula.
fn scan_status_ui(
    state: &mut GalleryState,
    ui: &mut egui::Ui,
    action: &mut Option<GalleryAction>,
) -> bool {
    if !state.scanned {
        ui.vertical_centered(|ui| {
            ui.add_space(40.0);
            ui.add(egui::Spinner::new().size(28.0));
            ui.label("Scanning for images…");
        });
        return false;
    }
    // Clonado ANTES del &mut state: rompe el préstamo inmutable de
    // `scan_error` para que `scan_error_ui` pueda marcar el reintento.
    if let Some(error) = state.scan_error.clone() {
        scan_error_ui(state, error, ui, action);
        return false;
    }
    if state.items.is_empty() {
        ui.vertical_centered(|ui| {
            ui.add_space(20.0);
            ui.label("This folder is empty.");
            ui.weak("Create a blank canvas or open an image.");
        });
    }
    true
}

/// Error de escaneo: motivo en pantalla, botón «Retry» y, en macOS sobre
/// carpetas de nube, botón que abre el panel «Full Disk Access».
fn scan_error_ui(
    state: &mut GalleryState,
    error: String,
    ui: &mut egui::Ui,
    action: &mut Option<GalleryAction>,
) {
    // Solo macOS consume este valor hoy (botón de paneles de disco
    // del error de escaneo): en las demás plataformas el flujo de
    // permisos de almacenamiento en la nube todavía no existe.
    #[allow(unused_variables)]
    let cloud_folder = canvas_io::is_cloud_storage_path(&state.folder);
    let mut retry_clicked = false;
    // Sin `move`: el closure deja ver `retry_clicked` (y &mut state)
    // para poder señalar el reintento hacia fuera del bloque.
    ui.vertical_centered(|ui| {
        ui.add_space(20.0);
        ui.colored_label(ui.visuals().error_fg_color, "Could not read this folder.");
        ui.label(error.as_str());
        ui.add_space(8.0);
        if ui
            .button("Retry")
            .on_hover_text("Rescan this folder.")
            .clicked()
        {
            retry_clicked = true;
        }
        #[cfg(target_os = "macos")]
        if cloud_folder {
            ui.add_space(8.0);
            if ui
                .button("Open Privacy & Security settings")
                .on_hover_text(
                    "Opens the Full Disk Access pane: grant disk access to \
                     the app or terminal that launched Canvas Desktop.",
                )
                .clicked()
            {
                state.note_settings_opened();
                super::shell::open_full_disk_access_pane();
            }
        }
    });
    if retry_clicked {
        *action = Some(GalleryAction::RetryScan);
    }
}

/// La cuadrícula: filas de `columns` celdas con su miniatura, y la celda
/// «+» al final de la última fila incompleta (o en fila propia si la última
/// fila estaba llena).
fn grid_ui(state: &mut GalleryState, ui: &mut egui::Ui, action: &mut Option<GalleryAction>) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        let columns = state.gallery_columns.clamp(1, 12);
        let cell_size = gallery_cell_size(ui.available_width(), columns);
        ui.spacing_mut().item_spacing.y = ROW_GAP;
        let row_count = state.items.chunks(columns).len();
        let mut add_cell_rendered = false;
        for (row_index, row) in state.items.chunks(columns).enumerate() {
            let is_last_row = row_index + 1 == row_count;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = CELL_GAP;
                for item in row {
                    if let Some(cell_action) = gallery_cell(
                        ui,
                        item,
                        cell_size,
                        &mut state.selected,
                        &mut state.rename_edit,
                    ) {
                        *action = Some(cell_action);
                    }
                }
                if is_last_row && row.len() < columns {
                    add_cell_rendered = true;
                    if gallery_add_cell(ui, cell_size) {
                        *action = Some(GalleryAction::NewDesign);
                    }
                }
            });
            ui.add_space(CELL_GAP);
        }

        if !add_cell_rendered {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = CELL_GAP;
                if gallery_add_cell(ui, cell_size) {
                    *action = Some(GalleryAction::NewDesign);
                }
            });
        }
    });
}
