//! Cálculos de layout de la baraja usados por el lienzo activo.

use eframe::egui;

use crate::deck::{Deck, SlotContent};

pub(super) fn sync_deck_layout(
    deck: &mut Deck,
    active_page: (f64, f64),
    needs_fit: bool,
    zoom: f64,
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
            // `delta` viene en px de documento (espacio de baraja) y `pan`
            // en puntos de pantalla: la conversión página → pantalla
            // multiplica por `zoom` (ver `page_to_screen`), así que la
            // compensación debe usar el mismo factor (A01).
            if delta.0 != 0.0 || delta.1 != 0.0 {
                *pan -= egui::vec2((delta.0 * zoom) as f32, (delta.1 * zoom) as f32);
            }
        }
    }
}

pub(super) fn active_slot_rect(deck: &Deck, rect: egui::Rect, zoom: f64) -> egui::Rect {
    let (x, y) = deck.active_origin();
    let offset = egui::vec2((x * zoom) as f32, (y * zoom) as f32);
    egui::Rect::from_min_size(rect.min + offset, rect.size())
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::deck::{DeckSeed, SeedItem};
    use crate::gallery::ItemKind;
    use crate::settings::GallerySort;

    fn deck_five(active: usize) -> Deck {
        let seed = DeckSeed {
            folder: PathBuf::from("carpeta"),
            sort: GallerySort::Name,
            items: (0..5)
                .map(|i| {
                    let name = format!("{i}.png");
                    SeedItem {
                        path: PathBuf::from(&name),
                        name,
                        kind: ItemKind::Image,
                        mtime: None,
                        thumb: None,
                        thumb_failed: false,
                    }
                })
                .collect(),
        };
        let mut deck = Deck::from_seed(seed, Path::new("4.png"));
        deck.active = active;
        deck
    }

    /// La posición en pantalla del activo no debe moverse al llegar sondas
    /// tardías (A01): `origen * zoom + pan` invariante, con varios zooms.
    #[test]
    fn late_probes_keep_the_active_canvas_fixed_on_screen() {
        for zoom in [0.25, 0.5, 1.0, 2.0] {
            let mut deck = deck_five(4);
            // Cuatro anteriores estimadas en 1600×1600, activa en 800×600.
            for (i, slot) in deck.slots.iter_mut().enumerate() {
                slot.page = Some(if i < 4 {
                    (1600.0, 1600.0)
                } else {
                    (800.0, 600.0)
                });
            }
            deck.relayout();
            let origin_before = deck.active_origin();
            let mut pan = egui::vec2(10.0, -20.0);
            let screen_before = (
                origin_before.0 * zoom + f64::from(pan.x),
                origin_before.1 * zoom + f64::from(pan.y),
            );

            // Llegan los tamaños reales de 800×600 para las anteriores.
            for slot in deck.slots.iter_mut().take(4) {
                slot.page = Some((800.0, 600.0));
            }
            deck.layout_dirty = true;
            sync_deck_layout(&mut deck, (800.0, 600.0), false, zoom, &mut pan);

            let origin_after = deck.active_origin();
            let screen_after = (
                origin_after.0 * zoom + f64::from(pan.x),
                origin_after.1 * zoom + f64::from(pan.y),
            );
            assert!(
                (screen_after.0 - screen_before.0).abs() < 1e-3
                    && (screen_after.1 - screen_before.1).abs() < 1e-3,
                "zoom {zoom}: antes {screen_before:?} después {screen_after:?}"
            );
        }
    }

    #[test]
    fn pending_fit_skips_the_pan_compensation() {
        let mut deck = deck_five(4);
        for slot in deck.slots.iter_mut() {
            slot.page = Some((800.0, 600.0));
        }
        deck.relayout();
        let mut pan = egui::vec2(0.0, 0.0);
        for slot in deck.slots.iter_mut().take(4) {
            slot.page = Some((1600.0, 1600.0));
        }
        deck.layout_dirty = true;
        sync_deck_layout(&mut deck, (800.0, 600.0), true, 0.25, &mut pan);
        assert_eq!(
            pan,
            egui::vec2(0.0, 0.0),
            "con fit pendiente no se compensa"
        );
    }
}
