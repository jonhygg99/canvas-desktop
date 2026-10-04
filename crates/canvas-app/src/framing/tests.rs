use super::*;

#[test]
fn late_results_from_a_closed_session_cannot_reach_another_document() {
    let ctx = egui::Context::default();
    let (old_tx, old_rx) = std::sync::mpsc::channel();
    let mut old = Session::new(Some(PathBuf::from("first.canvas")), true);
    old.receiver = Some(old_rx);
    let (current_tx, current_rx) = std::sync::mpsc::channel();
    let mut current = Session::new(Some(PathBuf::from("second.canvas")), true);
    current.receiver = Some(current_rx);
    drop(old);
    let result = |scale_pct| {
        Ok(jobs::Outcome::Saved {
            value: Framing {
                scale_pct,
                ..Default::default()
            },
            path: PathBuf::from(".framing/second.canvas.json"),
            sidecar_only: true,
        })
    };
    assert!(old_tx.send(result(50)).is_err());
    current.poll(&ctx);
    assert!(current.saved.is_none());
    current_tx.send(result(75)).unwrap();
    current.poll(&ctx);
    assert_eq!(current.saved.unwrap().scale_pct, 75);
    assert_eq!(
        current.path.as_deref(),
        Some(std::path::Path::new("second.canvas"))
    );
}

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
    assert!(!canvas_io::framing_path(&invalid).unwrap().exists());
}

#[test]
fn export_failure_preserves_existing_sidecars_and_cleans_reserved_png() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("existing.png");
    let raster = || canvas_io::LoadedImage {
        rgba: vec![255; 16],
        width: 2,
        height: 2,
    };
    let sidecar = canvas_io::write_framing(&path, Framing::default()).unwrap();
    let before = std::fs::read(&sidecar).unwrap();
    assert!(jobs::export_pair(&path, raster(), Framing::default()).is_err());
    assert!(!path.exists());
    assert_eq!(std::fs::read(&sidecar).unwrap(), before);
    std::fs::remove_file(sidecar).unwrap();
    std::fs::remove_dir(dir.path().join(".framing")).unwrap();
    std::fs::write(dir.path().join(".framing"), b"blocked directory").unwrap();
    assert!(jobs::export_pair(&path, raster(), Framing::default()).is_err());
    assert!(!path.exists());
}

#[test]
fn gallery_save_button_persists_the_edited_value() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("clip.png");
    let ctx = egui::Context::default();
    let mut session = Session::new(Some(path.clone()), false);
    session.value.x_pct = 24.0;
    session.texture = Some(ctx.load_texture(
        "test",
        egui::ColorImage::filled([2, 2], egui::Color32::WHITE),
        egui::TextureOptions::LINEAR,
    ));
    session.source = Some(canvas_io::LoadedImage {
        rgba: vec![255; 16],
        width: 2,
        height: 2,
    });
    let mut gallery = crate::gallery::framing::GalleryFramings::default();
    gallery.session = Some(session);
    let mut frame = |events| {
        ctx.run_ui(
            egui::RawInput {
                events,
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 900.0),
                )),
                ..Default::default()
            },
            |ui| {
                gallery.modal(ui.ctx());
            },
        )
    };
    frame(vec![]);
    let output = frame(vec![]);
    let pos = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.text() == "Save framing" => {
                Some(text.pos + text.galley.size() / 2.0)
            }
            _ => None,
        })
        .expect("save button rendered");
    frame(vec![egui::Event::PointerMoved(pos)]);
    frame(vec![egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    }]);
    frame(vec![egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    }]);
    for _ in 0..100 {
        if canvas_io::read_framing(&path).unwrap().is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(canvas_io::read_framing(&path).unwrap().unwrap().x_pct, 24.0);
}

#[test]
fn dragging_preview_is_cumulative_and_creates_one_undo_step() {
    let ctx = egui::Context::default();
    let mut session = Session::new(None, false);
    session.value.scale_pct = 50;
    let original = session.value;
    session.texture = Some(ctx.load_texture(
        "drag-source",
        egui::ColorImage::filled([192, 108], egui::Color32::WHITE),
        egui::TextureOptions::LINEAR,
    ));
    let mut frame = |events| {
        ctx.run_ui(
            egui::RawInput {
                events,
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900.0, 900.0),
                )),
                ..Default::default()
            },
            |ui| session.canvas(ui),
        )
    };
    frame(vec![]);
    let start = egui::pos2(450.0, 450.0);
    frame(vec![egui::Event::PointerMoved(start)]);
    frame(vec![egui::Event::PointerButton {
        pos: start,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    }]);
    for x in [440.0, 430.0, 420.0, 410.0] {
        frame(vec![egui::Event::PointerMoved(egui::pos2(x, 450.0))]);
    }
    frame(vec![]);
    frame(vec![]);
    frame(vec![egui::Event::PointerButton {
        pos: egui::pos2(410.0, 450.0),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    }]);
    assert!(
        (session.value.x_pct - 40.0 / 506.25 * 100.0).abs() < 0.5,
        "offset was {}",
        session.value.x_pct
    );
    assert_eq!(session.undo.len(), 1);
    session.undo();
    assert_eq!(session.value, original);
}
