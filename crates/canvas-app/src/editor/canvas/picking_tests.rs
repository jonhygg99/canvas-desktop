//! Regresión: los controles de crop superpuestos a un vecino conservan el documento activo.
use super::*;
use crate::deck::{DeckSeed, SeedItem};
use crate::editor::{interaction::layer_interaction, viewport::layer_corners_screen};
use crate::gallery::ItemKind;
use crate::settings::GallerySort;
use canvas_core::{ImageContent, LayerContent, Transform};
use std::path::{Path, PathBuf};

fn scene(active: usize) -> (EditorState, Deck) {
    let seed = DeckSeed {
        folder: PathBuf::from("crop-test"),
        sort: GallerySort::Name,
        items: ["a.canvas", "b.canvas"]
            .map(|name| SeedItem {
                path: PathBuf::from(name),
                name: name.into(),
                kind: ItemKind::Image,
                mtime: None,
                thumb: None,
                thumb_failed: false,
            })
            .to_vec(),
    };
    let mut deck = Deck::from_seed(seed, Path::new("a.canvas"));
    deck.active = active;
    for (index, slot) in deck.slots.iter_mut().enumerate() {
        slot.rect = crate::deck::DeckRect {
            x: index as f64 * 300.0,
            y: 0.0,
            w: 200.0,
            h: 180.0,
        };
    }
    let mut state = EditorState::new_blank(200.0, 180.0);
    state.viewport.pan = egui::vec2(20.0, 20.0);
    let id = state
        .doc
        .add_layer(
            "oversized image",
            Transform::new(if active == 0 { 20.0 } else { -280.0 }, 30.0, 340.0, 120.0),
            LayerContent::Image(ImageContent {
                source_path: None,
                natural_width: 340,
                natural_height: 120,
                crop: None,
            }),
        )
        .unwrap();
    state.selection.set(Some(id));
    state.crop_mode = true;
    (state, deck)
}

fn frame(ctx: &egui::Context, state: &mut EditorState, deck: &mut Deck, events: Vec<egui::Event>) {
    let _ = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 400.0),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            let (rect, response) =
                ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
            let mut action = None;
            handle_press(
                state,
                deck,
                ui,
                &PressGeometry {
                    rect,
                    response: &response,
                    visible: &[0, 1],
                    space_down: false,
                },
                "canvas",
                &mut action,
            );
            assert!(action.is_none());
            if !state.press_on_other_slot {
                let coord = super::super::layout::active_slot_rect(deck, rect, state.viewport.zoom);
                layer_interaction(state, ui, &response, coord);
            }
            if !ui.input(|i| i.pointer.primary_down()) {
                state.press_on_other_slot = false;
            }
        },
    );
}

fn button(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        pressed,
        button: egui::PointerButton::Primary,
        modifiers: egui::Modifiers::NONE,
    }
}

#[test]
fn crop_corner_over_neighbor_drags_image_without_switching_canvas() {
    for (active, margin) in [(0, false), (1, false), (0, true), (1, true)] {
        let (mut state, mut deck) = scene(active);
        let id = state.selection.primary().unwrap();
        let original = state.doc.layer(id).unwrap().transform;
        let coord = super::super::layout::active_slot_rect(
            &deck,
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 400.0)),
            1.0,
        );
        let mut pos = layer_corners_screen(&state.viewport, coord, &original)
            [if active == 0 { 3 } else { 2 }];
        if margin {
            pos += egui::vec2(if active == 0 { 6.0 } else { -6.0 }, 6.0);
        }
        let target = pos + egui::vec2(if active == 0 { -40.0 } else { 40.0 }, -20.0);
        let ctx = egui::Context::default();
        frame(
            &ctx,
            &mut state,
            &mut deck,
            vec![egui::Event::PointerMoved(pos)],
        );
        frame(&ctx, &mut state, &mut deck, vec![button(pos, true)]);
        assert_eq!(deck.jump_to, None, "la esquina pertenece al recorte activo");
        frame(
            &ctx,
            &mut state,
            &mut deck,
            vec![egui::Event::PointerMoved(target)],
        );
        frame(&ctx, &mut state, &mut deck, vec![button(target, false)]);
        assert_eq!(state.selection.primary(), Some(id));
        assert!(state.crop_mode);
        let layer = state.doc.layer(id).unwrap();
        assert_eq!(layer.transform.width, 300.0);
        assert_eq!(layer.transform.height, 100.0);
        assert!(matches!(&layer.content, LayerContent::Image(image) if image.crop.is_some()));
        state.undo();
        assert_eq!(state.doc.layer(id).unwrap().transform, original);
    }
}

#[test]
fn clicking_crop_handle_margin_keeps_selection_and_crop_mode() {
    let (mut state, mut deck) = scene(0);
    let selected = state.selection.clone();
    let ctx = egui::Context::default();
    let pos = egui::pos2(386.0, 176.0);
    frame(
        &ctx,
        &mut state,
        &mut deck,
        vec![egui::Event::PointerMoved(pos)],
    );
    frame(&ctx, &mut state, &mut deck, vec![button(pos, true)]);
    frame(&ctx, &mut state, &mut deck, vec![button(pos, false)]);
    assert_eq!(state.selection, selected);
    assert!(state.crop_mode);
    assert_eq!(deck.jump_to, None);
}

#[test]
fn crop_handle_over_neighbor_header_has_priority_over_header_actions() {
    let (mut state, mut deck) = scene(0);
    let id = state.selection.primary().unwrap();
    state.doc.layer_mut(id).unwrap().transform.y = -132.0;
    let ctx = egui::Context::default();
    let pos = egui::pos2(380.0, 8.0);
    frame(
        &ctx,
        &mut state,
        &mut deck,
        vec![egui::Event::PointerMoved(pos)],
    );
    frame(&ctx, &mut state, &mut deck, vec![button(pos, true)]);
    assert!(!state.press_on_other_slot);
    assert!(deck.rename_edit.is_none());
    assert_eq!(deck.jump_to, None);
}

#[test]
fn crop_mode_still_allows_switching_canvas_away_from_handles() {
    let (mut state, mut deck) = scene(0);
    let ctx = egui::Context::default();
    let pos = egui::pos2(480.0, 100.0);
    frame(
        &ctx,
        &mut state,
        &mut deck,
        vec![egui::Event::PointerMoved(pos)],
    );
    frame(&ctx, &mut state, &mut deck, vec![button(pos, true)]);
    assert_eq!(deck.jump_to, Some(1));
    assert!(state.press_on_other_slot);
}
