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
