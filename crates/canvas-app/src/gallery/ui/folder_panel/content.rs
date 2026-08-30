//! Contenido común de los paneles de carpetas vertical y horizontal.

use std::path::{Path, PathBuf};

use eframe::egui;

use super::rows::{folder_button_list, folder_name};
use super::{filter_folders, GalleryAction, GalleryState};
use crate::app_icons::{draw_plus_icon, icon_text_button_ui};

pub(super) fn folder_panel_contents(
    state: &mut GalleryState,
    ui: &mut egui::Ui,
) -> Option<GalleryAction> {
    let mut action = None;
    #[allow(unused_variables)]
    let cloud_folder = canvas_io::is_cloud_storage_path(&state.folder);
    draw_navigation(ui, state, &mut action);
    let current_name = folder_name(&state.folder);
    if state.folder_panel_side.is_vertical_flow() {
        egui::ScrollArea::vertical()
            .id_salt("gallery_folder_tree_vertical")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.strong(&current_name);
                new_folder_ui(
                    ui,
                    "inside",
                    &state.folder,
                    &mut state.new_folder_inside,
                    &mut action,
                );
                draw_folder_list(ui, state, cloud_folder, &mut action);
            });
    } else {
        ui.strong(current_name);
        new_folder_ui(
            ui,
            "inside",
            &state.folder,
            &mut state.new_folder_inside,
            &mut action,
        );
        let query = folder_search_ui(ui, 180.0);
        let visible = filter_folders(&state.folders.children, &query.trim().to_lowercase());
        egui::ScrollArea::horizontal()
            .id_salt("gallery_child_folders_horizontal")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    draw_visible_folders(ui, state, cloud_folder, &visible, &mut action)
                });
            });
    }
    action
}

fn draw_navigation(
    ui: &mut egui::Ui,
    state: &mut GalleryState,
    action: &mut Option<GalleryAction>,
) {
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
                super::next_folder_panel_side(state.folder_panel_side).label()
            ))
            .clicked()
        {
            *action = Some(GalleryAction::CycleFolderPanelSide);
        }
    });
    ui.horizontal_wrapped(|ui| {
        if ui
            .add_enabled(state.navigation.can_back(), egui::Button::new("<"))
            .clicked()
        {
            *action = Some(GalleryAction::Back);
        }
        if ui
            .add_enabled(state.navigation.can_forward(), egui::Button::new(">"))
            .clicked()
        {
            *action = Some(GalleryAction::Forward);
        }
        if let Some(parent) = state.folder.parent() {
            if ui
                .small_button("Parent")
                .on_hover_text("Open parent folder (Alt+Up)")
                .clicked()
            {
                *action = Some(GalleryAction::OpenFolder(parent.to_owned()));
            }
        }
    });
    ui.separator();
}

fn draw_folder_list(
    ui: &mut egui::Ui,
    state: &mut GalleryState,
    cloud_folder: bool,
    action: &mut Option<GalleryAction>,
) {
    let query = folder_search_ui(ui, ui.available_width());
    let visible = filter_folders(&state.folders.children, &query.trim().to_lowercase());
    draw_visible_folders(ui, state, cloud_folder, &visible, action);
}

/// `cloud_folder` solo se usa en macOS (botón «Grant access…»); en el
/// resto de plataformas es un parámetro deliberadamente sin usar.
#[cfg_attr(not(target_os = "macos"), allow(unused_variables))]
fn draw_visible_folders(
    ui: &mut egui::Ui,
    state: &mut GalleryState,
    cloud_folder: bool,
    visible: &[PathBuf],
    action: &mut Option<GalleryAction>,
) {
    if let Some(error) = &state.folders.read_error {
        ui.colored_label(ui.visuals().warn_fg_color, "Could not list folders.")
            .on_hover_text(error.clone());
        if ui
            .small_button("Retry")
            .on_hover_text("List this folder's subfolders again.")
            .clicked()
        {
            state.refresh_folder_lists();
        }
        #[cfg(target_os = "macos")]
        if cloud_folder && ui.small_button("Grant access…").clicked() {
            super::super::shell::open_full_disk_access_pane();
            state.note_settings_opened();
        }
    } else if state.folders.children.is_empty() && state.new_folder_inside.is_none() {
        ui.weak("No subfolders");
    } else if visible.is_empty() {
        ui.weak(format!("No folders match \"{}\"", ""));
    } else {
        folder_button_list(ui, visible, None, &mut state.folder_rename_edit, action);
    }
}

fn folder_search_ui(ui: &mut egui::Ui, width: f32) -> String {
    let filter_id = egui::Id::new("gallery_folder_filter");
    let mut query: String = ui.data_mut(|d| d.get_temp(filter_id).unwrap_or_default());
    ui.horizontal(|ui| {
        let response = ui.add_sized(
            [width, 20.0],
            egui::TextEdit::singleline(&mut query)
                .id(egui::Id::new(("gallery_folder_filter", "input")))
                .hint_text("Search folders…"),
        );
        if response.changed() {
            ui.data_mut(|d| d.insert_temp(filter_id, query.clone()));
        }
        let has_query = !query.trim().is_empty();
        if ui
            .add_enabled(has_query, egui::Button::new("✕").small())
            .clicked()
        {
            query.clear();
            ui.data_mut(|d| d.insert_temp(filter_id, String::new()));
        }
    });
    query
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
                draw_plus_icon,
                "New folder",
                None,
                egui::Vec2::ZERO,
            )
            .clicked()
            {
                *new_folder_name = Some(String::new());
                ui.ctx().memory_mut(|m| {
                    m.request_focus(egui::Id::new(("new_folder_input", id_prefix, parent)))
                });
            }
        }
    }
}
