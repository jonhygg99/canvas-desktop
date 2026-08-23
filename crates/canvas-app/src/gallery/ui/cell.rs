//! Una celda de la cuadricula: la miniatura, el titulo, el renombrado
//! in-place y el menu contextual. Y la celda «+» del final, que crea un lienzo
//! nuevo.

use std::path::PathBuf;

use eframe::egui;

use super::shell::reveal_in_explorer;

use super::super::{copy_to_slot, GalleryAction, GalleryItem, ItemKind};

pub(super) const CELL_GAP: f32 = 8.0;

pub(super) const ROW_GAP: f32 = 4.0;

const THUMB_INSET: f32 = 8.0;

const THUMB_ASPECT_RATIO: f32 = 16.0 / 9.0;

const TITLE_HEIGHT: f32 = 20.0;

const TITLE_TO_THUMB_GAP: f32 = 2.0;

const CARD_BOTTOM_PADDING: f32 = 6.0;

/// Styled "✚" cell for creating a new blank canvas, inserted at the
/// end of the gallery grid. Its title and thumbnail area match regular
/// gallery pages.
pub(super) fn gallery_add_cell(ui: &mut egui::Ui, cell_size: egui::Vec2) -> bool {
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

pub(in crate::gallery) fn gallery_cell_size(available_width: f32, columns: usize) -> egui::Vec2 {
    let columns = columns.max(1);
    let width = ((available_width - CELL_GAP * (columns.saturating_sub(1) as f32))
        / columns as f32)
        .max(1.0);
    let thumbnail_height = (width - THUMB_INSET * 2.0) / THUMB_ASPECT_RATIO;
    egui::vec2(
        width,
        TITLE_HEIGHT + TITLE_TO_THUMB_GAP + thumbnail_height + CARD_BOTTOM_PADDING,
    )
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

pub(super) fn gallery_cell(
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
