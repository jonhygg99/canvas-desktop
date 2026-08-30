//! Vista principal de la galería y su cuadrícula de diseños.

use super::super::{copy_to_slot, slot_contents, GalleryAction, GalleryState};
use super::cell::{gallery_add_cell, gallery_cell, CELL_GAP, ROW_GAP};
use super::folder_panel::show_folder_panel;
use crate::app_icons::{
    draw_close_icon, draw_minus_icon, draw_plus_icon, icon_button_ui, icon_text_button_ui,
};
use crate::settings::GallerySort;
use eframe::egui;

pub(super) fn show(state: &mut GalleryState, ui: &mut egui::Ui) -> Option<GalleryAction> {
    let mut action = None;
    let focused_now = ui.ctx().input(|i| i.viewport().focused.unwrap_or(true));
    if state.take_permission_retry_if_due(focused_now) {
        state.refresh_folder_lists();
        action = Some(GalleryAction::RetryScan);
    }
    handle_navigation_shortcuts(state, ui, &mut action);
    handle_clipboard_shortcuts(state, ui, &mut action);
    if let Some(panel_action) = show_folder_panel(state, ui) {
        action = Some(panel_action);
    }
    egui::CentralPanel::default().show(ui, |ui| {
        draw_gallery_header(state, ui, &mut action);
        if !state.scanned {
            ui.vertical_centered(|ui| {
                ui.add_space(40.0);
                ui.add(egui::Spinner::new().size(28.0));
                ui.label("Scanning for images…");
            });
            return;
        }
        if let Some(error) = state.scan_error.clone() {
            draw_scan_error(state, ui, &error, &mut action);
            return;
        }
        if state.items.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(20.0);
                ui.label("This folder is empty.");
                ui.weak("Create a blank canvas or open an image.");
            });
        }
        draw_gallery_grid(state, ui, &mut action);
    });
    action
}

fn handle_navigation_shortcuts(
    state: &mut GalleryState,
    ui: &egui::Ui,
    action: &mut Option<GalleryAction>,
) {
    if ui.ctx().text_edit_focused() {
        return;
    }
    let (back, forward, parent) = ui.ctx().input(|i| {
        (
            i.modifiers.alt && i.key_pressed(egui::Key::ArrowLeft),
            i.modifiers.alt && i.key_pressed(egui::Key::ArrowRight),
            i.modifiers.alt && i.key_pressed(egui::Key::ArrowUp),
        )
    });
    if back && state.navigation.can_back() {
        *action = Some(GalleryAction::Back);
    } else if forward && state.navigation.can_forward() {
        *action = Some(GalleryAction::Forward);
    } else if parent {
        if let Some(folder) = state.folder.parent() {
            *action = Some(GalleryAction::OpenFolder(folder.to_owned()));
        }
    }
}

fn handle_clipboard_shortcuts(
    state: &mut GalleryState,
    ui: &egui::Ui,
    action: &mut Option<GalleryAction>,
) {
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
    if ui.ctx().text_edit_focused() {
        return;
    }
    if want_copy {
        if let Some(path) = state.selected.clone() {
            copy_to_slot(path);
        }
    }
    if want_paste {
        if let Some(path) = slot_contents() {
            *action = Some(GalleryAction::PasteHere(path));
        }
    }
}

fn draw_gallery_header(
    state: &mut GalleryState,
    ui: &mut egui::Ui,
    action: &mut Option<GalleryAction>,
) {
    ui.add_space(6.0);
    ui.horizontal(|ui| {
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
            if let Some((_, text)) = state.folder_rename_edit.as_mut() {
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
            }
        } else if ui
            .add(
                egui::Button::new(egui::RichText::new(&current_name).heading())
                    .fill(egui::Color32::TRANSPARENT),
            )
            .clicked()
        {
            state.folder_rename_edit = Some((state.folder.clone(), current_name.clone()));
        }
        ui.weak(format!("— {} items", state.items.len()));
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
            if icon_button_ui(ui, 16.0, true, draw_plus_icon).clicked() {
                state.gallery_columns = (state.gallery_columns + 1).min(12);
            }
            ui.label(format!("{} por línea", state.gallery_columns));
            if icon_button_ui(ui, 16.0, true, draw_minus_icon).clicked() {
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
    });
    if let Some(error) = state.op_error.clone() {
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(ui.visuals().error_fg_color, &error);
            if icon_button_ui(ui, 16.0, true, draw_close_icon).clicked() {
                state.op_error = None;
            }
        });
    }
    ui.add_space(4.0);
    ui.separator();
}

fn draw_scan_error(
    state: &mut GalleryState,
    ui: &mut egui::Ui,
    error: &str,
    action: &mut Option<GalleryAction>,
) {
    #[allow(unused_variables)]
    let cloud_folder = canvas_io::is_cloud_storage_path(&state.folder);
    let mut retry = false;
    ui.vertical_centered(|ui| {
        ui.add_space(20.0);
        ui.colored_label(ui.visuals().error_fg_color, "Could not read this folder.");
        ui.label(error);
        ui.add_space(8.0);
        if ui.button("Retry").clicked() {
            retry = true;
        }
        #[cfg(target_os = "macos")]
        if cloud_folder && ui.button("Open Privacy & Security settings").clicked() {
            state.note_settings_opened();
            super::open_full_disk_access_pane();
        }
    });
    if retry {
        *action = Some(GalleryAction::RetryScan);
    }
}

fn draw_gallery_grid(
    state: &mut GalleryState,
    ui: &mut egui::Ui,
    action: &mut Option<GalleryAction>,
) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        let columns = state.gallery_columns.clamp(1, 12);
        let cell_size = super::gallery_cell_size(ui.available_width(), columns);
        ui.spacing_mut().item_spacing.y = ROW_GAP;
        let row_count = state.items.chunks(columns).len();
        let mut add_cell_rendered = false;
        for (row_index, row) in state.items.chunks(columns).enumerate() {
            let is_last_row = row_index + 1 == row_count;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = CELL_GAP;
                for item in row {
                    if let Some(a) = gallery_cell(
                        ui,
                        item,
                        cell_size,
                        &mut state.selected,
                        &mut state.rename_edit,
                    ) {
                        *action = Some(a);
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
