use super::*;

fn clip() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("clip.mp4");
    let status = canvas_io::media_command(
        canvas_io::ffmpeg_path().expect("FFmpeg installed for video tests"),
    )
    .args([
        "-v",
        "error",
        "-f",
        "lavfi",
        "-i",
        "testsrc2=size=64x36:rate=20:duration=1",
        "-c:v",
        "libx264",
        "-pix_fmt",
        "yuv420p",
    ])
    .arg(&path)
    .status()
    .unwrap();
    assert!(status.success());
    (dir, path)
}

fn wait_frame(video: &mut Video) -> decoder::Frame {
    for _ in 0..1000 {
        if let Some(frame) = video.poll() {
            return frame.unwrap();
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    panic!("video frame timed out");
}

#[test]
#[ignore = "requires FFmpeg and ffprobe"]
fn playback_seek_pause_and_background_use_real_video_frames() {
    let (_dir, path) = clip();
    let ctx = egui::Context::default();
    let mut video = Video::new(path, (64, 36), 1.0);
    video.seek(0.0, true, &ctx);
    let first = wait_frame(&mut video);
    let later = wait_frame(&mut video);
    assert!(later.time > first.time);
    assert_eq!(
        later.background.pixels,
        crate::framing::preview::background(&later.image)
            .unwrap()
            .pixels
    );
    video.pause();
    let paused = video.position;
    std::thread::sleep(std::time::Duration::from_millis(100));
    assert!(video.poll().is_none());
    assert_eq!(video.position, paused);
    video.seek(0.6, false, &ctx);
    let sought = wait_frame(&mut video);
    assert_eq!(sought.time, 0.6);
    assert_ne!(first.image.rgba, sought.image.rgba);
    video.seek(5.0, false, &ctx);
    assert!(video.position < 1.0);
}

#[test]
#[ignore = "requires FFmpeg and ffprobe"]
fn gallery_video_framing_exposes_transport_controls() {
    let (_dir, path) = clip();
    let ctx = egui::Context::default();
    let mut session = crate::framing::Session::open(path, &ctx);
    for _ in 0..1000 {
        session.poll(&ctx);
        if session.ready() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(session.ready(), "{:?}", session.error);
    let mut frame = || ctx.run_ui(egui::RawInput::default(), |ui| session.controls(ui));
    frame();
    let output = frame();
    let labels: Vec<_> = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.text()),
            _ => None,
        })
        .collect();
    assert!(labels.contains(&"Play"));
    assert!(labels.contains(&"Restart"));
    assert!(labels.iter().any(|s| s.contains("00:00.0 / 00:01.0")));
}

#[test]
fn transport_controls_are_available_without_starting_a_decoder() {
    let ctx = egui::Context::default();
    let mut video = Video::new(PathBuf::from("clip.mp4"), (1920, 1080), 10.0);
    let mut frame = || ctx.run_ui(egui::RawInput::default(), |ui| video.controls(ui));
    frame();
    let output = frame();
    assert!(output
        .shapes
        .iter()
        .any(|shape| matches!(&shape.shape,egui::Shape::Text(text) if text.galley.text()=="Play")));
    assert!(!video.playing);
    assert!(video.worker.is_none());
}
