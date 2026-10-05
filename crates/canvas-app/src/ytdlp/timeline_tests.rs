use super::*;

fn scene() -> (VideoEdit, egui::Context) {
    let edit = VideoEdit::open(
        "clip.mp4",
        "Clip".into(),
        PathBuf::from("clip.mp4"),
        Some(60.0),
        (1920.0, 1080.0),
        &Document::new(1920.0, 1080.0),
    );
    (edit, egui::Context::default())
}

fn frame(edit: &mut VideoEdit, ctx: &egui::Context, events: Vec<egui::Event>) -> egui::Rect {
    let mut rect = egui::Rect::NOTHING;
    let _ = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 300.0),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            rect = timeline::show(edit, ui).rect;
        },
    );
    rect.shrink2(egui::vec2(10.0, 20.0))
}

fn pointer(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        pressed,
        button: egui::PointerButton::Primary,
        modifiers: egui::Modifiers::NONE,
    }
}

fn key(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}

#[test]
fn dragging_a_timeline_handle_is_undoable_as_one_gesture() {
    let (mut edit, ctx) = scene();
    let track = frame(&mut edit, &ctx, vec![]);
    let begin = egui::pos2(track.left(), track.center().y);
    frame(
        &mut edit,
        &ctx,
        vec![egui::Event::PointerMoved(begin), pointer(begin, true)],
    );
    for time in [5.0, 10.0, 15.0] {
        let pos = egui::pos2(track.left() + track.width() * time / 60.0, track.center().y);
        frame(&mut edit, &ctx, vec![egui::Event::PointerMoved(pos)]);
    }
    let end = egui::pos2(track.left() + track.width() / 4.0, track.center().y);
    frame(&mut edit, &ctx, vec![pointer(end, false)]);
    assert!((edit.trim_start - 15.0).abs() < 0.01);
    assert_eq!(edit.trim_history.undo.len(), 1);
    edit.undo_trim();
    assert_eq!(edit.trim_start, 0.0);
}

#[test]
fn a_timeline_click_seeks_and_keyboard_shortcuts_require_focus() {
    let (mut edit, ctx) = scene();
    let track = frame(&mut edit, &ctx, vec![]);
    edit.playhead = 5.0;
    frame(&mut edit, &ctx, vec![key(egui::Key::I)]);
    assert_eq!(edit.trim_start, 0.0);
    let pos = egui::pos2(track.left() + track.width() / 2.0, track.center().y);
    frame(
        &mut edit,
        &ctx,
        vec![egui::Event::PointerMoved(pos), pointer(pos, true)],
    );
    frame(&mut edit, &ctx, vec![pointer(pos, false)]);
    assert!((edit.playhead - 30.0).abs() < 0.01);
    frame(&mut edit, &ctx, vec![key(egui::Key::I)]);
    assert!((edit.trim_start - 30.0).abs() < 0.01);
}
