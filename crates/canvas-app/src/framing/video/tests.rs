use super::*;

#[test]
fn trim_is_bounded_to_original_video_and_does_not_mark_it_saved() {
    let mut video = Video::new(PathBuf::from("original.mp4"), (1920, 1080), 10.0);
    let original = video.trim;
    video.playing = true;
    let trim = canvas_io::VideoTrim {
        start_s: 2.0,
        end_s: 8.0,
    };
    video.apply_trim(trim).unwrap();
    assert_eq!(video.trim, trim);
    assert_eq!(video.position, 2.0);
    assert!(!video.playing);
    assert_eq!(video.saved_trim, original);
    assert!(video.muted);
    assert!(video
        .apply_trim(canvas_io::VideoTrim {
            start_s: 2.0,
            end_s: 11.0
        })
        .is_err());
    assert_eq!(video.trim, trim);
}

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

fn wait_frame(video: &mut Video, ctx: &egui::Context) -> decoder::Frame {
    for _ in 0..1000 {
        if let Some(frame) = video.poll(ctx) {
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
    let first = wait_frame(&mut video, &ctx);
    let later = wait_frame(&mut video, &ctx);
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
    assert!(video.poll(&ctx).is_none());
    assert_eq!(video.position, paused);
    video.seek(0.6, false, &ctx);
    let sought = wait_frame(&mut video, &ctx);
    assert_eq!(sought.time, 0.6);
    assert_ne!(first.image.rgba, sought.image.rgba);
    video.seek(5.0, false, &ctx);
    assert!(video.position < 1.0);
    video.seek(0.9, true, &ctx);
    let last = wait_frame(&mut video, &ctx);
    assert!(last.time >= 0.9);
    let mut restarted = false;
    for _ in 0..10 {
        if wait_frame(&mut video, &ctx).time == 0.0 {
            restarted = true;
            break;
        }
    }
    assert!(restarted, "the full video should loop at EOF");
    assert!(video.playing);
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
    assert!(session.video.as_ref().unwrap().playing);
    assert!(labels.contains(&"Pause"));
    assert!(labels.contains(&"Restart"));
    assert!(labels.iter().any(|s| s.contains(" / 00:01.0")));
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

#[test]
#[ignore = "requires FFmpeg and ffprobe"]
fn original_video_trim_saves_reopens_and_limits_playback() {
    let (_dir, path) = clip();
    let original = std::fs::read(&path).unwrap();
    let ctx = egui::Context::default();
    let mut session = crate::framing::Session::open(path.clone(), &ctx);
    let wait = |session: &mut crate::framing::Session| {
        for _ in 0..1000 {
            session.poll(&ctx);
            if !session.busy() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("session worker timed out");
    };
    wait(&mut session);
    let trim = canvas_io::VideoTrim {
        start_s: 0.25,
        end_s: 0.75,
    };
    session.video.as_mut().unwrap().apply_trim(trim).unwrap();
    session.save(&ctx);
    wait(&mut session);
    assert!(session.error.is_none(), "{:?}", session.error);
    assert_eq!(session.video.as_ref().unwrap().saved_trim, trim);
    assert_eq!(
        canvas_io::read_framing_with_trim(&path).unwrap().unwrap().1,
        Some(trim)
    );
    drop(session);
    let mut reopened = crate::framing::Session::open(path.clone(), &ctx);
    wait(&mut reopened);
    let video = reopened.video.as_mut().unwrap();
    assert_eq!(video.trim, trim);
    assert_eq!(video.position, trim.start_s);
    assert!(video.playing, "reopened video should autoplay");
    let first = wait_frame(video, &ctx);
    assert!(first.time >= trim.start_s && first.time < trim.end_s);
    let mut previous = first.time;
    let mut loops = 0;
    for _ in 0..1000 {
        if let Some(frame) = video.poll(&ctx) {
            let frame = frame.unwrap();
            assert!(frame.time >= trim.start_s && frame.time < trim.end_s);
            if frame.time < previous {
                loops += 1;
                assert_eq!(frame.time, trim.start_s);
            }
            previous = frame.time;
            if loops == 2 {
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(
        loops, 2,
        "video should loop repeatedly inside the saved trim"
    );
    assert!(video.playing);
    video.pause();
    let paused = video.position;
    std::thread::sleep(std::time::Duration::from_millis(600));
    assert!(video.poll(&ctx).is_none());
    assert!(!video.playing);
    assert_eq!(video.position, paused);
    assert_eq!(std::fs::read(&path).unwrap(), original);
}

#[test]
fn whole_composition_framing_does_not_replace_layers_with_the_raw_video() {
    let ctx = egui::Context::default();
    let mut session = crate::framing::Session::from_composition(
        Some(PathBuf::from("missing-video.mp4")),
        canvas_io::LoadedImage {
            rgba: vec![255; 16],
            width: 2,
            height: 2,
        },
        &ctx,
    );
    for _ in 0..1000 {
        session.poll(&ctx);
        if !session.busy() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(session.ready(), "{:?}", session.error);
    assert!(session.video.is_none());
    assert!(session.exportable);
}

#[test]
#[ignore = "requires FFmpeg and ffprobe"]
fn real_gallery_play_and_pause_buttons_work_with_a_video_session() {
    let (_dir, path) = clip();
    let ctx = egui::Context::default();
    let mut gallery = crate::gallery::framing::GalleryFramings::default();
    gallery.session = Some(crate::framing::Session::open(path, &ctx));
    for _ in 0..1000 {
        gallery.session.as_mut().unwrap().poll(&ctx);
        if gallery.session.as_ref().unwrap().ready() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let frame = |events, gallery: &mut crate::gallery::framing::GalleryFramings| {
        ctx.run_ui(
            egui::RawInput {
                events,
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 900.0),
                )),
                ..Default::default()
            },
            |ui| gallery.modal(ui.ctx()),
        )
    };
    let click = |label: &str, gallery: &mut crate::gallery::framing::GalleryFramings| {
        frame(vec![], gallery);
        let output = frame(vec![], gallery);
        let pos = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text() == label => {
                    Some(text.pos + text.galley.size() / 2.0)
                }
                _ => None,
            })
            .expect("video transport button visible");
        frame(vec![egui::Event::PointerMoved(pos)], gallery);
        for pressed in [true, false] {
            frame(
                vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                }],
                gallery,
            );
        }
    };
    click("Pause", &mut gallery);
    assert!(
        !gallery
            .session
            .as_ref()
            .unwrap()
            .video
            .as_ref()
            .unwrap()
            .playing
    );
    click("Play", &mut gallery);
    assert!(
        gallery
            .session
            .as_ref()
            .unwrap()
            .video
            .as_ref()
            .unwrap()
            .playing
    );
}
