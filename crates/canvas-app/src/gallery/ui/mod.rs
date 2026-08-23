//! Renderizado egui de la galeria: panel de carpetas, cuadricula de
//! miniaturas y sus celdas (con renombrado in-place y menu contextual).

use eframe::egui;

use crate::settings::GallerySort;

use super::{copy_to_slot, slot_contents, GalleryAction, GalleryState};

mod cell;
mod folder_panel;
mod shell;

pub use folder_panel::next_folder_panel_side;

pub(super) use cell::gallery_cell_size;

use cell::{gallery_add_cell, gallery_cell, CELL_GAP, ROW_GAP};
use folder_panel::show_folder_panel;

pub fn show(state: &mut GalleryState, ui: &mut egui::Ui) -> Option<GalleryAction> {
    let mut action = None;

    if !ui.ctx().text_edit_focused() {
        let (back, forward, parent) = ui.ctx().input(|i| {
            (
                i.modifiers.alt && i.key_pressed(egui::Key::ArrowLeft),
                i.modifiers.alt && i.key_pressed(egui::Key::ArrowRight),
                i.modifiers.alt && i.key_pressed(egui::Key::ArrowUp),
            )
        });
        if back && state.navigation.can_back() {
            action = Some(GalleryAction::Back);
        } else if forward && state.navigation.can_forward() {
            action = Some(GalleryAction::Forward);
        } else if parent {
            if let Some(folder) = state.folder.parent() {
                action = Some(GalleryAction::OpenFolder(folder.to_owned()));
            }
        }
    }

    // Ctrl+C / Ctrl+V: copiar/pegar un diseño entre carpetas. winit no deja
    // pasar Ctrl+C/V como pulsaciones normales de tecla — los intercepta
    // para la integración con el portapapeles del SO y en su lugar egui los
    // entrega como `Event::Copy`/`Event::Paste(texto)`, así que
    // `consume_shortcut` nunca los ve; hay que mirar los eventos crudos.
    // Se ignoran mientras se edita texto (p. ej. un renombrado en curso)
    // para no robarle el atajo al campo — mismo guard que
    // `EditorState::handle_shortcuts`.
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
    if !ui.ctx().text_edit_focused() {
        if want_copy {
            if let Some(path) = state.selected.clone() {
                copy_to_slot(path);
            }
        }
        if want_paste {
            if let Some(path) = slot_contents() {
                action = Some(GalleryAction::PasteHere(path));
            }
        }
    }

    if let Some(panel_action) = show_folder_panel(state, ui) {
        action = Some(panel_action);
    }

    egui::CentralPanel::default().show(ui, |ui| {
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
                let Some((_, text)) = state.folder_rename_edit.as_mut() else {
                    ui.heading(&current_name);
                    ui.weak(format!("— {} items", state.items.len()));
                    return;
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
                        action = Some(GalleryAction::RenameFolder(state.folder.clone(), trimmed));
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

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut sort = state.sort;
                egui::ComboBox::from_id_salt("gallery_sort")
                    .selected_text(sort.label())
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut sort,
                            GallerySort::Name,
                            GallerySort::Name.label(),
                        );
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
                    action = Some(GalleryAction::SortChanged(sort));
                }
                ui.add_space(12.0);
                if ui
                    .small_button("+")
                    .on_hover_text("Show more designs per line")
                    .clicked()
                {
                    state.gallery_columns = (state.gallery_columns + 1).min(12);
                }
                ui.label(format!("{} por línea", state.gallery_columns));
                if ui
                    .small_button("−")
                    .on_hover_text("Show fewer designs per line")
                    .clicked()
                {
                    state.gallery_columns = state.gallery_columns.saturating_sub(1).max(1);
                }
                if ui.button("✚ New design").clicked() {
                    action = Some(GalleryAction::NewDesign);
                }
            });
        });
        if let Some(error) = state.op_error.clone() {
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(ui.visuals().error_fg_color, &error);
                if ui.small_button("✕").clicked() {
                    state.op_error = None;
                }
            });
        }
        ui.add_space(4.0);
        ui.separator();

        if !state.scanned {
            ui.vertical_centered(|ui| {
                ui.add_space(40.0);
                ui.add(egui::Spinner::new().size(28.0));
                ui.label("Scanning for images…");
            });
            return;
        }
        if state.items.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(20.0);
                ui.label("This folder is empty.");
                ui.weak("Create a blank canvas or open an image.");
            });
        }

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
                            action = Some(cell_action);
                        }
                    }
                    if is_last_row && row.len() < columns {
                        add_cell_rendered = true;
                        if gallery_add_cell(ui, cell_size) {
                            action = Some(GalleryAction::NewDesign);
                        }
                    }
                });
                ui.add_space(CELL_GAP);
            }

            if !add_cell_rendered {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = CELL_GAP;
                    if gallery_add_cell(ui, cell_size) {
                        action = Some(GalleryAction::NewDesign);
                    }
                });
            }
        });
    });
    action
}
