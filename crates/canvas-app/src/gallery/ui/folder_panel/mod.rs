//! El panel de carpetas (hermanas y de dentro): la lista de botones, el campo
//! para crear una carpeta nueva, y de que lado de la ventana se ancla.

use std::path::{Path, PathBuf};

use eframe::egui;

use crate::app_icons::{draw_plus_icon, icon_text_button_ui};

use crate::deck::StripSide;

use self::rows::{folder_button_list, folder_name};

use super::super::{GalleryAction, GalleryState};

mod rows;

pub fn next_folder_panel_side(side: StripSide) -> StripSide {
    match side {
        StripSide::Left => StripSide::Bottom,
        StripSide::Bottom => StripSide::Right,
        StripSide::Right => StripSide::Top,
        StripSide::Top => StripSide::Left,
    }
}

/// Buscador del visor de carpetas: caja de texto (con placeholder) y botón
/// de limpiar. El texto vive en la memoria de egui, así que se conserva al
/// navegar entre carpetas dentro de la misma sesión de galería. Devuelve el
/// filtro escrito (puede estar vacío).
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
            .on_hover_text("Clear search")
            .clicked()
        {
            query.clear();
            ui.data_mut(|d| d.insert_temp(filter_id, String::new()));
        }
    });
    query
}

/// Subcarpetas que coinciden con el filtro (subcadena, sin distinguir
/// mayúsculas, sobre el nombre de la carpeta). Filtro vacío = todas.
/// Devuelve una lista PROPIA (clonada), no referencias a `state`: así la
/// lista filtrada no ata un préstamo de `state` dentro del closure que
/// también pide `&mut state` (p. ej. `refresh_folder_lists`).
fn filter_folders(children: &[PathBuf], filter: &str) -> Vec<PathBuf> {
    if filter.is_empty() {
        return children.to_vec();
    }
    children
        .iter()
        .filter(|folder| folder_name(folder).to_lowercase().contains(filter))
        .cloned()
        .collect()
}

fn folder_panel_contents(state: &mut GalleryState, ui: &mut egui::Ui) -> Option<GalleryAction> {
    let mut action = None;
    // Solo macOS consume este valor hoy (botón «Grant access…» del pane
    // de Full Disk Access): en las demás plataformas el flujo de permisos
    // de almacenamiento en la nube todavía no existe.
    #[allow(unused_variables)]
    let cloud_folder = canvas_io::is_cloud_storage_path(&state.folder);
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
                ui.strong(current_name);
                new_folder_ui(
                    ui,
                    "inside",
                    &state.folder,
                    &mut state.new_folder_inside,
                    &mut action,
                );
                ui.add_space(2.0);
                // Buscador: debajo de «New folder», ancho completo.
                let query = folder_search_ui(ui, ui.available_width());
                let visible = filter_folders(&state.folders.children, &query.trim().to_lowercase());
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
                        super::open_full_disk_access_pane();
                        state.note_settings_opened();
                    }
                } else if state.folders.children.is_empty() && state.new_folder_inside.is_none() {
                    ui.weak("No subfolders");
                } else if visible.is_empty() {
                    ui.weak(format!("No folders match \"{}\"", query.trim()));
                } else {
                    folder_button_list(
                        ui,
                        &visible,
                        None,
                        &mut state.folder_rename_edit,
                        &mut action,
                    );
                }
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
        ui.add_space(2.0);
        // Buscador compacto en la tira horizontal.
        let query = folder_search_ui(ui, 180.0);
        let visible = filter_folders(&state.folders.children, &query.trim().to_lowercase());
        egui::ScrollArea::horizontal()
            .id_salt("gallery_child_folders_horizontal")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal(|ui| {
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
                            super::open_full_disk_access_pane();
                            state.note_settings_opened();
                        }
                    } else if state.folders.children.is_empty() && state.new_folder_inside.is_none()
                    {
                        ui.weak("No subfolders");
                    } else if visible.is_empty() {
                        ui.weak(format!("No folders match \"{}\"", query.trim()));
                    } else {
                        folder_button_list(
                            ui,
                            &visible,
                            None,
                            &mut state.folder_rename_edit,
                            &mut action,
                        );
                    }
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
                draw_plus_icon,
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

#[cfg(test)]
mod tests {
    use super::filter_folders;
    use std::path::PathBuf;

    #[test]
    fn filter_matches_folder_name_case_insensitively() {
        let children = vec![
            PathBuf::from("/a/Chismes MX"),
            PathBuf::from("/a/Youtube"),
            PathBuf::from("/a/chismes 2"),
        ];
        let hits = filter_folders(&children, "chismes");
        assert_eq!(hits.len(), 2);
        assert!(hits.iter().all(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.to_lowercase().contains("chismes"))
        }));
    }

    #[test]
    fn filter_empty_returns_all() {
        let children = vec![PathBuf::from("/a/x"), PathBuf::from("/a/y")];
        assert_eq!(filter_folders(&children, "").len(), 2);
    }

    #[test]
    fn filter_no_match_returns_empty() {
        let children = vec![PathBuf::from("/a/x")];
        assert!(filter_folders(&children, "zzz").is_empty());
    }
}
