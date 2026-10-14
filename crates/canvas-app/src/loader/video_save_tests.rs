use super::*;
use canvas_core::{LayerContent, Transform, VideoContent};
use std::time::Duration;

fn request(src: &std::path::Path, dst: PathBuf, reserve: bool) -> VideoSaveRequest {
    let mut document = canvas_core::Document::new(96.0, 64.0);
    document.source_path = Some(src.to_owned());
    let id = document
        .add_layer(
            "clip",
            Transform::new(16.0, 8.0, 64.0, 48.0),
            LayerContent::Video(VideoContent {
                source_path: Some(src.to_owned()),
                natural_width: 64,
                natural_height: 48,
                crop: None,
                duration_secs: Some(2.0),
                poster_time: 0.25,
                trim_start: 0.25,
                trim_end: Some(1.25),
            }),
        )
        .unwrap();
    VideoSaveRequest {
        target: ImageInsertDest {
            generation: 7,
            slot_id: 2,
        },
        capture: (0, 0),
        reserve,
        payload: CanvasPayload {
            document,
            images: vec![(id.raw(), vec![80; 64 * 48 * 4], 64, 48)],
            background_layer: None,
            preview: None,
        },
        composition: VideoComposition {
            src: src.to_owned(),
            dst,
            start_s: 0.25,
            end_s: Some(1.25),
            page_w: 96,
            page_h: 64,
            fg_x: 16,
            fg_y: 8,
            fg_w: 64,
            fg_h: 48,
            bg_blur: 0.0,
        },
    }
}

fn fixture(dir: &std::path::Path) -> PathBuf {
    let ffmpeg = canvas_io::ffmpeg_path().expect("FFmpeg required for video save regression");
    let src = dir.join("download.mp4");
    let out = canvas_io::media_command(ffmpeg)
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=64x48:rate=10",
            "-t",
            "2",
            "-pix_fmt",
            "yuv420p",
        ])
        .arg(&src)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    src
}

fn done(rx: &std::sync::mpsc::Receiver<AppMsg>) -> VideoSaveDone {
    let AppMsg::VideoCompositionRendered(done) = rx.recv_timeout(Duration::from_secs(30)).unwrap()
    else {
        panic!("video completion")
    };
    done.result.as_ref().unwrap();
    *done
}

#[test]
fn video_saves_reserve_per_window_preserve_original_and_reopen_after_clear() {
    let staging = tempfile::tempdir().unwrap();
    let src = fixture(staging.path());
    let original = std::fs::read(&src).unwrap();
    let project = tempfile::tempdir().unwrap();
    let dst = project.path().join("clip.mp4");
    std::fs::write(&dst, b"external file appeared after insertion").unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    spawn_video_save(
        request(&src, dst.clone(), true),
        tx.clone(),
        egui::Context::default(),
    );
    spawn_video_save(
        request(&src, dst.clone(), true),
        tx,
        egui::Context::default(),
    );
    let a = done(&rx);
    let b = done(&rx);
    assert_ne!(a.dst, b.dst);
    assert_eq!(
        std::fs::read(&dst).unwrap(),
        b"external file appeared after insertion"
    );
    assert_eq!(std::fs::read(&src).unwrap(), original);
    assert_eq!(
        std::fs::read_dir(project.path().join(".download"))
            .unwrap()
            .count(),
        1
    );
    std::fs::remove_file(&src).unwrap();
    for saved in [&a, &b] {
        let canvas_io::OpenOutcome::Restored(restored) =
            canvas_io::open_document(&saved.dst, true).unwrap()
        else {
            panic!("editable sidecar")
        };
        let layer = &restored.document.page().unwrap().layers[0];
        let LayerContent::Video(v) = &layer.content else {
            panic!("video")
        };
        assert_eq!((v.trim_start, v.trim_end), (0.25, Some(1.25)));
        assert_eq!(layer.transform, Transform::new(16.0, 8.0, 64.0, 48.0));
        assert!(v.source_path.as_ref().unwrap().is_file());
        let size = canvas_io::probe_video_size(&saved.dst).unwrap();
        assert_eq!((size.0, size.1), (96, 64));
        assert!((size.2.unwrap() - 1.0).abs() < 0.15);
        // Segundo guardado desde la fuente duradera: mismo trim, sin doble recorte.
        let (tx, rx) = std::sync::mpsc::channel();
        spawn_video_save(
            request(v.source_path.as_deref().unwrap(), saved.dst.clone(), false),
            tx,
            egui::Context::default(),
        );
        done(&rx);
        assert!((canvas_io::probe_video_size(&saved.dst).unwrap().2.unwrap() - 1.0).abs() < 0.15);
    }
    assert!(std::fs::read_dir(project.path())
        .unwrap()
        .all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains("exporting")));
}

#[test]
fn failed_video_save_cleans_only_its_own_reservation() {
    let dir = tempfile::tempdir().unwrap();
    let dst = dir.path().join("clip.mp4");
    std::fs::write(&dst, b"existing").unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    spawn_video_save(
        request(&dir.path().join("missing.mp4"), dst.clone(), true),
        tx,
        egui::Context::default(),
    );
    let AppMsg::VideoCompositionRendered(done) = rx.recv_timeout(Duration::from_secs(10)).unwrap()
    else {
        panic!("completion")
    };
    assert!(done.result.is_err());
    assert!(!dir.path().join("clip 2.mp4").exists());
    assert_eq!(std::fs::read(dst).unwrap(), b"existing");
}
