use super::*;
use canvas_core::{Selection, Transform};

fn video_state() -> (EditorState, LayerId) {
    let mut state = EditorState::new_blank(200.0, 120.0);
    let content = serde_json::from_value(serde_json::json!({
        "source_path": "clip.mp4", "natural_width": 2, "natural_height": 2,
        "duration_secs": 10.0, "poster_time": 0.0
    }))
    .unwrap();
    let id = state
        .doc
        .add_layer(
            "Video",
            Transform::new(0.0, 0.0, 100.0, 100.0),
            LayerContent::Video(content),
        )
        .unwrap();
    state.selection = Selection::single(id);
    state
        .images
        .insert(id, canvas_render::image_data_from_rgba(vec![0; 16], 2, 2));
    (state, id)
}

fn pending_frame(state: &mut EditorState, id: LayerId) -> mpsc::Receiver<()> {
    let LayerContent::Video(video) = state.doc.layer(id).unwrap().content.clone() else {
        unreachable!()
    };
    let (cancel, rx) = mpsc::channel();
    state.video_playing_layer = Some(id);
    state.video_playback = Some(VideoPlayback {
        layer: id,
        slot: state.active_slot_id,
        video,
        cancel,
        playing: false,
        frames: Arc::new(Mutex::new(Some(Ok(Some((
            0.5,
            LoadedImage {
                rgba: vec![255; 16],
                width: 2,
                height: 2,
            },
        )))))),
    });
    rx
}

#[test]
fn playback_advances_without_selection_or_a_properties_panel() {
    let (mut state, id) = video_state();
    let _cancel = pending_frame(&mut state, id);
    state.selection = Selection::default();
    state.tick_video(&egui::Context::default());
    let LayerContent::Video(v) = &state.doc.layer(id).unwrap().content else {
        unreachable!()
    };
    assert_eq!(v.poster_time, 0.5);
    assert_eq!(state.images[&id].data.data(), &[255; 16]);
    assert!(!state.is_dirty(), "playback is not a document edit");
    assert_eq!(
        state.video_layer(),
        Some(id),
        "transport remains discoverable"
    );
}

#[test]
fn blurred_video_background_receives_the_same_frame_without_losing_effects() {
    let (mut state, id) = video_state();
    state.set_blurred_background(true);
    let bg = state.background_layer.unwrap();
    let transform = state.doc.layer(bg).unwrap().transform;
    let effects = state.doc.layer(bg).unwrap().effects;
    state.apply_video_frame(
        id,
        2.0,
        LoadedImage {
            rgba: vec![64; 16],
            width: 2,
            height: 2,
        },
    );
    assert_eq!(state.images[&bg].data.id(), state.images[&id].data.id());
    assert_eq!(state.doc.layer(bg).unwrap().transform, transform);
    assert_eq!(state.doc.layer(bg).unwrap().effects, effects);
    assert!(
        matches!(&state.doc.layer(bg).unwrap().content, LayerContent::Video(v) if v.poster_time == 2.0)
    );
}

#[test]
fn late_frames_cannot_modify_another_canvas_with_the_same_layer_id() {
    let (mut state, id) = video_state();
    let cancelled = pending_frame(&mut state, id);
    state.active_slot_id += 1;
    state.tick_video(&egui::Context::default());
    assert_eq!(state.images[&id].data.data(), &[0; 16]);
    assert!(state.video_playback.is_none());
    assert!(cancelled.try_recv().is_ok());
}

#[test]
fn switching_canvas_cancels_playback_and_its_pending_frame() {
    let (mut state, id) = video_state();
    let cancelled = pending_frame(&mut state, id);
    let slot = state.take_slot();
    state.put_slot(slot);
    assert!(state.video_playback.is_none());
    assert_eq!(
        state.video_playing_layer,
        Some(id),
        "the newly active video is ready to autoplay"
    );
    assert!(cancelled.try_recv().is_ok());
}

#[test]
fn legacy_video_documents_and_trimmed_ranges_are_preserved() {
    let (mut state, id) = video_state();
    let LayerContent::Video(video) = &mut state.doc.layer_mut(id).unwrap().content else {
        unreachable!()
    };
    assert_eq!(video.playback_range(), (0.0, Some(10.0)));
    video.trim_start = 2.0;
    video.trim_end = Some(6.0);
    assert_eq!(video.playback_range(), (2.0, Some(6.0)));
    let reloaded: VideoContent =
        serde_json::from_str(&serde_json::to_string(video).unwrap()).unwrap();
    assert_eq!(reloaded.playback_range(), (2.0, Some(6.0)));
    video.trim_end = Some(200.0);
    assert_eq!(video.playback_range(), (2.0, Some(10.0)));
    video.trim_start = f64::NAN;
    video.trim_end = Some(-1.0);
    assert_eq!(video.playback_range(), (0.0, Some(10.0)));
}

#[test]
fn real_decoder_advances_the_canvas_and_replaces_an_older_seek() {
    let Some(ffmpeg) = canvas_io::ffmpeg_path() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("preview.mp4");
    assert!(canvas_io::media_command(ffmpeg)
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=64x48:rate=20",
            "-t",
            "2",
            "-pix_fmt",
            "yuv420p"
        ])
        .arg(&path)
        .output()
        .unwrap()
        .status
        .success());
    let (mut state, id) = video_state();
    let LayerContent::Video(video) = &mut state.doc.layer_mut(id).unwrap().content else {
        unreachable!()
    };
    video.source_path = Some(path);
    video.natural_width = 64;
    video.natural_height = 48;
    video.duration_secs = Some(2.0);
    video.trim_start = 0.25;
    video.trim_end = Some(0.75);
    video.poster_time = 0.25;
    let video = video.clone();
    let ctx = egui::Context::default();
    state.video_playing_layer = Some(id);
    state.video_playback = Some(VideoPlayback::start(
        id,
        state.active_slot_id,
        video,
        true,
        ctx.clone(),
    ));
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        state.tick_video(&ctx);
        if matches!(&state.doc.layer(id).unwrap().content, LayerContent::Video(v) if v.poster_time >= 0.3)
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        matches!(&state.doc.layer(id).unwrap().content, LayerContent::Video(v) if v.poster_time >= 0.3 && v.poster_time < 0.75)
    );
    assert_eq!(
        (state.images[&id].width, state.images[&id].height),
        (64, 48)
    );
    state.seek_video(id, 0.3, false, &ctx);
    state.seek_video(id, 0.6, false, &ctx);
    let deadline = Instant::now() + Duration::from_secs(3);
    while state
        .video_playback
        .as_ref()
        .unwrap()
        .frames
        .lock()
        .unwrap()
        .is_none()
        && Instant::now() < deadline
    {
        std::thread::sleep(Duration::from_millis(10));
    }
    state.tick_video(&ctx);
    let expected = canvas_io::load_video_frame(&dir.path().join("preview.mp4"), 0.6).unwrap();
    assert_eq!(state.images[&id].data.data(), expected.rgba.as_slice());
    assert!(
        matches!(&state.doc.layer(id).unwrap().content, LayerContent::Video(v) if v.poster_time == 0.6)
    );
    assert!(state.video_playing_layer.is_none());
    assert!(!state.is_dirty());
}
