//! "Chrome" de los lienzos vecinos visibles en la baraja: marco, nombre,
//! miniatura/placeholder, cabecera con candado/duplicar/borrar, tooltips,
//! renombrado in situ, y la zona "+" para añadir un lienzo — todo lo que se
//! dibuja ENCIMA del blit de vello pero no es el lienzo activo en sí.

use eframe::egui;

use crate::deck::{Deck, DeckAxis, Slot, SlotContent};
use crate::gallery::ItemKind;

use super::viewport::page_to_screen;
use super::{CanvasAction, EditorState, Viewport, ACCENT};

/// Marco (acento en la activa, débil en las demás), nombre de archivo, y —
/// si la ranura todavía no está cargada — su miniatura o un glifo de
/// estado, encima del blit de vello.
pub(super) fn draw_slot_chrome(
    state: &EditorState,
    deck: &Deck,
    idx: usize,
    ui: &egui::Ui,
    rect: egui::Rect,
) {
    let Some(slot) = deck.slots.get(idx) else {
        return;
    };
    let is_active = idx == deck.active;
    let tl = page_to_screen(&state.viewport, rect, slot.rect.x, slot.rect.y);
    let br = page_to_screen(
        &state.viewport,
        rect,
        slot.rect.x + slot.rect.w,
        slot.rect.y + slot.rect.h,
    );
    let screen_rect = egui::Rect::from_min_max(tl, br);
    let painter = ui.painter();

    if !matches!(slot.content, SlotContent::Ready(_) | SlotContent::Active) {
        if let Some(tex) = &slot.thumb {
            painter.image(
                tex.id(),
                screen_rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        } else {
            painter.rect_filled(screen_rect, 0.0, ui.visuals().extreme_bg_color);
        }
        if let SlotContent::Failed(message) = &slot.content {
            // Un fallo de carga de fondo SÍ se explica, aunque haya
            // miniatura: es la única pista de por qué este lienzo no abre.
            painter.text(
                screen_rect.center(),
                egui::Align2::CENTER_CENTER,
                "⚠",
                egui::FontId::proportional(28.0),
                ui.visuals().error_fg_color,
            );
            let mut short = message.clone();
            if short.chars().count() > 60 {
                short = format!("{}…", short.chars().take(59).collect::<String>());
            }
            painter.text(
                screen_rect.center() + egui::vec2(0.0, 22.0),
                egui::Align2::CENTER_TOP,
                short,
                egui::FontId::proportional(10.0),
                ui.visuals().error_fg_color,
            );
        } else if slot.thumb.is_none() {
            let glyph = if slot.thumb_failed {
                "⚠"
            } else if slot.kind == ItemKind::Design {
                "🖹"
            } else {
                "⏳"
            };
            painter.text(
                screen_rect.center(),
                egui::Align2::CENTER_CENTER,
                glyph,
                egui::FontId::proportional(28.0),
                ui.visuals().weak_text_color(),
            );
        }
    }

    let stroke = if is_active {
        egui::Stroke::new(2.0, ACCENT)
    } else {
        egui::Stroke::new(1.0, ui.visuals().weak_text_color().gamma_multiply(0.6))
    };
    painter.rect_stroke(screen_rect, 0.0, stroke, egui::StrokeKind::Outside);

    draw_slot_header(deck, slot, ui, screen_rect);
}

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
fn draw_slot_header(deck: &Deck, slot: &Slot, ui: &egui::Ui, screen_rect: egui::Rect) {
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
    painter.text(
        header.isolate.center(),
        egui::Align2::CENTER_CENTER,
        "I",
        egui::FontId::proportional(12.0),
        icon_color,
    );
    draw_duplicate_icon(
        painter,
        header.dup,
        icon_color,
        ui.visuals().extreme_bg_color,
    );
    draw_delete_icon(painter, header.del, egui::Color32::from_rgb(220, 70, 70));
}

/// Dirección de `draw_triangle_icon`.
#[derive(Clone, Copy)]
enum IconDir {
    Up,
    Down,
    Left,
    Right,
}

/// Triángulo relleno apuntando en `dir`, centrado en `rect`.
fn draw_triangle_icon(
    painter: &egui::Painter,
    rect: egui::Rect,
    dir: IconDir,
    color: egui::Color32,
) {
    let c = rect.center();
    let s = rect.width().min(rect.height()) * 0.28;
    let points = match dir {
        IconDir::Up => vec![
            c + egui::vec2(0.0, -s),
            c + egui::vec2(-s, s * 0.8),
            c + egui::vec2(s, s * 0.8),
        ],
        IconDir::Down => vec![
            c + egui::vec2(0.0, s),
            c + egui::vec2(-s, -s * 0.8),
            c + egui::vec2(s, -s * 0.8),
        ],
        IconDir::Left => vec![
            c + egui::vec2(-s, 0.0),
            c + egui::vec2(s * 0.8, -s),
            c + egui::vec2(s * 0.8, s),
        ],
        IconDir::Right => vec![
            c + egui::vec2(s, 0.0),
            c + egui::vec2(-s * 0.8, -s),
            c + egui::vec2(-s * 0.8, s),
        ],
    };
    painter.add(egui::Shape::convex_polygon(
        points,
        color,
        egui::Stroke::NONE,
    ));
}

/// Puntos a lo largo de un arco de circunferencia. Convención pensada para
/// que `start = -FRAC_PI_2`/`end = FRAC_PI_2` trace un semicírculo superior
/// de izquierda a derecha pasando por arriba (coordenadas de pantalla, eje Y
/// hacia abajo) — el arco del candado.
fn arc_points(
    center: egui::Pos2,
    radius: f32,
    start: f32,
    end: f32,
    segments: usize,
) -> Vec<egui::Pos2> {
    (0..=segments)
        .map(|i| {
            let t = start + (end - start) * (i as f32 / segments as f32);
            center + egui::vec2(t.sin(), -t.cos()) * radius
        })
        .collect()
}

/// Candado: cuerpo relleno (rect redondeado) + arco. Cerrado, el arco se
/// apoya simétrico sobre el cuerpo; abierto, se desplaza hacia arriba y a
/// la derecha, dejando el lado izquierdo suelto.
fn draw_lock_icon(painter: &egui::Painter, rect: egui::Rect, locked: bool, color: egui::Color32) {
    let half_pi = std::f32::consts::FRAC_PI_2;
    let body_w = rect.width() * 0.5;
    let body_h = rect.height() * 0.42;
    let body = egui::Rect::from_center_size(
        rect.center() + egui::vec2(0.0, body_h * 0.35),
        egui::vec2(body_w, body_h),
    );
    painter.rect_filled(body, 1.0, color);
    let shackle_r = body_w * 0.42;
    let stroke = egui::Stroke::new(1.3, color);
    let shackle_center = egui::pos2(body.center().x, body.top());
    let center = if locked {
        shackle_center
    } else {
        shackle_center + egui::vec2(shackle_r * 0.55, -shackle_r * 0.35)
    };
    let pts = arc_points(center, shackle_r, -half_pi, half_pi, 10);
    painter.add(egui::Shape::line(pts, stroke));
}

/// Duplicar: dos rects redondeados solapados (icono estándar de "copiar").
/// `bg` es el fondo de la propia cabecera — rellena el rect de delante para
/// que de verdad tape la esquina del de detrás, en vez de dejar ambos
/// contornos cruzándose sin más.
fn draw_duplicate_icon(
    painter: &egui::Painter,
    rect: egui::Rect,
    color: egui::Color32,
    bg: egui::Color32,
) {
    let s = rect.width().min(rect.height()) * 0.36;
    let stroke = egui::Stroke::new(1.2, color);
    let back = egui::Rect::from_center_size(
        rect.center() + egui::vec2(-s * 0.28, -s * 0.28),
        egui::vec2(s, s),
    );
    let front = egui::Rect::from_center_size(
        rect.center() + egui::vec2(s * 0.28, s * 0.28),
        egui::vec2(s, s),
    );
    painter.rect_stroke(back, 1.0, stroke, egui::StrokeKind::Outside);
    painter.rect_filled(front, 1.0, bg);
    painter.rect_stroke(front, 1.0, stroke, egui::StrokeKind::Outside);
}

/// Borrar: cubo de basura simple — cuerpo, tapa y un par de ranuras.
fn draw_delete_icon(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let stroke = egui::Stroke::new(1.2, color);
    let w = rect.width() * 0.42;
    let h = rect.height() * 0.4;
    let body =
        egui::Rect::from_center_size(rect.center() + egui::vec2(0.0, h * 0.2), egui::vec2(w, h));
    painter.rect_stroke(body, 0.5, stroke, egui::StrokeKind::Outside);
    painter.line_segment(
        [
            egui::pos2(body.left() - w * 0.15, body.top()),
            egui::pos2(body.right() + w * 0.15, body.top()),
        ],
        stroke,
    );
    for i in [-1.0_f32, 1.0] {
        let x = rect.center().x + i * w * 0.22;
        painter.line_segment(
            [
                egui::pos2(x, body.top() + h * 0.18),
                egui::pos2(x, body.bottom() - h * 0.12),
            ],
            stroke,
        );
    }
}

/// Alto de la cabecera de un lienzo (nombre + botones), en px de pantalla —
/// constante frente al zoom, como el resto del texto de `draw_slot_chrome`.
const HEADER_H: f32 = 20.0;
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
pub(super) struct SlotHeader {
    pub(super) bar: egui::Rect,
    pub(super) name: egui::Rect,
    pub(super) prev: egui::Rect,
    pub(super) next: egui::Rect,
    pub(super) lock: egui::Rect,
    pub(super) isolate: egui::Rect,
    pub(super) dup: egui::Rect,
    pub(super) del: egui::Rect,
}

/// `None` si la cabecera es demasiado angosta en pantalla para pintarse o
/// pulsarse con sentido (ver `HEADER_MIN_VISIBLE_W`) — el llamador la omite
/// entera en vez de intentar encajarla.
pub(super) fn slot_header_layout(left: f32, right: f32, top: f32) -> Option<SlotHeader> {
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
pub(super) fn draw_header_tooltips(
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

/// Si hay un renombrado en curso (`deck.rename_edit`, pulsado desde la
/// cabecera de un lienzo), dibuja su cuadro de texto en un `egui::Area` de
/// primer plano anclado a esa cabecera — un paso de UI SEPARADO del
/// `response` gigante del lienzo (como ya hacen los modales existentes de
/// esta app), para que arrastrar dentro del cuadro (seleccionar texto) no
/// compita con el arrastre de capa por el mismo puntero. Mismo patrón
/// Escape-antes-que-`lost_focus()` que `gallery.rs::gallery_cell` y
/// `layers_panel.rs::rename_edit_ui`.
pub(super) fn draw_rename_overlay(
    state: &EditorState,
    deck: &mut Deck,
    ui: &egui::Ui,
    rect: egui::Rect,
) -> Option<CanvasAction> {
    let id = deck.rename_edit.as_ref()?.0;
    let idx = deck.find_by_id(id)?;
    let s_rect = deck.slots[idx].rect;
    let top_left = page_to_screen(&state.viewport, rect, s_rect.x, s_rect.y);
    let top_right = page_to_screen(&state.viewport, rect, s_rect.x + s_rect.w, s_rect.y);
    // Si el zoom cambió mientras se renombraba y la cabecera ya no cabe
    // (`HEADER_MIN_VISIBLE_W`), cancela en vez de dejar el cuadro de texto
    // colgado sin dónde anclarse.
    let Some(header) = slot_header_layout(top_left.x, top_right.x, top_left.y) else {
        deck.rename_edit = None;
        return None;
    };

    let mut cancel = false;
    let mut commit = false;
    let text_id = egui::Id::new(("canvas_slot_rename", id));
    egui::Area::new(text_id.with("area"))
        .order(egui::Order::Foreground)
        .fixed_pos(header.name.left_top())
        .show(ui.ctx(), |ui| {
            if let Some((_, text)) = deck.rename_edit.as_mut() {
                let resp = ui.add_sized(
                    header.name.size(),
                    egui::TextEdit::singleline(text).id(text_id),
                );
                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    cancel = true;
                } else if resp.lost_focus() {
                    commit = true;
                }
            }
        });

    if cancel {
        deck.rename_edit = None;
        return None;
    }
    if commit {
        if let Some((id, text)) = deck.rename_edit.take() {
            let new_stem = text.trim().to_owned();
            let original_stem = deck
                .find_by_id(id)
                .and_then(|idx| deck.slots[idx].path.file_stem())
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            if !new_stem.is_empty() && new_stem != original_stem {
                return Some(CanvasAction::Rename(id, new_stem));
            }
        }
    }
    None
}

/// Zona "+" al final de la baraja, en el área central: mismo estilo que
/// `deck_strip::strip_add_cell` (borde discontinuo, glifo "✚", etiqueta) pero
/// en coordenadas de pantalla del propio lienzo, no de celda de tira. Solo
/// se pinta si hay una carpeta detrás de la baraja (un archivo suelto no
/// tiene dónde materializar el nuevo diseño) y si `deck.add_zone` cae dentro
/// de lo visible.
pub(super) fn draw_add_zone(state: &EditorState, deck: &Deck, ui: &egui::Ui, rect: egui::Rect) {
    if deck.folder.is_none() {
        return;
    }
    let tl = page_to_screen(&state.viewport, rect, deck.add_zone.x, deck.add_zone.y);
    let br = page_to_screen(
        &state.viewport,
        rect,
        deck.add_zone.x + deck.add_zone.w,
        deck.add_zone.y + deck.add_zone.h,
    );
    let screen_rect = egui::Rect::from_min_max(tl, br);
    if !ui.is_rect_visible(screen_rect) {
        return;
    }
    let painter = ui.painter();
    painter.rect_stroke(
        screen_rect.shrink(4.0),
        6.0,
        egui::Stroke::new(1.5, ui.visuals().weak_text_color()),
        egui::StrokeKind::Inside,
    );
    let glyph_size = (screen_rect.width().min(screen_rect.height()) * 0.15).clamp(18.0, 56.0);
    painter.text(
        screen_rect.center() - egui::vec2(0.0, glyph_size * 0.4),
        egui::Align2::CENTER_CENTER,
        "✚",
        egui::FontId::proportional(glyph_size),
        ui.visuals().weak_text_color(),
    );
    painter.text(
        screen_rect.center() + egui::vec2(0.0, glyph_size * 0.6),
        egui::Align2::CENTER_CENTER,
        "Add canvas",
        egui::FontId::proportional(13.0),
        ui.visuals().weak_text_color(),
    );
}
