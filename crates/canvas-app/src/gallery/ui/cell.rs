//! Una celda de la cuadricula: la miniatura, el titulo, el renombrado
//! in-place y el menu contextual. Y la celda «+» del final, que crea un lienzo
//! nuevo.

use std::path::PathBuf;

use eframe::egui;

use super::shell::reveal_in_explorer;

use super::super::{copy_to_slot, GalleryAction, GalleryItem};
use crate::app_icons::draw_plus_icon;

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
    // Id estable: sin él, la celda «+» comparte la secuencia de auto-ids con
    // la cuadrícula y un banner/renombrado intercalado desplaza el reparto.
    let (rect, response) = ui
        .push_id(egui::Id::new("gallery_add_cell"), |ui| {
            ui.allocate_exact_size(cell_size, egui::Sense::click())
        })
        .inner;
    if !ui.is_rect_visible(rect) {
        return false;
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
    let plus_size = (thumbnail_height * 0.4).max(20.0);
    draw_plus_icon(
        painter,
        egui::Rect::from_center_size(add_rect.center(), egui::vec2(plus_size, plus_size)),
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

pub(super) fn begin_rename(
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

#[cfg(test)]
pub(in crate::gallery) fn gallery_cell(
    ui: &mut egui::Ui,
    item: &GalleryItem,
    cell_size: egui::Vec2,
    selected: &mut Option<PathBuf>,
    rename_edit: &mut Option<(PathBuf, String)>,
) -> Option<GalleryAction> {
    // Id estable por ruta: con auto-ids, cualquier widget condicional pintado
    // antes (banner de error, renombrado in-place, toolbar) desplaza la
    // secuencia entre el frame del press y el del release y egui atribuye el
    // `clicked()` a otra celda (salto de varias filas, intermitente).
    ui.push_id(egui::Id::new(("gallery_cell", &item.path)), |ui| {
        gallery_cell_inner(ui, item, cell_size, selected, rename_edit, None)
    })
    .inner
}

pub(in crate::gallery) fn gallery_cell_with_framing(
    ui: &mut egui::Ui,
    item: &GalleryItem,
    cell_size: egui::Vec2,
    selected: &mut Option<PathBuf>,
    rename_edit: &mut Option<(PathBuf, String)>,
    framings: &mut super::super::framing::GalleryFramings,
) -> Option<GalleryAction> {
    ui.push_id(egui::Id::new(("gallery_cell", &item.path)), |ui| {
        gallery_cell_inner(ui, item, cell_size, selected, rename_edit, Some(framings))
    })
    .inner
}

fn gallery_cell_inner(
    ui: &mut egui::Ui,
    item: &GalleryItem,
    cell_size: egui::Vec2,
    selected: &mut Option<PathBuf>,
    rename_edit: &mut Option<(PathBuf, String)>,
    mut framings: Option<&mut super::super::framing::GalleryFramings>,
) -> Option<GalleryAction> {
    let (rect, response) = ui.allocate_exact_size(cell_size, egui::Sense::click());
    // Fuera del viewport no hay click posible: solo se reservó espacio para
    // el scroll. Sin este corte, una celda invisible podía emitir `Open`.
    if !ui.is_rect_visible(rect) {
        return None;
    }
    let is_selected = selected.as_deref() == Some(item.path.as_path());
    let renaming = rename_edit
        .as_ref()
        .is_some_and(|(path, _)| path == &item.path);
    let name_rect = egui::Rect::from_min_size(
        rect.left_top() + egui::vec2(8.0, 4.0),
        egui::vec2(rect.width() - 16.0, TITLE_HEIGHT - 4.0),
    );
    // Aquí la celda es visible seguro (corte al inicio): se pinta siempre.
    let painter = ui.painter();
    let vertical = framings.as_ref().is_some_and(|state| state.vertical);
    let card = framings
        .as_mut()
        .and_then(|state| state.card(&item.path, ui.ctx(), vertical));
    let saved = card.is_some_and(|card| card.saved.is_some());
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
    let thumbnail_height = if vertical {
        thumbnail_width * 16.0 / 9.0
    } else {
        thumbnail_width / THUMB_ASPECT_RATIO
    };
    let thumb_rect = egui::Rect::from_min_size(
        rect.left_top() + egui::vec2(THUMB_INSET, TITLE_HEIGHT + TITLE_TO_THUMB_GAP),
        egui::vec2(thumbnail_width, thumbnail_height),
    );
    super::thumbnail::paint(painter, item, thumb_rect, card, vertical, ui.visuals());

    if !renaming {
        let mut name_pos = name_rect.left_center();
        if saved {
            let icon_rect = egui::Rect::from_center_size(
                name_pos + egui::vec2(5.0, 0.0),
                egui::vec2(10.0, 16.0),
            );
            crate::framing::portrait_icon(painter, icon_rect, ui.visuals().selection.stroke.color);
            name_pos.x += 16.0;
            painter.text(
                name_pos,
                egui::Align2::LEFT_CENTER,
                "9:16",
                egui::FontId::proportional(10.0),
                ui.visuals().weak_text_color(),
            );
            name_pos.x += 30.0;
        }
        let mut name = item.name.clone();
        if name.chars().count() > 30 {
            name = format!("{}...", name.chars().take(27).collect::<String>());
        }
        painter.text(
            name_pos,
            egui::Align2::LEFT_CENTER,
            name,
            egui::FontId::proportional(12.5),
            ui.visuals().text_color(),
        );
    }

    let mut action = None;
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
        // `!dragged()`: un press que termina en scroll/drag no es un click,
        // aunque el release caiga sobre otra celda varias filas más abajo.
        if response.clicked() && !response.dragged() {
            *selected = Some(item.path.clone());
        }
        if response.double_clicked() && !response.dragged() {
            tracing::debug!(path = %item.path.display(), "gallery open click");
            action = Some(GalleryAction::Open(item.path.clone()));
        }
        if response.secondary_clicked() {
            *selected = Some(item.path.clone());
        }
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::SelectableLabel,
                true,
                is_selected,
                &item.name,
            )
        });
        response.context_menu(|ui| {
            if ui
                .button(if saved {
                    "Edit framing 9:16"
                } else {
                    "Create framing 9:16"
                })
                .clicked()
            {
                action = Some(GalleryAction::EditFraming(item.path.clone()));
                ui.close();
            }
            if let Some(error) = card.and_then(|card| card.error.as_ref()) {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            ui.separator();
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
