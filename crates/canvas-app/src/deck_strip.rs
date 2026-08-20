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
/// Hueco alrededor de la miniatura dentro de la celda.
const CELL_PAD: f32 = 8.0;
/// Alto reservado para el nombre del archivo debajo de la miniatura.
const LABEL_H: f32 = 18.0;

/// Tamaño de celda calculado contra el espacio real del panel, no una
/// constante — es lo que hace que arrastrar el borde de la tira agrande de
/// verdad las miniaturas.
struct StripMetrics {
    cell: egui::Vec2,
    thumb: f32,
}

/// Dimensiona la celda contra el espacio que el panel tiene DE VERDAD.
/// La dimensión que manda es la TRANSVERSAL al flujo de scroll — el ancho
/// en una tira en columna (Left/Right), el alto en una en fila (Top/Bottom)
/// —, porque la otra es la que hace scroll y es efectivamente infinita.
fn strip_metrics(ui: &egui::Ui, vertical_flow: bool) -> StripMetrics {
    let bar = ui.spacing().scroll.bar_width + ui.spacing().scroll.bar_inner_margin;
    if vertical_flow {
        let across = (ui.available_width() - bar).clamp(CELL_MIN, CELL_MAX);
        let thumb = across - 2.0 * CELL_PAD;
        StripMetrics {
            cell: egui::vec2(across, thumb + CELL_PAD + LABEL_H),
            thumb,
        }
    } else {
        let across = (ui.available_height() - bar).clamp(CELL_MIN + CELL_PAD + LABEL_H, CELL_MAX);
        let thumb = across - CELL_PAD - LABEL_H;
        StripMetrics {
            cell: egui::vec2(thumb + 2.0 * CELL_PAD, across),
            thumb,
        }
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
    let plus_size = (m.thumb * 0.4).max(14.0);
    painter.text(
        egui::pos2(rect.center().x, rect.top() + 6.0 + m.thumb / 2.0),
        egui::Align2::CENTER_CENTER,
        "✚",
        egui::FontId::proportional(plus_size),
        ui.visuals().weak_text_color(),
    );
    painter.text(
        egui::pos2(rect.center().x, rect.bottom() - 10.0),
        egui::Align2::CENTER_CENTER,
        "New canvas",
        egui::FontId::proportional(10.5),
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

    let thumb_rect = egui::Rect::from_center_size(
        egui::pos2(rect.center().x, rect.top() + 6.0 + m.thumb / 2.0),
        egui::Vec2::splat(m.thumb),
    );
    let glyph_size = (m.thumb * 0.34).max(12.0);
    match (&slot.thumb, slot.thumb_failed) {
        (Some(tex), _) => {
            let size = tex.size_vec2();
            let scale = (m.thumb / size.x).min(m.thumb / size.y).min(1.0);
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

    let mut name = slot.name.clone();
    let max_chars = ((m.cell.x / 7.0) as usize).max(8);
    if name.chars().count() > max_chars {
        name = format!(
            "{}…",
            name.chars()
                .take(max_chars.saturating_sub(1))
                .collect::<String>()
        );
    }
    painter.text(
        egui::pos2(rect.center().x, rect.bottom() - 10.0),
        egui::Align2::CENTER_CENTER,
        name,
        egui::FontId::proportional(10.5),
        ui.visuals().text_color(),
    );

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
