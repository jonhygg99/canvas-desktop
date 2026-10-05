use super::{
    insert_tool::InsertTool,
    selection_gesture_tests::{fixture, frame, pointer},
};
use canvas_core::{LayerContent, ShapeContent, Transform};
use eframe::egui;
#[test]
fn click_places_the_new_layer_at_the_pointer_with_one_undo_step() {
    let (ctx, mut state, _, _) = fixture();
    state.insert_tool = Some(InsertTool::new(
        "Rectangle",
        100.0,
        60.0,
        LayerContent::Shape(ShapeContent::default()),
    ));
    frame(&ctx, &mut state, pointer(300.0, 200.0, Some(true)));
    assert_eq!(state.doc.page().unwrap().layers.len(), 2);
    frame(&ctx, &mut state, pointer(300.0, 200.0, Some(false)));
    let id = state.selection.primary().unwrap();
    assert_eq!(
        state.doc.layer(id).unwrap().transform,
        Transform::new(300.0, 200.0, 100.0, 60.0)
    );
    assert!(state.insert_tool.is_none());
    state.undo();
    assert_eq!(state.doc.page().unwrap().layers.len(), 2);
}
#[test]
fn backwards_drag_sets_position_and_size_without_changing_document_until_release() {
    let (ctx, mut state, _, _) = fixture();
    state.insert_tool = Some(InsertTool::new(
        "Rectangle",
        100.0,
        60.0,
        LayerContent::Shape(ShapeContent::default()),
    ));
    frame(&ctx, &mut state, pointer(300.0, 200.0, Some(true)));
    frame(&ctx, &mut state, pointer(200.0, 100.0, None));
    assert_eq!(state.doc.page().unwrap().layers.len(), 2);
    frame(&ctx, &mut state, pointer(200.0, 100.0, Some(false)));
    assert_eq!(
        state
            .doc
            .layer(state.selection.primary().unwrap())
            .unwrap()
            .transform,
        Transform::new(200.0, 100.0, 100.0, 100.0)
    );
}
#[test]
fn escape_abandons_placement_without_document_changes() {
    let (ctx, mut state, _, _) = fixture();
    state.insert_tool = Some(InsertTool::new(
        "Rectangle",
        100.0,
        60.0,
        LayerContent::Shape(ShapeContent::default()),
    ));
    frame(&ctx, &mut state, pointer(300.0, 200.0, Some(true)));
    frame(&ctx, &mut state, pointer(200.0, 100.0, None));
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
    assert!(state.insert_tool.is_none());
    assert_eq!(state.doc.page().unwrap().layers.len(), 2);
    assert_eq!(state.history.undo_depth(), 0);
}
