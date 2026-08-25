//! "Chrome" de los lienzos vecinos visibles en la baraja: marco, nombre,
//! miniatura/placeholder, cabecera con candado/duplicar/borrar, tooltips,
//! renombrado in situ, y la zona "+" para anadir un lienzo - todo lo que se
//! dibuja ENCIMA del blit de vello pero no es el lienzo activo en si.

use eframe::egui;

use crate::app_icons::{draw_doc_icon, draw_plus_icon, draw_spinner_icon, draw_warning_icon};
use crate::deck::{Deck, SlotContent};
use crate::gallery::ItemKind;

use super::viewport::page_to_screen;
use super::{CanvasAction, EditorState, ACCENT};

mod header;
mod icons;

pub(in crate::editor) use header::{draw_header_tooltips, slot_header_layout};

use header::draw_slot_header;

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
            draw_warning_icon(
                ui.painter(),
                egui::Rect::from_center_size(screen_rect.center(), egui::vec2(40.0, 40.0)),
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
            let glyph_rect =
                egui::Rect::from_center_size(screen_rect.center(), egui::vec2(40.0, 40.0));
            if slot.thumb_failed {
                draw_warning_icon(ui.painter(), glyph_rect, ui.visuals().weak_text_color());
            } else if slot.kind == ItemKind::Design {
                draw_doc_icon(ui.painter(), glyph_rect, ui.visuals().weak_text_color());
            } else {
                let t = ui.input(|i| i.time);
                draw_spinner_icon(ui.painter(), glyph_rect, t, ui.visuals().weak_text_color());
            }
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
    draw_plus_icon(
        painter,
        egui::Rect::from_center_size(
            screen_rect.center() - egui::vec2(0.0, glyph_size * 0.4),
            egui::vec2(glyph_size, glyph_size),
        ),
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
