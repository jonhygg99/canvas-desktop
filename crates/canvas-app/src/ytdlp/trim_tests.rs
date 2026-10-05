use super::*;

fn edit() -> VideoEdit {
    VideoEdit::open(
        "clip.mp4",
        "Clip".into(),
        PathBuf::from("clip.mp4"),
        Some(60.0),
        (1920.0, 1080.0),
        &Document::new(1920.0, 1080.0),
    )
}

#[test]
fn duration_control_makes_a_seven_second_clip_and_is_undoable() {
    let mut edit = edit();
    edit.set_trim_edge(TrimEdge::Start, 12.0);
    edit.set_trim_duration(7.0);
    assert_eq!((edit.trim_start, edit.trim_end), (12.0, 19.0));
    edit.undo_trim();
    assert_eq!((edit.trim_start, edit.trim_end), (12.0, 60.0));
}

#[test]
fn seven_second_preset_works_without_typing_a_time() {
    let mut edit = edit();
    edit.set_trim_edge(TrimEdge::Start, 12.0);
    let ctx = egui::Context::default();
    let render = |edit: &mut VideoEdit, events| {
        ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(320.0, 700.0),
                )),
                events,
                ..Default::default()
            },
            |ui| super::trim_controls::trim_controls(edit, ui),
        )
    };
    let output = render(&mut edit, vec![]);
    let pos = output
        .shapes
        .iter()
        .find_map(|shape| {
            if let egui::Shape::Text(text) = &shape.shape {
                (text.galley.job.text == "7 s").then(|| text.pos + text.galley.size() / 2.0)
            } else {
                None
            }
        })
        .expect("visible 7 s preset");
    for pressed in [true, false] {
        render(
            &mut edit,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    pressed,
                    button: egui::PointerButton::Primary,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
    assert_eq!((edit.trim_start, edit.trim_end), (12.0, 19.0));
}

#[test]
fn trim_handles_keep_their_identity_and_preview_the_changed_edge() {
    let mut edit = edit();
    edit.set_trim_edge(TrimEdge::End, 20.0);
    assert!(edit.playhead < 20.0 && edit.playhead > 19.9);
    edit.set_trim_edge(TrimEdge::Start, 30.0);
    assert_eq!(edit.trim_end, 20.0);
    assert!((edit.trim_start - 19.9).abs() < 1e-9);
    assert_eq!(edit.playhead, edit.trim_start);
}

#[test]
fn a_trim_gesture_is_one_undo_step_and_reset_preserves_appearance() {
    let mut edit = edit();
    edit.blur = 35.0;
    edit.zoom = 2.0;
    edit.begin_trim_gesture();
    edit.set_trim_edge(TrimEdge::Start, 5.0);
    edit.set_trim_edge(TrimEdge::Start, 10.0);
    edit.finish_trim_gesture();
    edit.undo_trim();
    assert_eq!(edit.trim_start, 0.0);
    edit.redo_trim();
    assert_eq!(edit.trim_start, 10.0);
    edit.reset_trim();
    assert_eq!((edit.trim_start, edit.trim_end), (0.0, 60.0));
    assert_eq!((edit.blur, edit.zoom), (35.0, 2.0));
    edit.undo_trim();
    assert_eq!(edit.trim_start, 10.0);
}

#[test]
fn timecodes_show_hours_and_milliseconds() {
    assert_eq!(timecode(3723.456), "01:02:03.456");
}

#[test]
fn timeline_zoom_keeps_the_trim_intact_and_focuses_the_selection() {
    let mut edit = edit();
    edit.set_trim_edge(TrimEdge::Start, 20.0);
    edit.set_trim_edge(TrimEdge::End, 25.0);
    edit.fit_timeline_selection();
    let (start, end) = edit.timeline_view();
    assert!(start <= 20.0 && end >= 25.0 && end - start < 7.0);
    edit.zoom_timeline(0.5);
    assert_eq!((edit.trim_start, edit.trim_end), (20.0, 25.0));
    assert!(edit.timeline_view().1 - edit.timeline_view().0 < 4.0);
    edit.timeline_range = None;
    assert_eq!(edit.timeline_view(), (0.0, 60.0));
}

#[test]
fn loop_selection_restarts_at_the_trim_start() {
    let mut edit = edit();
    edit.trim_start = 10.0;
    edit.trim_end = 11.0;
    edit.playhead = 10.99;
    edit.playing = true;
    edit.loop_selection = true;
    edit.last_tick = Some(std::time::Instant::now() - std::time::Duration::from_millis(50));
    advance_playhead(&mut edit, &egui::Context::default());
    assert!(edit.playing);
    assert_eq!(edit.playhead, 10.0);
}
