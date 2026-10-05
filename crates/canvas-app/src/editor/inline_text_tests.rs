use super::{inline_text, EditorState};
use canvas_core::{LayerContent, TextContent};
use eframe::egui;
fn fixture() -> (EditorState, canvas_core::LayerId) {
    let mut state = EditorState::new_blank(600.0, 400.0);
    state.insert_layer_centered(
        "Text",
        200.0,
        80.0,
        LayerContent::Text(TextContent::default()),
    );
    let id = state.selection.primary().unwrap();
    state.history.mark_saved();
    (state, id)
}
fn text(state: &EditorState, id: canvas_core::LayerId) -> String {
    match &state.doc.layer(id).unwrap().content {
        LayerContent::Text(t) => t.text.clone(),
        _ => unreachable!(),
    }
}
fn show(ctx: &egui::Context, state: &mut EditorState, events: Vec<egui::Event>) {
    let _ = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(600.0, 400.0),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            inline_text::show(
                state,
                ui,
                egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(600.0, 400.0)),
                ui.max_rect(),
            );
        },
    );
}
#[test]
fn typing_on_canvas_commits_one_step_and_undo_restores_content() {
    let (mut s, id) = fixture();
    let before = text(&s, id);
    let ctx = egui::Context::default();
    inline_text::begin(&mut s, id);
    show(&ctx, &mut s, vec![]);
    show(&ctx, &mut s, vec![]);
    assert!(ctx.text_edit_focused());
    show(&ctx, &mut s, vec![egui::Event::Text(" edited".to_owned())]);
    assert!(text(&s, id).contains(" edited"));
    assert!(s.is_dirty());
    inline_text::finish(&mut s, false);
    assert_eq!(s.history.undo_depth(), 2);
    s.undo();
    assert_eq!(text(&s, id), before);
}
#[test]
fn escape_restores_content_and_locked_text_cannot_be_edited() {
    let (mut s, id) = fixture();
    let before = text(&s, id);
    let ctx = egui::Context::default();
    inline_text::begin(&mut s, id);
    show(&ctx, &mut s, vec![]);
    show(&ctx, &mut s, vec![]);
    show(&ctx, &mut s, vec![egui::Event::Text("changed".to_owned())]);
    show(
        &ctx,
        &mut s,
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert_eq!(text(&s, id), before);
    assert_eq!(s.history.undo_depth(), 1);
    s.doc.layer_mut(id).unwrap().locked = true;
    inline_text::begin(&mut s, id);
    assert!(s.inline_text.is_none());
}
#[test]
fn save_request_commits_the_open_text_session_before_saving() {
    let (mut s, id) = fixture();
    inline_text::begin(&mut s, id);
    if let LayerContent::Text(t) = &mut s.doc.layer_mut(id).unwrap().content {
        t.text = "saved edit".to_owned();
    }
    s.save_clicked = true;
    inline_text::prepare(&mut s, &egui::Context::default());
    assert!(s.inline_text.is_none());
    assert!(s.is_dirty());
    assert_eq!(s.history.undo_depth(), 2);
}

#[test]
fn ctrl_enter_confirms_without_inserting_a_line_break() {
    let (mut s, id) = fixture();
    let before = text(&s, id);
    let ctx = egui::Context::default();
    inline_text::begin(&mut s, id);
    show(&ctx, &mut s, vec![]);
    show(&ctx, &mut s, vec![]);
    show(&ctx, &mut s, vec![egui::Event::Text(" edited".to_owned())]);
    show(
        &ctx,
        &mut s,
        vec![egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND,
        }],
    );
    assert!(s.inline_text.is_none());
    assert!(!text(&s, id).contains('\n'));
    s.undo();
    assert_eq!(text(&s, id), before);
}
