//! Tira lateral de miniaturas: todos los lienzos de la carpeta abierta, para
//! saltar de uno a otro sin volver a la galería. Reutiliza el patrón de
//! `gallery.rs::gallery_cell` (miniatura con aspecto preservado, culling
//! fuera de pantalla, glifos de estado) a una escala más pequeña.

use std::path::PathBuf;

use eframe::egui;

use crate::deck::{Deck, DeckAxis, Slot, SlotContent};
use crate::gallery::ItemKind;

/// Acción pedida desde la tira: la app decide si hace falta preguntar por
/// cambios sin guardar antes de saltar, o (para `ToggleAxis`/`CycleSide`)
/// persistir el nuevo valor en los ajustes — la tira no conoce
/// `AppSettings`.
pub enum StripAction {
    Open(PathBuf),
    ToggleAxis,
    /// Mueve la tira al siguiente lado (Left→Top→Right→Bottom→Left).
    CycleSide,
    /// Añade un lienzo en blanco al final de la baraja (celda "+").
    AddCanvas,
}

/// Lado mínimo de una celda: por debajo, la miniatura deja de leerse.
const CELL_MIN: f32 = 64.0;
/// Lado máximo: por encima, la tira compite con el propio lienzo. Un poco
/// por encima del ancho por defecto del panel "layers" (220 px, `main.rs`).
const CELL_MAX: f32 = 240.0;
/// Hueco lateral alrededor de la miniatura dentro de la celda.
const CELL_PAD: f32 = 8.0;
/// Gallery uses a 16:9 thumbnail area for every design card.
const THUMB_ASPECT_RATIO: f32 = 16.0 / 9.0;
/// Alto reservado para el nombre, colocado encima de la miniatura como en Gallery.
const LABEL_H: f32 = 20.0;
const TITLE_TO_THUMB_GAP: f32 = 2.0;
const CELL_BOTTOM_PAD: f32 = 6.0;

/// Tamaño de celda calculado contra el espacio real del panel, no una
/// constante — es lo que hace que arrastrar el borde de la tira agrande de
/// verdad las miniaturas.
struct StripMetrics {
    cell: egui::Vec2,
    thumb: egui::Vec2,
}

fn strip_cell_metrics(across: f32, vertical_flow: bool) -> StripMetrics {
    if vertical_flow {
        let thumb_width = (across - 2.0 * CELL_PAD).max(1.0);
        let thumb_height = thumb_width / THUMB_ASPECT_RATIO;
        StripMetrics {
            cell: egui::vec2(
                across,
                LABEL_H + TITLE_TO_THUMB_GAP + thumb_height + CELL_BOTTOM_PAD,
            ),
            thumb: egui::vec2(thumb_width, thumb_height),
        }
    } else {
        let thumb_height = (across - LABEL_H - TITLE_TO_THUMB_GAP - CELL_BOTTOM_PAD).max(1.0);
        let thumb_width = thumb_height * THUMB_ASPECT_RATIO;
        StripMetrics {
            cell: egui::vec2(thumb_width + 2.0 * CELL_PAD, across),
            thumb: egui::vec2(thumb_width, thumb_height),
        }
    }
}

/// Dimensiona la celda contra el espacio que el panel tiene DE VERDAD.
/// La dimensión que manda es la TRANSVERSAL al flujo de scroll — el ancho
/// en una tira en columna (Left/Right), el alto en una en fila (Top/Bottom)
/// —, porque la otra es la que hace scroll y es efectivamente infinita.
fn strip_metrics(ui: &egui::Ui, vertical_flow: bool) -> StripMetrics {
    let bar = ui.spacing().scroll.bar_width + ui.spacing().scroll.bar_inner_margin;
    if vertical_flow {
        let across = (ui.available_width() - bar).clamp(CELL_MIN, CELL_MAX);
        strip_cell_metrics(across, true)
    } else {
        let min_across =
            LABEL_H + TITLE_TO_THUMB_GAP + CELL_MIN / THUMB_ASPECT_RATIO + CELL_BOTTOM_PAD;
        let across = (ui.available_height() - bar).clamp(min_across, CELL_MAX);
        strip_cell_metrics(across, false)
    }
}

/// `active_dirty`: el estado sucio del lienzo activo vive en
/// `EditorState::history`, no en su ranura (`SlotContent::Active` es solo
/// un marcador — su contenido real está prestado fuera de la baraja), así
/// que hace falta que el llamador lo traiga.
pub fn deck_strip_ui(
    deck: &mut Deck,
    active_dirty: bool,
    ui: &mut egui::Ui,
) -> Option<StripAction> {
    let mut action = None;
    let axis = deck.axis;
    let side = deck.strip_side;
    ui.add_space(6.0);
    // `horizontal_wrapped`, no `horizontal`: a 96 px (mínimo de una tira
    // Left/Right) un contador más dos botones no cabe en una sola línea. Con
    // el ajuste envuelve en dos líneas cortas; en cuanto el usuario
    // ensancha el panel (ya posible, ver `strip_metrics`) se juntan en una.
    ui.horizontal_wrapped(|ui| {
        ui.weak(format!("{} / {}", deck.active + 1, deck.slots.len()));
        let (icon, hover) = match axis {
            DeckAxis::Vertical => ("⇅", "Switch to horizontal layout"),
            DeckAxis::Horizontal => ("⇆", "Switch to vertical layout"),
        };
        if ui.small_button(icon).on_hover_text(hover).clicked() {
            action = Some(StripAction::ToggleAxis);
        }
        if ui
            .small_button(side.glyph())
            .on_hover_text(format!("Canvases panel: {} (click to move)", side.label()))
            .clicked()
        {
            action = Some(StripAction::CycleSide);
        }
    });
    ui.separator();
    let active_idx = deck.active;
    let cell_dirty = |idx: usize, slot: &Slot| -> bool {
        if idx == active_idx {
            active_dirty
        } else {
            matches!(&slot.content, SlotContent::Ready(d) if d.history.is_dirty())
        }
    };
    let vertical_flow = side.is_vertical_flow();
    let can_add = deck.folder.is_some();
    if vertical_flow {
        // `auto_shrink([false, false])`: sin esto, la `ScrollArea` se
        // encoge al tamaño de su CONTENIDO en el eje transversal (el ancho,
        // aquí) — y ES ESE tamaño encogido el que `egui::Panel` vuelve a
        // persistir como "el tamaño del panel" en el siguiente frame
        // (persiste el rect que el contenido ocupó, no el del arrastre).
        // En la tira vertical esto no se notaba porque `ui.separator()` de
        // más abajo ya reclamaba el ancho completo por su cuenta; se pone
        // aquí también para que el comportamiento no dependa de ese efecto
        // colateral y sea simétrico con la rama horizontal de abajo.
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // Match Gallery's row rhythm instead of egui's larger default.
                ui.spacing_mut().item_spacing.y = 4.0;
                let m = strip_metrics(ui, true);
                for (idx, slot) in deck.slots.iter().enumerate() {
                    let dirty = cell_dirty(idx, slot);
                    if let Some(a) = strip_cell(ui, slot, &m, idx == active_idx, dirty) {
                        action = Some(a);
                    }
                }
                if can_add && strip_add_cell(ui, &m) {
                    action = Some(StripAction::AddCanvas);
                }
            });
    } else {
        // Ver el comentario de la rama vertical: aquí es donde el fallo
        // era visible — sin `auto_shrink([false, false])` la tira se
        // encogía al alto de su contenido (menos el margen de la barra de
        // scroll) cada frame, y ese valor decreciente es justo el que se
        // persistía como "el alto del panel" — arrastrar el borde parecía
        // funcionar un instante y luego se deshacía solo.
        egui::ScrollArea::horizontal()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let m = strip_metrics(ui, false);
                ui.horizontal(|ui| {
                    for (idx, slot) in deck.slots.iter().enumerate() {
                        let dirty = cell_dirty(idx, slot);
                        if let Some(a) = strip_cell(ui, slot, &m, idx == active_idx, dirty) {
                            action = Some(a);
                        }
                    }
                    if can_add && strip_add_cell(ui, &m) {
                        action = Some(StripAction::AddCanvas);
                    }
                });
            });
    }
    action
}

/// Celda "+" al final de la tira: añade un lienzo en blanco. Reutiliza la
/// misma disciplina de asignación y culling que `strip_cell` para que
/// participe del scroll exactamente igual que cualquier otra celda.
fn strip_add_cell(ui: &mut egui::Ui, m: &StripMetrics) -> bool {
    let (rect, response) = ui.allocate_exact_size(m.cell, egui::Sense::click());
    if !ui.is_rect_visible(rect) {
        return response.clicked();
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let painter = ui.painter();
    painter.rect_stroke(
        rect.shrink(2.0),
        4.0,
        egui::Stroke::new(1.0, ui.visuals().weak_text_color()),
        egui::StrokeKind::Inside,
    );
    let name_rect = egui::Rect::from_min_size(
        rect.left_top() + egui::vec2(CELL_PAD, 4.0),
        egui::vec2(rect.width() - 2.0 * CELL_PAD, LABEL_H - 4.0),
    );
    painter.text(
        name_rect.left_center(),
        egui::Align2::LEFT_CENTER,
        "New canvas",
        egui::FontId::proportional(12.5),
        ui.visuals().text_color(),
    );
    let thumb_rect = egui::Rect::from_min_size(
        rect.left_top() + egui::vec2(CELL_PAD, LABEL_H + TITLE_TO_THUMB_GAP),
        m.thumb,
    );
    painter.rect_stroke(
        thumb_rect,
        4.0,
        egui::Stroke::new(1.0, ui.visuals().weak_text_color()),
        egui::StrokeKind::Inside,
    );
    let plus_size = (m.thumb.y * 0.4).max(14.0);
    painter.text(
        thumb_rect.center(),
        egui::Align2::CENTER_CENTER,
        "✚",
        egui::FontId::proportional(plus_size),
        ui.visuals().weak_text_color(),
    );
    response
        .on_hover_text("Add a blank canvas to this folder")
        .clicked()
}

fn strip_cell(
    ui: &mut egui::Ui,
    slot: &Slot,
    m: &StripMetrics,
    is_active: bool,
    dirty: bool,
) -> Option<StripAction> {
    let (rect, response) = ui.allocate_exact_size(m.cell, egui::Sense::click());
    // Fuera del scroll: nada que pintar, pero el clic sigue siendo válido
    // (misma disciplina que `gallery::gallery_cell`).
    if !ui.is_rect_visible(rect) {
        return response
            .clicked()
            .then(|| StripAction::Open(slot.path.clone()));
    }

    let painter = ui.painter();
    if is_active {
        painter.rect_stroke(
            rect,
            4.0,
            egui::Stroke::new(2.0, egui::Color32::from_rgb(0, 122, 255)),
            egui::StrokeKind::Inside,
        );
    } else if response.hovered() {
        painter.rect_filled(rect, 4.0, ui.visuals().widgets.hovered.weak_bg_fill);
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    let name_rect = egui::Rect::from_min_size(
        rect.left_top() + egui::vec2(CELL_PAD, 4.0),
        egui::vec2(rect.width() - 2.0 * CELL_PAD, LABEL_H - 4.0),
    );
    let mut name = slot.name.clone();
    let max_chars = ((rect.width() / 7.0) as usize).max(8);
    if name.chars().count() > max_chars {
        name = format!(
            "{}…",
            name.chars()
                .take(max_chars.saturating_sub(1))
                .collect::<String>()
        );
    }
    painter.text(
        name_rect.left_center(),
        egui::Align2::LEFT_CENTER,
        name,
        egui::FontId::proportional(12.5),
        ui.visuals().text_color(),
    );

    let thumb_rect = egui::Rect::from_min_size(
        egui::pos2(
            rect.left() + CELL_PAD,
            rect.top() + LABEL_H + TITLE_TO_THUMB_GAP,
        ),
        m.thumb,
    );
    let glyph_size = (m.thumb.y * 0.34).max(12.0);
    match (&slot.thumb, slot.thumb_failed) {
        (Some(tex), _) => {
            let size = tex.size_vec2();
            let scale = (m.thumb.x / size.x).max(m.thumb.y / size.y);
            let fitted = egui::Rect::from_center_size(thumb_rect.center(), size * scale);
            painter.image(
                tex.id(),
                fitted,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
        (None, _) if slot.kind == ItemKind::Design => {
            painter.text(
                thumb_rect.center(),
                egui::Align2::CENTER_CENTER,
                "🖹",
                egui::FontId::proportional(glyph_size),
                ui.visuals().weak_text_color(),
            );
        }
        (None, true) => {
            painter.text(
                thumb_rect.center(),
                egui::Align2::CENTER_CENTER,
                "⚠",
                egui::FontId::proportional(glyph_size),
                ui.visuals().error_fg_color,
            );
        }
        (None, false) => {
            painter.text(
                thumb_rect.center(),
                egui::Align2::CENTER_CENTER,
                "⏳",
                egui::FontId::proportional(glyph_size * 0.8),
                ui.visuals().weak_text_color(),
            );
        }
    }

    // Punto de acento: cambios sin guardar en esta ranura.
    if dirty {
        painter.circle_filled(
            rect.right_top() + egui::vec2(-7.0, 7.0),
            3.0,
            egui::Color32::from_rgb(0, 122, 255),
        );
    }

    let hover = if dirty {
        format!("{} (unsaved changes)", slot.name)
    } else {
        slot.name.clone()
    };
    let response = response.on_hover_text(hover);
    response
        .clicked()
        .then(|| StripAction::Open(slot.path.clone()))
}

#[cfg(test)]
mod tests {
    use super::{
        strip_cell_metrics, CELL_BOTTOM_PAD, LABEL_H, THUMB_ASPECT_RATIO, TITLE_TO_THUMB_GAP,
    };

    #[test]
    fn strip_thumbnail_matches_gallery_aspect_ratio() {
        let metrics = strip_cell_metrics(200.0, true);
        let ratio = metrics.thumb.x / metrics.thumb.y;
        assert!((ratio - THUMB_ASPECT_RATIO).abs() < 0.001);
        assert!(
            (metrics.cell.y - (LABEL_H + TITLE_TO_THUMB_GAP + metrics.thumb.y + CELL_BOTTOM_PAD))
                .abs()
                < 0.001
        );
    }

    #[test]
    fn horizontal_strip_keeps_compact_gallery_geometry() {
        let metrics = strip_cell_metrics(120.0, false);
        let ratio = metrics.thumb.x / metrics.thumb.y;
        assert!((ratio - THUMB_ASPECT_RATIO).abs() < 0.001);
        assert!((metrics.cell.y - 120.0).abs() < 0.001);
        assert!((metrics.cell.x - (metrics.thumb.x + 16.0)).abs() < 0.001);
    }
}
