use super::*;
use canvas_core::{LayerContent, Transform, VideoContent};

#[test]
fn saved_design_survives_download_cleanup_and_project_move() {
    let staging = tempfile::tempdir().unwrap();
    let src = staging.path().join("source.mp4");
    std::fs::write(&src, b"original video bytes").unwrap();
    let project = tempfile::tempdir().unwrap();
    let dst = project.path().join("design.canvas");
    let mut payload = crate::blank_design(96.0, 64.0);
    payload
        .document
        .add_layer(
            "video",
            Transform::new(10.0, 5.0, 64.0, 48.0),
            LayerContent::Video(VideoContent {
                source_path: Some(src.clone()),
                natural_width: 64,
                natural_height: 48,
                crop: None,
                duration_secs: Some(8.0),
                poster_time: 2.0,
                trim_start: 2.0,
                trim_end: Some(5.0),
            }),
        )
        .unwrap();
    let links = preserve_video_sources(&mut payload, &dst).unwrap();
    assert_eq!(links.len(), 1);
    preserve_video_sources(&mut payload, &dst).unwrap();
    assert_eq!(
        std::fs::read_dir(project.path().join(".download"))
            .unwrap()
            .count(),
        1
    );
    crate::write_design(&dst, &payload).unwrap();
    std::fs::remove_file(src).unwrap();
    let moved = tempfile::tempdir().unwrap();
    let moved_project = moved.path().join("moved");
    // Rename de directorio real: las referencias absolutas viejas ya no sirven.
    std::fs::rename(project.path(), &moved_project).unwrap();
    let restored = crate::read_design(&moved_project.join("design.canvas")).unwrap();
    let layer = &restored.document.page().unwrap().layers[0];
    let LayerContent::Video(video) = &layer.content else {
        panic!("video")
    };
    let path = video.source_path.as_ref().unwrap();
    assert!(path.starts_with(&moved_project));
    assert_eq!(std::fs::read(path).unwrap(), b"original video bytes");
    assert_eq!((video.trim_start, video.trim_end), (2.0, Some(5.0)));
    assert_eq!(layer.transform, Transform::new(10.0, 5.0, 64.0, 48.0));
}

#[test]
fn design_writer_refuses_mp4_before_modifying_any_bytes() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("download.mp4");
    std::fs::write(&path, b"video remains untouched").unwrap();
    assert!(crate::write_design(&path, &crate::blank_design(8.0, 8.0)).is_err());
    assert_eq!(std::fs::read(path).unwrap(), b"video remains untouched");
}
