//! Cálculos de layout de la baraja usados por el lienzo activo.

use eframe::egui;

use crate::deck::{Deck, SlotContent};

pub(super) fn sync_deck_layout(
    deck: &mut Deck,
    active_page: (f64, f64),
    needs_fit: bool,
    pan: &mut egui::Vec2,
) {
    let mut changed = false;
    if let Some(slot) = deck.slots.get_mut(deck.active) {
        if slot.page != Some(active_page) {
            slot.page = Some(active_page);
            changed = true;
        }
    }
    for slot in &mut deck.slots {
        if let SlotContent::Ready(document) = &slot.content {
            if let Ok(page) = document.doc.page() {
                let size = (page.width, page.height);
                if slot.page != Some(size) {
                    slot.page = Some(size);
                    changed = true;
                }
            }
        }
    }
    deck.layout_dirty |= changed;
    if deck.layout_dirty {
        let before = deck.active_origin();
        deck.relayout();
        if !needs_fit {
            let after = deck.active_origin();
            let delta = (after.0 - before.0, after.1 - before.1);
            if delta.0 != 0.0 || delta.1 != 0.0 {
                *pan -= egui::vec2(delta.0 as f32, delta.1 as f32);
            }
        }
    }
}

pub(super) fn active_slot_rect(deck: &Deck, rect: egui::Rect, zoom: f64) -> egui::Rect {
    let (x, y) = deck.active_origin();
    let offset = egui::vec2((x * zoom) as f32, (y * zoom) as f32);
    egui::Rect::from_min_size(rect.min + offset, rect.size())
}
