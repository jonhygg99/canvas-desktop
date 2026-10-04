use super::*;

#[test]
fn each_gesture_has_its_own_history_and_new_edit_discards_redo() {
    let mut session = Session::new(None, false);
    let before = session.value;
    session.value.x_pct = 20.0;
    session.commit(before);
    session.undo();
    assert_eq!(session.value, before);
    session.redo();
    assert_eq!(session.value.x_pct, 20.0);
    session.undo();
    session.value.scale_pct = 75;
    session.commit(before);
    session.redo();
    assert_eq!(session.value.scale_pct, 75);
    assert_eq!(session.value.x_pct, 0.0);
}

#[test]
fn export_pair_writes_portable_png_and_refuses_existing_sources() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("composition.png");
    let raster = || canvas_io::LoadedImage {
        rgba: vec![255; 8 * 4 * 4],
        width: 8,
        height: 4,
    };
    jobs::export_pair(&path, raster(), Framing::default()).unwrap();
    assert_eq!(canvas_io::load_image(&path).unwrap().width, 8);
    assert_eq!(
        canvas_io::read_framing(&path).unwrap(),
        Some(Framing::default())
    );
    let original = std::fs::read(&path).unwrap();
    assert!(jobs::export_pair(&path, raster(), Framing::default()).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), original);
    let invalid = dir.path().join("invalid.png");
    assert!(jobs::export_pair(
        &invalid,
        raster(),
        Framing {
            scale_pct: 0,
            ..Framing::default()
        }
    )
    .is_err());
    assert!(!invalid.exists());
}
