//! El panel de carpetas (hermanas y de dentro): la lista de botones, el campo
//! para crear una carpeta nueva, y de que lado de la ventana se ancla.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use eframe::egui;

use crate::app_icons::{draw_folder_icon, draw_plus_icon, icon_text_button_ui};

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

/// Una fila de carpeta estilizada: icono de carpeta + nombre, fondo al
/// hover, doble clic para renombrar, menú contextual con Rename y Delete.
fn gallery_folder_row_ui(
    ui: &mut egui::Ui,
    path: &Path,
    name: &str,
    is_current: bool,
    rename_edit: &mut Option<(PathBuf, String)>,
) -> Option<GalleryAction> {
    let visuals = ui.visuals().clone();
    let font = egui::FontId::proportional(13.0);
    let icon_sz = 14.0;
    let pad_y = 5.0;
    let gap = 8.0;
    let left_pad = 6.0;

    let galley = ui
        .painter()
        .layout_no_wrap(name.to_owned(), font.clone(), visuals.text_color());
    let height = (galley.size().y + pad_y * 2.0).max(26.0);
    let width = ui.available_width().max(1.0);

    let (row_rect, row_resp) =
        ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());
    let hovered = row_resp.hovered();

    // Fondo al hover
    if hovered {
        ui.painter()
            .rect_filled(row_rect, 4.0, visuals.widgets.hovered.weak_bg_fill);
    }

    let color = if is_current {
        visuals.strong_text_color()
    } else if hovered {
        visuals.widgets.active.text_color()
    } else {
        visuals.text_color()
    };

    // ── Icono carpeta + nombre ──
    let content_start_x = row_rect.left() + left_pad;
    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(content_start_x + icon_sz / 2.0, row_rect.center().y),
        egui::vec2(icon_sz, icon_sz),
    );
    draw_folder_icon(ui.painter(), icon_rect, color);
    let text_origin = egui::pos2(icon_rect.right() + gap, row_rect.center().y);
    ui.painter()
        .text(text_origin, egui::Align2::LEFT_CENTER, name, font, color);

    row_resp.clone().on_hover_text(path.display().to_string());

    // ── Clic ──
    if row_resp.clicked() && !is_current {
        return Some(GalleryAction::OpenFolder(path.to_owned()));
    }

    // ── Doble clic → renombrar ──
    if row_resp.double_clicked() {
        let stem = folder_name(path);
        *rename_edit = Some((path.to_owned(), stem));
        ui.ctx().memory_mut(|m| {
            m.request_focus(egui::Id::new(("folder_rename", path.as_os_str())));
        });
    }

    // ── Menú contextual ──
    let context_action = Rc::new(RefCell::new(None));
    let path_clone = path.to_owned();
    {
        let context_action = context_action.clone();
        row_resp.context_menu(move |ui| {
            if ui.button("Rename").clicked() {
                let stem = folder_name(&path_clone);
                *context_action.borrow_mut() = Some((path_clone.clone(), stem));
                ui.close();
            }
            let del_label = egui::RichText::new("Delete").color(ui.visuals().warn_fg_color);
            if ui.button(del_label).clicked() {
                *context_action.borrow_mut() = Some((path_clone.clone(), "__DELETE__".to_owned()));
                ui.close();
            }
        });
    }

    if let Some((p, extra)) = context_action.take() {
        if extra == "__DELETE__" {
            let fname = folder_name(&p);
            let confirmed = rfd::MessageDialog::new()
                .set_level(rfd::MessageLevel::Warning)
                .set_title("Delete folder")
                .set_description(format!(
                    "Move \"{fname}\" to the trash?\nThis deletes all files inside it.",
                ))
                .set_buttons(rfd::MessageButtons::OkCancel)
                .show();
            if confirmed == rfd::MessageDialogResult::Ok {
                return Some(GalleryAction::DeleteFolder(p));
            }
        } else {
            *rename_edit = Some((p, extra));
            ui.ctx().memory_mut(|m| {
                m.request_focus(egui::Id::new(("folder_rename", path.as_os_str())));
            });
        }
    }

    None
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
        } else {
            let is_current = current == Some(folder.as_path());
            if let Some(a) = gallery_folder_row_ui(ui, folder, &name, is_current, rename_edit) {
                *action = Some(a);
            }
        }
    }
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
