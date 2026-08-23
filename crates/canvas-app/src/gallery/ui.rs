//! Renderizado egui de la galería: panel de carpetas, cuadrícula de
//! miniaturas y sus celdas (con renombrado in-place y menú contextual).

use std::path::{Path, PathBuf};

use eframe::egui;

use crate::deck::StripSide;
use crate::settings::GallerySort;

use super::{copy_to_slot, slot_contents, GalleryAction, GalleryItem, GalleryState, ItemKind};

/// Abre el Explorador de Windows con `path` ya seleccionado. Mejor esfuerzo:
/// no hay nada sensato que hacer si falla, así que no se reporta.
#[cfg(windows)]
fn reveal_in_explorer(path: &Path) {
    if let Err(e) = std::process::Command::new("explorer")
        .arg("/select,")
        .arg(path)
        .spawn()
    {
        tracing::debug!("no se pudo abrir el Explorador en {}: {e}", path.display());
    }
}

#[cfg(not(windows))]
fn reveal_in_explorer(_path: &Path) {}

const MIN_CELL_WIDTH: f32 = 140.0;
const PREFERRED_CELL_WIDTH: f32 = 156.0;
const CELL_GAP: f32 = 8.0;
const ROW_GAP: f32 = 4.0;
const THUMB_INSET: f32 = 8.0;
const THUMB_ASPECT_RATIO: f32 = 16.0 / 9.0;
const TITLE_HEIGHT: f32 = 20.0;
const TITLE_TO_THUMB_GAP: f32 = 2.0;
const CARD_BOTTOM_PADDING: f32 = 6.0;

/// Styled "✚" cell for creating a new blank canvas, inserted at the
/// end of the gallery grid. Its title and thumbnail area match regular
/// gallery pages.
fn gallery_add_cell(ui: &mut egui::Ui, cell_size: egui::Vec2) -> bool {
    let (rect, response) = ui.allocate_exact_size(cell_size, egui::Sense::click());
    if !ui.is_rect_visible(rect) {
        return response.clicked();
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let painter = ui.painter();
    let name_rect = egui::Rect::from_min_size(
        rect.left_top() + egui::vec2(8.0, 4.0),
        egui::vec2(rect.width() - 16.0, TITLE_HEIGHT - 4.0),
    );
    painter.text(
        name_rect.left_center(),
        egui::Align2::LEFT_CENTER,
        "New design",
        egui::FontId::proportional(12.5),
        ui.visuals().text_color(),
    );

    let thumbnail_width = rect.width() - THUMB_INSET * 2.0;
    let thumbnail_height = thumbnail_width / THUMB_ASPECT_RATIO;
    let thumb_rect = egui::Rect::from_min_size(
        rect.left_top() + egui::vec2(THUMB_INSET, TITLE_HEIGHT + TITLE_TO_THUMB_GAP),
        egui::vec2(thumbnail_width, thumbnail_height),
    );
    // Give the add tile the same visual footprint as the regular gallery
    // thumbnails while keeping its title outside the outlined area.
    let add_rect = egui::Rect::from_min_max(
        egui::pos2(rect.left() + 2.0, thumb_rect.top()),
        egui::pos2(rect.right() - 2.0, thumb_rect.bottom()),
    );
    if response.hovered() {
        painter.rect_filled(add_rect, 6.0, ui.visuals().widgets.hovered.weak_bg_fill);
    }
    painter.rect_stroke(
        add_rect,
        6.0,
        egui::Stroke::new(1.0, ui.visuals().weak_text_color()),
        egui::StrokeKind::Inside,
    );
    let plus_size = (thumbnail_height * 0.4).max(18.0);
    painter.text(
        add_rect.center(),
        egui::Align2::CENTER_CENTER,
        "✚",
        egui::FontId::proportional(plus_size),
        ui.visuals().weak_text_color(),
    );
    response
        .on_hover_text("Create a new blank canvas in this folder")
        .clicked()
}

pub(super) fn gallery_column_count(available_width: f32) -> usize {
    ((available_width + CELL_GAP) / (PREFERRED_CELL_WIDTH + CELL_GAP))
        .floor()
        .max(1.0) as usize
}

pub(super) fn gallery_cell_size(available_width: f32, columns: usize) -> egui::Vec2 {
    let width = ((available_width - CELL_GAP * (columns.saturating_sub(1) as f32))
        / columns as f32)
        .max(MIN_CELL_WIDTH);
    let thumbnail_height = (width - THUMB_INSET * 2.0) / THUMB_ASPECT_RATIO;
    egui::vec2(
        width,
        TITLE_HEIGHT + TITLE_TO_THUMB_GAP + thumbnail_height + CARD_BOTTOM_PADDING,
    )
}

pub fn next_folder_panel_side(side: StripSide) -> StripSide {
    match side {
        StripSide::Left => StripSide::Bottom,
        StripSide::Bottom => StripSide::Right,
        StripSide::Right => StripSide::Top,
        StripSide::Top => StripSide::Left,
    }
}

fn folder_name(folder: &Path) -> String {
    folder
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| folder.display().to_string())
}

fn folder_button_list(
    ui: &mut egui::Ui,
    folders: &[PathBuf],
    current: Option<&Path>,
    rename_edit: &mut Option<(PathBuf, String)>,
    action: &mut Option<GalleryAction>,
) {
    for folder in folders {
        let name = folder_name(folder);
        let rename_this = rename_edit
            .as_ref()
            .is_some_and(|(p, _)| p == folder);
        if rename_this {
            let Some((_, text)) = rename_edit.as_mut() else { continue };
            let id = egui::Id::new(("folder_rename", folder.as_os_str()));
            let response = ui.add(
                egui::TextEdit::singleline(text)
                    .id(id)
                    .desired_width(140.0),
            );
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                *rename_edit = None;
            } else if response.lost_focus() {
                let trimmed = text.trim().to_owned();
                let old_name = folder_name(folder);
                if !trimmed.is_empty() && trimmed != old_name {
                    *action = Some(GalleryAction::RenameFolder(folder.clone(), trimmed));
                }
                *rename_edit = None;
            }
        } else if current == Some(folder.as_path()) {
            let label = ui.add(
                egui::Label::new(egui::RichText::new(&name).strong())
                    .sense(egui::Sense::click()),
            );
            label.context_menu(|ui| {
                if ui.button("Rename").clicked() {
                    let stem = folder_name(folder);
                    *rename_edit = Some((folder.clone(), stem));
                    ui.ctx().memory_mut(|m| {
                        m.request_focus(egui::Id::new(("folder_rename", folder.as_os_str())));
                    });
                    ui.close();
                }
                let delete_label = egui::RichText::new("Delete").color(ui.visuals().warn_fg_color);
                if ui.button(delete_label).clicked() {
                    let fname = folder_name(folder);
                    let confirmed = rfd::MessageDialog::new()
                        .set_level(rfd::MessageLevel::Warning)
                        .set_title("Delete folder")
                        .set_description(format!(
                            "Move \"{fname}\" to the trash?\nThis deletes all files inside it.",
                        ))
                        .set_buttons(rfd::MessageButtons::OkCancel)
                        .show();
                    if confirmed == rfd::MessageDialogResult::Ok {
                        *action = Some(GalleryAction::DeleteFolder(folder.clone()));
                    }
                    ui.close();
                }
            });
        } else {
            let btn = ui.button(name);
            if btn.clicked() {
                *action = Some(GalleryAction::OpenFolder(folder.clone()));
            }
            btn.context_menu(|ui| {
                if ui.button("Rename").clicked() {
                    let stem = folder_name(folder);
                    *rename_edit = Some((folder.clone(), stem));
                    ui.ctx().memory_mut(|m| {
                        m.request_focus(egui::Id::new(("folder_rename", folder.as_os_str())));
                    });
                    ui.close();
                }
                let delete_label = egui::RichText::new("Delete").color(ui.visuals().warn_fg_color);
                if ui.button(delete_label).clicked() {
                    let folder_name = folder_name(folder);
                    let confirmed = rfd::MessageDialog::new()
                        .set_level(rfd::MessageLevel::Warning)
                        .set_title("Delete folder")
                        .set_description(format!(
                            "Move \"{folder_name}\" to the trash?\nThis deletes all files inside it.",
                        ))
                        .set_buttons(rfd::MessageButtons::OkCancel)
                        .show();
                    if confirmed == rfd::MessageDialogResult::Ok {
                        *action = Some(GalleryAction::DeleteFolder(folder.clone()));
                    }
                    ui.close();
                }
            });
        }
    }
}

fn folder_panel_contents(state: &mut GalleryState, ui: &mut egui::Ui) -> Option<GalleryAction> {
    let mut action = None;
    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        ui.strong("Folders");
        if ui
            .small_button("↻")
            .on_hover_text("Refresh folder list")
            .clicked()
        {
            state.refresh_folder_lists();
        }
        if ui
            .small_button("Change View")
            .on_hover_text(format!(
                "Change folders view to {}",
                next_folder_panel_side(state.folder_panel_side).label()
            ))
            .clicked()
        {
            action = Some(GalleryAction::CycleFolderPanelSide);
        }
    });
    ui.horizontal_wrapped(|ui| {
        if ui
            .add_enabled(state.navigation.can_back(), egui::Button::new("<"))
            .on_hover_text("Back to previous folder (Alt+Left)")
            .clicked()
        {
            action = Some(GalleryAction::Back);
        }
        if ui
            .add_enabled(state.navigation.can_forward(), egui::Button::new(">"))
            .on_hover_text("Forward to next folder (Alt+Right)")
            .clicked()
        {
            action = Some(GalleryAction::Forward);
        }
        if let Some(parent) = state.folder.parent() {
            if ui
                .small_button("Parent")
                .on_hover_text("Open parent folder (Alt+Up)")
                .clicked()
            {
                action = Some(GalleryAction::OpenFolder(parent.to_owned()));
            }
        }
    });
    ui.separator();

    let current_name = folder_name(&state.folder);
    if state.folder_panel_side.is_vertical_flow() {
        egui::ScrollArea::vertical()
            .id_salt("gallery_folder_tree_vertical")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.strong(format!("Inside {current_name}"));
                new_folder_ui(ui, "inside", &state.folder, &mut state.new_folder_inside, &mut action);
                ui.add_space(2.0);
                if state.folders.children.is_empty() && state.new_folder_inside.is_none() {
                    ui.weak("No subfolders");
                } else if !state.folders.children.is_empty() {
                    folder_button_list(ui, &state.folders.children, None, &mut state.folder_rename_edit, &mut action);
                }
                ui.add_space(8.0);
                ui.separator();
                ui.strong("Siblings");
                if let Some(parent) = state.folder.parent() {
                    new_folder_ui(ui, "sibling", &parent, &mut state.new_folder_sibling, &mut action);
                    ui.add_space(2.0);
                }
                folder_button_list(
                    ui,
                    &state.folders.siblings,
                    Some(state.folder.as_path()),
                    &mut state.folder_rename_edit,
                    &mut action,
                );
            });
    } else {
        ui.strong(format!("Inside {current_name}"));
        new_folder_ui(ui, "inside", &state.folder, &mut state.new_folder_inside, &mut action);
        ui.add_space(2.0);
        egui::ScrollArea::horizontal()
            .id_salt("gallery_child_folders_horizontal")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if state.folders.children.is_empty() && state.new_folder_inside.is_none() {
                        ui.weak("No subfolders");
                    } else if !state.folders.children.is_empty() {
                        folder_button_list(ui, &state.folders.children, None, &mut state.folder_rename_edit, &mut action);
                    }
                });
            });
        ui.separator();
        ui.strong("Siblings");
        if let Some(parent) = state.folder.parent() {
            new_folder_ui(ui, "sibling", &parent, &mut state.new_folder_sibling, &mut action);
            ui.add_space(2.0);
        }
        egui::ScrollArea::horizontal()
            .id_salt("gallery_sibling_folders_horizontal")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    folder_button_list(
                        ui,
                        &state.folders.siblings,
                        Some(state.folder.as_path()),
                        &mut state.folder_rename_edit,
                        &mut action,
                    );
                });
            });
    }
    action
}

fn new_folder_ui(
    ui: &mut egui::Ui,
    id_prefix: &str,
    parent: &Path,
    new_folder_name: &mut Option<String>,
    action: &mut Option<GalleryAction>,
) {
    match new_folder_name {
        Some(name) => {
            let id = egui::Id::new(("new_folder_input", id_prefix, parent));
            let response = ui.add(
                egui::TextEdit::singleline(name)
                    .id(id)
                    .hint_text("Folder name")
                    .desired_width(160.0),
            );
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                *new_folder_name = None;
            } else if response.lost_focus() {
                let trimmed = name.trim().to_owned();
                if !trimmed.is_empty() {
                    *action = Some(GalleryAction::CreateFolder(parent.to_path_buf(), trimmed));
                }
                *new_folder_name = None;
            }
        }
        None => {
            if ui.small_button("✚ New folder").clicked() {
                *new_folder_name = Some(String::new());
                ui.ctx().memory_mut(|m| {
                    m.request_focus(egui::Id::new(("new_folder_input", id_prefix, parent)));
                });
            }
        }
    }
}

fn show_folder_panel(state: &mut GalleryState, ui: &mut egui::Ui) -> Option<GalleryAction> {
    let mut action = None;
    match state.folder_panel_side {
        StripSide::Left => {
            egui::Panel::left("gallery_folders_left")
                .default_size(180.0)
                .size_range(140.0..=320.0)
                .resizable(true)
                .show(ui, |ui| action = folder_panel_contents(state, ui));
        }
        StripSide::Right => {
            egui::Panel::right("gallery_folders_right")
                .default_size(180.0)
                .size_range(140.0..=320.0)
                .resizable(true)
                .show(ui, |ui| action = folder_panel_contents(state, ui));
        }
        StripSide::Top => {
            egui::Panel::top("gallery_folders_top")
                .default_size(112.0)
                .size_range(78.0..=220.0)
                .resizable(true)
                .show(ui, |ui| action = folder_panel_contents(state, ui));
        }
        StripSide::Bottom => {
            egui::Panel::bottom("gallery_folders_bottom")
                .default_size(112.0)
                .size_range(78.0..=220.0)
                .resizable(true)
                .show(ui, |ui| action = folder_panel_contents(state, ui));
        }
    }
    action
}
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
                        action = Some(GalleryAction::RenameFolder(
                            state.folder.clone(),
                            trimmed,
                        ));
                    }
                    state.folder_rename_edit = None;
                }
            } else {
                let btn = egui::Button::new(
                    egui::RichText::new(&current_name)
                        .heading(),
                )
                .fill(egui::Color32::TRANSPARENT);
                if ui.add(btn).clicked() {
                    state.folder_rename_edit =
                        Some((state.folder.clone(), current_name.clone()));
                    ui.ctx().memory_mut(|m| {
                        m.request_focus(egui::Id::new(
                            ("gallery_folder_rename_heading", &state.folder),
                        ));
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
            let columns = gallery_column_count(ui.available_width());
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

fn begin_rename(
    item: &GalleryItem,
    rename_edit: &mut Option<(PathBuf, String)>,
    ctx: &egui::Context,
) {
    let stem = item
        .path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    *rename_edit = Some((item.path.clone(), stem));
    ctx.memory_mut(|memory| {
        memory.request_focus(egui::Id::new(("gallery_rename", item.path.clone())))
    });
}

fn gallery_cell(
    ui: &mut egui::Ui,
    item: &GalleryItem,
    cell_size: egui::Vec2,
    selected: &mut Option<PathBuf>,
    rename_edit: &mut Option<(PathBuf, String)>,
) -> Option<GalleryAction> {
    let (rect, response) = ui.allocate_exact_size(cell_size, egui::Sense::click());
    let is_selected = selected.as_deref() == Some(item.path.as_path());
    let renaming = rename_edit
        .as_ref()
        .is_some_and(|(path, _)| path == &item.path);
    let name_rect = egui::Rect::from_min_size(
        rect.left_top() + egui::vec2(8.0, 4.0),
        egui::vec2(rect.width() - 16.0, TITLE_HEIGHT - 4.0),
    );
    let name_response = (!renaming).then(|| {
        ui.interact(
            name_rect,
            egui::Id::new(("gallery_name", item.path.clone())),
            egui::Sense::click(),
        )
    });

    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if response.hovered() {
            painter.rect_filled(rect, 6.0, ui.visuals().widgets.hovered.weak_bg_fill);
        }
        if is_selected {
            painter.rect_stroke(
                rect,
                6.0,
                egui::Stroke::new(2.0, egui::Color32::from_rgb(0, 122, 255)),
                egui::StrokeKind::Inside,
            );
        }

        let thumbnail_width = rect.width() - THUMB_INSET * 2.0;
        let thumbnail_height = thumbnail_width / THUMB_ASPECT_RATIO;
        let thumb_rect = egui::Rect::from_min_size(
            rect.left_top() + egui::vec2(THUMB_INSET, TITLE_HEIGHT + TITLE_TO_THUMB_GAP),
            egui::vec2(thumbnail_width, thumbnail_height),
        );
        match (&item.tex, item.failed) {
            (Some(tex), _) => {
                let size = tex.size_vec2();
                let scale = (thumbnail_width / size.x).max(thumbnail_height / size.y);
                let fitted = egui::Rect::from_center_size(thumb_rect.center(), size * scale);
                painter.image(
                    tex.id(),
                    fitted,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }
            (None, _) if item.kind == ItemKind::Design => {
                painter.text(
                    thumb_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "Document",
                    egui::FontId::proportional(14.0),
                    ui.visuals().weak_text_color(),
                );
            }
            (None, true) => {
                painter.text(
                    thumb_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "Failed to load",
                    egui::FontId::proportional(14.0),
                    ui.visuals().error_fg_color,
                );
            }
            (None, false) => {
                painter.text(
                    thumb_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "Loading",
                    egui::FontId::proportional(14.0),
                    ui.visuals().weak_text_color(),
                );
            }
        }

        if item.kind == ItemKind::Design {
            painter.text(
                thumb_rect.right_top() + egui::vec2(-2.0, 2.0),
                egui::Align2::RIGHT_TOP,
                "Design",
                egui::FontId::proportional(11.0),
                ui.visuals().weak_text_color(),
            );
        }

        if !renaming {
            let mut name = item.name.clone();
            if name.chars().count() > 30 {
                name = format!("{}...", name.chars().take(27).collect::<String>());
            }
            painter.text(
                name_rect.left_center(),
                egui::Align2::LEFT_CENTER,
                name,
                egui::FontId::proportional(12.5),
                ui.visuals().text_color(),
            );
        }
    }

    let mut action = None;
    let name_clicked = name_response.as_ref().is_some_and(|name| name.clicked());
    if let Some(name_response) = &name_response {
        if name_response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
        }
        if name_response.clicked() {
            begin_rename(item, rename_edit, ui.ctx());
        }
    }

    if renaming {
        let text_id = egui::Id::new(("gallery_rename", item.path.clone()));
        let mut cancel = false;
        let mut commit = false;
        if let Some((_, text)) = rename_edit.as_mut() {
            let edit_response = ui.put(
                name_rect,
                egui::TextEdit::singleline(text)
                    .id(text_id)
                    .horizontal_align(egui::Align::LEFT),
            );
            if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                cancel = true;
            } else if edit_response.lost_focus() {
                commit = true;
            }
        }
        if cancel {
            *rename_edit = None;
        } else if commit {
            if let Some((path, text)) = rename_edit.take() {
                let new_stem = text.trim().to_owned();
                let original_stem = path
                    .file_stem()
                    .map(|stem| stem.to_string_lossy().into_owned())
                    .unwrap_or_default();
                if !new_stem.is_empty() && new_stem != original_stem {
                    action = Some(GalleryAction::Rename(path, new_stem));
                }
            }
        }
    } else {
        if !name_clicked && response.clicked() {
            action = Some(GalleryAction::Open(item.path.clone()));
        }
        if response.secondary_clicked() {
            *selected = Some(item.path.clone());
        }
        response.context_menu(|ui| {
            if ui.button("Open").clicked() {
                action = Some(GalleryAction::Open(item.path.clone()));
                ui.close();
            }
            if ui.button("Rename").clicked() {
                begin_rename(item, rename_edit, ui.ctx());
                ui.close();
            }
            if ui.button("Duplicate").clicked() {
                action = Some(GalleryAction::Duplicate(item.path.clone()));
                ui.close();
            }
            if ui.button("Copy").clicked() {
                *selected = Some(item.path.clone());
                copy_to_slot(item.path.clone());
                ui.close();
            }
            if ui.button("Reveal in Explorer").clicked() {
                reveal_in_explorer(&item.path);
                ui.close();
            }
            ui.separator();
            let delete_label = egui::RichText::new("Delete").color(ui.visuals().warn_fg_color);
            if ui.button(delete_label).clicked() {
                action = Some(GalleryAction::Delete(item.path.clone()));
                ui.close();
            }
        });
    }

    action
}
