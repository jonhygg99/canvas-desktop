//! El panel de carpetas (hermanas y de dentro): la lista de botones, el campo
//! para crear una carpeta nueva, y de que lado de la ventana se ancla.

use std::path::{Path, PathBuf};

use eframe::egui;

use crate::app_icons::{draw_plus_icon, icon_text_button_ui};

use crate::deck::StripSide;

use super::super::{GalleryAction, GalleryState};

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
        let rename_this = rename_edit.as_ref().is_some_and(|(p, _)| p == folder);
        if rename_this {
            let Some((_, text)) = rename_edit.as_mut() else {
                continue;
            };
            let id = egui::Id::new(("folder_rename", folder.as_os_str()));
            let response = ui.add(egui::TextEdit::singleline(text).id(id).desired_width(140.0));
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
                egui::Label::new(egui::RichText::new(&name).strong()).sense(egui::Sense::click()),
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
                new_folder_ui(
                    ui,
                    "inside",
                    &state.folder,
                    &mut state.new_folder_inside,
                    &mut action,
                );
                ui.add_space(2.0);
                if state.folders.children.is_empty() && state.new_folder_inside.is_none() {
                    ui.weak("No subfolders");
                } else if !state.folders.children.is_empty() {
                    folder_button_list(
                        ui,
                        &state.folders.children,
                        None,
                        &mut state.folder_rename_edit,
                        &mut action,
                    );
                }
                ui.add_space(8.0);
                ui.separator();
                ui.strong("Siblings");
                if let Some(parent) = state.folder.parent() {
                    new_folder_ui(
                        ui,
                        "sibling",
                        parent,
                        &mut state.new_folder_sibling,
                        &mut action,
                    );
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
        new_folder_ui(
            ui,
            "inside",
            &state.folder,
            &mut state.new_folder_inside,
            &mut action,
        );
        ui.add_space(2.0);
        egui::ScrollArea::horizontal()
            .id_salt("gallery_child_folders_horizontal")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if state.folders.children.is_empty() && state.new_folder_inside.is_none() {
                        ui.weak("No subfolders");
                    } else if !state.folders.children.is_empty() {
                        folder_button_list(
                            ui,
                            &state.folders.children,
                            None,
                            &mut state.folder_rename_edit,
                            &mut action,
                        );
                    }
                });
            });
        ui.separator();
        ui.strong("Siblings");
        if let Some(parent) = state.folder.parent() {
            new_folder_ui(
                ui,
                "sibling",
                parent,
                &mut state.new_folder_sibling,
                &mut action,
            );
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
            if icon_text_button_ui(
                ui,
                true,
                |p, r, c| draw_plus_icon(p, r, c),
                "New folder",
                None,
                egui::Vec2::ZERO,
            )
            .clicked()
            {
                *new_folder_name = Some(String::new());
                ui.ctx().memory_mut(|m| {
                    m.request_focus(egui::Id::new(("new_folder_input", id_prefix, parent)));
                });
            }
        }
    }
}

pub(super) fn show_folder_panel(
    state: &mut GalleryState,
    ui: &mut egui::Ui,
) -> Option<GalleryAction> {
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
