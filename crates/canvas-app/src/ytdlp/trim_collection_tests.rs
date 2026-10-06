use super::*;

fn editor() -> VideoEdit {
    VideoEdit::open(
        "clip",
        "Clip".into(),
        PathBuf::new(),
        Some(30.0),
        (1920.0, 1080.0),
        &Document::new(1920.0, 1080.0),
    )
}

#[test]
fn trims_keep_independent_ranges_and_undo_history_when_switching() {
    let mut edit = editor();
    edit.set_trim_duration(7.0);
    edit.add_trim();
    assert_eq!((edit.trim_start, edit.trim_end), (7.0, 14.0));
    edit.set_trim_duration(5.0);
    edit.select_trim(0);
    assert_eq!((edit.trim_start, edit.trim_end), (0.0, 7.0));
    edit.undo_trim();
    assert_eq!((edit.trim_start, edit.trim_end), (0.0, 30.0));
    edit.select_trim(1);
    assert_eq!((edit.trim_start, edit.trim_end), (7.0, 12.0));
    edit.undo_trim();
    assert_eq!((edit.trim_start, edit.trim_end), (7.0, 14.0));
}

#[test]
fn removing_trims_keeps_the_remaining_selection_and_at_least_one_trim() {
    let mut edit = editor();
    edit.set_trim_duration(5.0);
    edit.add_trim();
    edit.add_trim();
    edit.remove_trim(0);
    assert_eq!(edit.active_trim, 1);
    assert_eq!(edit.trim_ranges(), vec![(5.0, 10.0), (10.0, 15.0)]);
    edit.remove_trim(1);
    assert_eq!((edit.trim_start, edit.trim_end), (5.0, 10.0));
    edit.remove_trim(0);
    assert_eq!(edit.trim_ranges(), vec![(5.0, 10.0)]);
}

#[test]
fn each_accepted_trim_uses_its_own_poster_and_shares_appearance() {
    let mut edit = editor();
    edit.frames = (0..30)
        .map(|i| PathBuf::from(format!("frame-{i}.png")))
        .collect();
    edit.frame_fps = 1.0;
    edit.zoom = 2.0;
    edit.position = (40.0, -15.0);
    edit.set_trim_duration(7.0);
    edit.add_trim();
    let accepts = edit.build_accepts();
    assert_eq!(accepts.len(), 2);
    assert_eq!(
        (accepts[0].trim_start, accepts[0].trim_end),
        (0.0, Some(7.0))
    );
    assert_eq!(
        (accepts[1].trim_start, accepts[1].trim_end),
        (7.0, Some(14.0))
    );
    assert_eq!(accepts[1].poster, PathBuf::from("frame-7.png"));
    for accept in accepts {
        assert_eq!(accept.zoom, 2.0);
        assert_eq!(accept.position, (40.0, -15.0));
    }
}
