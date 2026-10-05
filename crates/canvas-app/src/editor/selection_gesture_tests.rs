//! Pruebas del gesto completo con eventos reales de egui, sin GPU.
use super::{interaction::layer_interaction, EditorState};
use canvas_core::{LayerContent, ShapeContent, Transform};
use eframe::egui;

pub(super) fn frame(ctx: &egui::Context, state: &mut EditorState, events: Vec<egui::Event>) {
    let _ = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(600.0, 500.0),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(600.0, 500.0));
            let response = ui.allocate_rect(rect, egui::Sense::click_and_drag());
            layer_interaction(state, ui, &response, rect);
        },
    );
}
pub(super) fn pointer(x: f32, y: f32, pressed: Option<bool>) -> Vec<egui::Event> {
    let pos = egui::pos2(x, y);
    let mut events = vec![egui::Event::PointerMoved(pos)];
    if let Some(pressed) = pressed {
        events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
    }
    events
}
pub(super) fn fixture() -> (
    egui::Context,
    EditorState,
    canvas_core::LayerId,
    canvas_core::LayerId,
) {
    let mut state = EditorState::new_blank(600.0, 500.0);
    let a = state
        .doc
        .add_layer(
            "a",
            Transform::new(20.0, 20.0, 40.0, 40.0),
            LayerContent::Shape(ShapeContent::default()),
        )
        .unwrap();
    let b = state
        .doc
        .add_layer(
            "b",
            Transform::new(100.0, 20.0, 40.0, 40.0),
            LayerContent::Shape(ShapeContent::default()),
        )
        .unwrap();
    state.selection.set(Some(a));
    state.selection.toggle(b);
    let ctx = egui::Context::default();
    ctx.set_fonts(egui::FontDefinitions::empty());
    frame(&ctx, &mut state, vec![]);
    (ctx, state, a, b)
}
#[test]
fn dragging_selected_object_keeps_selection_and_moves_both_with_one_undo() {
    let (ctx, mut state, a, b) = fixture();
    frame(&ctx, &mut state, pointer(35.0, 35.0, Some(true)));
    frame(&ctx, &mut state, pointer(40.0, 35.0, None));
    frame(&ctx, &mut state, pointer(80.0, 75.0, None));
    frame(&ctx, &mut state, pointer(80.0, 75.0, Some(false)));
    assert_eq!(state.selection.len(), 2);
    assert_eq!(state.doc.layer(a).unwrap().transform.x, 65.0);
    assert_eq!(state.doc.layer(b).unwrap().transform.x, 145.0);
    assert_eq!(state.history.undo_depth(), 1);
    state.undo();
    assert_eq!(state.doc.layer(a).unwrap().transform.x, 20.0);
    assert_eq!(state.doc.layer(b).unwrap().transform.x, 100.0);
}
#[test]
fn escape_restores_every_layer_without_adding_undo() {
    let (ctx, mut state, a, b) = fixture();
    frame(&ctx, &mut state, pointer(35.0, 35.0, Some(true)));
    frame(&ctx, &mut state, pointer(40.0, 35.0, None));
    frame(&ctx, &mut state, pointer(80.0, 75.0, None));
    frame(
        &ctx,
        &mut state,
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    frame(&ctx, &mut state, pointer(80.0, 75.0, Some(false)));
    assert_eq!(state.doc.layer(a).unwrap().transform.x, 20.0);
    assert_eq!(state.doc.layer(b).unwrap().transform.x, 100.0);
    assert_eq!(state.history.undo_depth(), 0);
}

#[test]
fn corner_resizes_the_whole_selection_proportionally() {
    let (ctx, mut state, a, b) = fixture();
    frame(&ctx, &mut state, pointer(140.0, 60.0, Some(true)));
    frame(&ctx, &mut state, pointer(200.0, 80.0, None));
    frame(&ctx, &mut state, pointer(200.0, 80.0, Some(false)));
    assert_eq!(
        state.doc.layer(a).unwrap().transform,
        Transform::new(20.0, 20.0, 60.0, 60.0)
    );
    assert_eq!(
        state.doc.layer(b).unwrap().transform,
        Transform::new(140.0, 20.0, 60.0, 60.0)
    );
    state.undo();
    assert_eq!(state.doc.layer(b).unwrap().transform.width, 40.0);
}

#[test]
fn rotation_handle_rotates_both_layers_around_selection_center() {
    let (ctx, mut state, a, b) = fixture();
    for id in [a, b] {
        state.doc.layer_mut(id).unwrap().transform.y += 100.0;
    }
    frame(&ctx, &mut state, vec![]);
    frame(&ctx, &mut state, pointer(80.0, 94.0, Some(true)));
    frame(&ctx, &mut state, pointer(126.0, 140.0, None));
    frame(&ctx, &mut state, pointer(126.0, 140.0, Some(false)));
    assert!((state.doc.layer(a).unwrap().transform.rotation - 90.0).abs() < 1e-9);
    assert!((state.doc.layer(b).unwrap().transform.center().1 - 180.0).abs() < 1e-9);
}

#[test]
fn marquee_selects_contained_layers_and_ignores_locked_ones() {
    let (ctx, mut state, a, b) = fixture();
    state.selection.clear();
    state.doc.layer_mut(b).unwrap().locked = true;
    frame(&ctx, &mut state, pointer(10.0, 10.0, Some(true)));
    frame(&ctx, &mut state, pointer(150.0, 90.0, None));
    frame(&ctx, &mut state, pointer(150.0, 90.0, Some(false)));
    assert_eq!(state.selection.ids(), &[a]);
    assert_eq!(state.history.undo_depth(), 0);
}

#[test]
fn escape_cancels_marquee_and_restores_prior_selection() {
    let (ctx, mut state, a, b) = fixture();
    frame(&ctx, &mut state, pointer(150.0, 90.0, Some(true)));
    frame(&ctx, &mut state, pointer(90.0, 10.0, None));
    assert_eq!(state.selection.ids(), &[b]);
    frame(
        &ctx,
        &mut state,
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert!(state.selection.contains(a) && state.selection.contains(b));
}

#[test]
fn single_layer_move_can_be_cancelled_without_undo() {
    let (ctx, mut state, a, _) = fixture();
    state.selection.set(Some(a));
    frame(&ctx, &mut state, pointer(35.0, 35.0, Some(true)));
    frame(&ctx, &mut state, pointer(80.0, 75.0, None));
    assert_eq!(state.doc.layer(a).unwrap().transform.x, 60.0);
    frame(
        &ctx,
        &mut state,
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    frame(&ctx, &mut state, pointer(80.0, 75.0, Some(false)));
    assert_eq!(state.doc.layer(a).unwrap().transform.x, 20.0);
    assert_eq!(state.history.undo_depth(), 0);
}

#[test]
fn shift_locks_the_drag_to_its_dominant_axis() {
    let (ctx, mut state, a, _) = fixture();
    state.selection.set(Some(a));
    frame(&ctx, &mut state, pointer(35.0, 35.0, Some(true)));
    let _ = ctx.run_ui(
        egui::RawInput {
            modifiers: egui::Modifiers::SHIFT,
            events: pointer(80.0, 65.0, None),
            ..Default::default()
        },
        |ui| {
            let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(600.0, 500.0));
            let response = ui.allocate_rect(rect, egui::Sense::click_and_drag());
            layer_interaction(&mut state, ui, &response, rect);
        },
    );
    assert_eq!(state.doc.layer(a).unwrap().transform.y, 20.0);
    assert_eq!(state.doc.layer(a).unwrap().transform.x, 60.0);
}
