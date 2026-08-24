//! La cabecera de un lienzo de la baraja: su disposicion, los botones
//! (candado, duplicar, borrar) y sus tooltips.

use eframe::egui;

use crate::deck::{Deck, DeckAxis, Slot};

use crate::app_icons::draw_isolate_icon;

use super::icons::{
    draw_delete_icon, draw_duplicate_icon, draw_lock_icon, draw_triangle_icon, IconDir,
};

use super::super::viewport::page_to_screen;
use super::super::Viewport;

/// Cabecera de un lienzo, justo encima de su `screen_rect`: nombre a la
/// izquierda (editable — ver `draw_rename_overlay`, que se encarga del
/// cuadro de texto en sí) y, a la derecha, mover/bloquear/duplicar/borrar.
/// Duplicar/borrar se ocultan en una ranura PROVISIONAL (`is_placeholder`):
/// no tienen sentido sin archivo en disco. Pintada con el `Painter` normal
/// de egui, no widgets — el hit-test vive en el bloque de pulsación de
/// `canvas_ui`, sobre el mismo `slot_header_layout` que esta función usa
/// para pintar, así que ambos nunca pueden desalinearse.
///
/// Los 6 botones son formas dibujadas a mano (triángulos, arco, rects),
/// NO texto/emoji: un carácter Unicode como ▲/▼ puede simplemente no estar
/// en la fuente que trae `egui` integrada — pasó de verdad (las flechas
/// dejaron de verse, mientras que otros glifos ya usados en la app seguían
/// bien) — y un dibujo a mano no depende de qué cubra ninguna fuente.
pub(super) fn draw_slot_header(deck: &Deck, slot: &Slot, ui: &egui::Ui, screen_rect: egui::Rect) {
    let Some(header) =
        slot_header_layout(screen_rect.left(), screen_rect.right(), screen_rect.top())
    else {
        return;
    };
    let painter = ui.painter();
    // Fondo propio con contraste real (antes era texto suelto directamente
    // sobre el lienzo — "iconos grises sobre fondo gris" cuando la imagen de
    // debajo también era clara/gris) + un borde débil para separarla del
    // lienzo cuando el fondo coincide en tono.
    painter.rect_filled(header.bar, 4.0, ui.visuals().extreme_bg_color);
    painter.rect_stroke(
        header.bar,
        4.0,
        egui::Stroke::new(1.0, ui.visuals().weak_text_color().gamma_multiply(0.6)),
        egui::StrokeKind::Outside,
    );
    let icon_color = ui.visuals().strong_text_color();

    let renaming = deck
        .rename_edit
        .as_ref()
        .is_some_and(|(id, _)| *id == slot.id);
    if !renaming {
        let mut name = slot.name.clone();
        let max_chars = ((header.name.width() / 6.5) as usize).max(4);
        if name.chars().count() > max_chars {
            name = format!(
                "{}…",
                name.chars()
                    .take(max_chars.saturating_sub(1))
                    .collect::<String>()
            );
        }
        painter.text(
            header.name.left_center() + egui::vec2(4.0, 0.0),
            egui::Align2::LEFT_CENTER,
            name,
            egui::FontId::proportional(11.0),
            ui.visuals().text_color(),
        );
    }

    // Flechas de mover en la dirección real de apilado: arriba/abajo con la
    // baraja en vertical, izquierda/derecha en horizontal.
    let (prev_dir, next_dir) = match deck.axis {
        DeckAxis::Vertical => (IconDir::Up, IconDir::Down),
        DeckAxis::Horizontal => (IconDir::Left, IconDir::Right),
    };
    draw_triangle_icon(painter, header.prev, prev_dir, icon_color);
    draw_triangle_icon(painter, header.next, next_dir, icon_color);
    draw_lock_icon(painter, header.lock, slot.locked, icon_color);
    draw_isolate_icon(painter, header.isolate, icon_color);
    draw_duplicate_icon(
        painter,
        header.dup,
        icon_color,
        ui.visuals().extreme_bg_color,
    );
    draw_delete_icon(painter, header.del, egui::Color32::from_rgb(220, 70, 70));
}

/// Alto de la cabecera de un lienzo (nombre + botones), en px de pantalla —
/// constante frente al zoom, como el resto del texto de `draw_slot_chrome`.
pub(super) const HEADER_H: f32 = 20.0;

/// Ancho de cada botón cuadrado de la cabecera cuando hay sitio de sobra.
const HEADER_BTN_W: f32 = 20.0;

/// Suelo: por debajo de esto un botón deja de ser legible/pulsable con
/// precisión — mejor que se quede aquí a que seis encimen sin límite.
const HEADER_BTN_MIN: f32 = 12.0;

/// Ancho de cabecera por debajo del cual ni se pinta ni se comprueba el
/// clic: con 6 botones al suelo (`HEADER_BTN_MIN * 6`) más algo de nombre,
/// menos que esto es un lienzo tan alejado que la cabecera sería ruido
/// ilegible superpuesto a otra cosa — mismo criterio que usan las
/// miniaturas de la tira, que a partir de cierto tamaño mínimo dejan de
/// intentar mostrar detalle y muestran solo un glifo.
const HEADER_MIN_VISIBLE_W: f32 = 84.0;

/// Rects (en espacio de PANTALLA) de la cabecera de un lienzo: la barra
/// entera (para el fondo), el nombre y los 5 botones, calculados a partir
/// del borde superior de su `screen_rect`. Una sola fuente de verdad
/// compartida por pintado (`draw_slot_header`, `draw_rename_overlay`,
/// `draw_header_tooltips`) y hit-test (`canvas_ui`) — así nunca se
/// desalinean entre sí.
pub(in crate::editor) struct SlotHeader {
    pub(in crate::editor) bar: egui::Rect,
    pub(in crate::editor) name: egui::Rect,
    pub(in crate::editor) prev: egui::Rect,
    pub(in crate::editor) next: egui::Rect,
    pub(in crate::editor) lock: egui::Rect,
    pub(in crate::editor) isolate: egui::Rect,
    pub(in crate::editor) dup: egui::Rect,
    pub(in crate::editor) del: egui::Rect,
}

/// `None` si la cabecera es demasiado angosta en pantalla para pintarse o
/// pulsarse con sentido (ver `HEADER_MIN_VISIBLE_W`) — el llamador la omite
/// entera en vez de intentar encajarla.
pub(in crate::editor) fn slot_header_layout(left: f32, right: f32, top: f32) -> Option<SlotHeader> {
    let bar = egui::Rect::from_min_max(egui::pos2(left, top - HEADER_H), egui::pos2(right, top));
    if bar.width() < HEADER_MIN_VISIBLE_W {
        return None;
    }
    // Ancho de botón bien definido: se encoge en proporción al ancho real
    // del lienzo en pantalla (con suelo `HEADER_BTN_MIN`) en vez de quedar
    // fijo — así los 5 botones SIEMPRE caben dentro de la propia cabecera,
    // nunca se salen sobre el lienzo vecino, y en cuanto hay sitio de sobra
    // (zoom normal o mayor) se quedan clavados en `HEADER_BTN_W` sin seguir
    // creciendo ni encogiendo con el zoom.
    let btn_w = (bar.width() / 6.0).clamp(HEADER_BTN_MIN, HEADER_BTN_W);
    let buttons_w = btn_w * 6.0;
    let name_right = (bar.right() - buttons_w).max(bar.left());
    let name = egui::Rect::from_min_max(bar.left_top(), egui::pos2(name_right, bar.bottom()));
    let btn = |i: f32| {
        let x0 = name_right + btn_w * i;
        egui::Rect::from_min_max(
            egui::pos2(x0, bar.top()),
            egui::pos2(x0 + btn_w, bar.bottom()),
        )
    };
    Some(SlotHeader {
        bar,
        name,
        prev: btn(0.0),
        next: btn(1.0),
        lock: btn(2.0),
        isolate: btn(3.0),
        dup: btn(4.0),
        del: btn(5.0),
    })
}

/// Tooltip de un botón de cabecera al pasar el ratón por encima. Los rects
/// de la cabecera son pintados a mano (`Painter`, ver la doc de
/// `draw_slot_header`), no widgets egui — no hay `Response` del que colgar
/// `on_hover_text`, así que el propio tooltip se pinta a mano también,
/// sobre los MISMOS rects que ya usa el hit-test de clic en `canvas_ui`
/// (nunca puede desalinearse de lo que en verdad es pulsable).
pub(in crate::editor) fn draw_header_tooltips(
    deck: &Deck,
    ui: &egui::Ui,
    rect: egui::Rect,
    viewport: &Viewport,
    visible: &[usize],
) {
    let Some(pos) = ui.input(|i| i.pointer.hover_pos()) else {
        return;
    };
    if !rect.contains(pos) {
        return;
    }
    let (move_prev, move_next) = match deck.axis {
        DeckAxis::Vertical => ("Move up", "Move down"),
        DeckAxis::Horizontal => ("Move left", "Move right"),
    };
    for &idx in visible {
        let Some(slot) = deck.slots.get(idx) else {
            continue;
        };
        let s_rect = slot.rect;
        let top_left = page_to_screen(viewport, rect, s_rect.x, s_rect.y);
        let top_right = page_to_screen(viewport, rect, s_rect.x + s_rect.w, s_rect.y);
        let Some(header) = slot_header_layout(top_left.x, top_right.x, top_left.y) else {
            continue;
        };
        let label = if header.prev.contains(pos) {
            Some(move_prev)
        } else if header.next.contains(pos) {
            Some(move_next)
        } else if header.lock.contains(pos) {
            Some(if slot.locked { "Unlock" } else { "Lock" })
        } else if header.isolate.contains(pos) {
            Some("Isolate")
        } else if header.dup.contains(pos) {
            Some("Duplicate")
        } else if header.del.contains(pos) {
            Some("Delete")
        } else if !slot.is_placeholder && header.name.contains(pos) {
            Some("Rename")
        } else {
            None
        };
        if let Some(text) = label {
            paint_tooltip(ui, pos, text);
            return;
        }
    }
}

/// Etiqueta pegada al cursor, mismo estilo (fondo + borde) que la cabecera
/// que la disparó.
fn paint_tooltip(ui: &egui::Ui, pos: egui::Pos2, text: &str) {
    let painter = ui.painter();
    let font = egui::FontId::proportional(11.0);
    let galley = painter.layout_no_wrap(text.to_owned(), font, ui.visuals().text_color());
    let pad = egui::vec2(6.0, 4.0);
    let box_rect =
        egui::Rect::from_min_size(pos + egui::vec2(12.0, 16.0), galley.size() + pad * 2.0);
    painter.rect_filled(box_rect, 4.0, ui.visuals().extreme_bg_color);
    painter.rect_stroke(
        box_rect,
        4.0,
        egui::Stroke::new(1.0, ui.visuals().weak_text_color()),
        egui::StrokeKind::Outside,
    );
    painter.galley(box_rect.min + pad, galley, ui.visuals().text_color());
}
