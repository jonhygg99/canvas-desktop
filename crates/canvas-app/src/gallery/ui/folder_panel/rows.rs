//! Filas de la lista de carpetas del panel: botón-fila con icono de
//! carpeta, doble clic para renombrar y menú contextual con Rename y
//! Delete (con confirmación nativa). El layout del panel vive en `mod.rs`.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use eframe::egui;

use crate::app_icons::draw_folder_icon;
use crate::gallery::GalleryAction;

pub(super) fn folder_name(folder: &Path) -> String {
    folder
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| folder.display().to_string())
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

pub(super) fn folder_button_list(
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
