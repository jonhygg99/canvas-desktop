use super::*;
use std::time::{Duration, Instant};

fn editor(duration: Option<f64>) -> VideoEdit {
    VideoEdit::open(
        "clip.mp4",
        "Clip".to_owned(),
        PathBuf::from("clip.mp4"),
        duration,
        (1920.0, 1080.0),
        &Document::new(1920.0, 1080.0),
    )
}

fn frame_paths(count: usize) -> Vec<PathBuf> {
    (0..count)
        .map(|index| PathBuf::from(format!("frame-{index:03}.png")))
        .collect()
}

#[test]
fn playback_keeps_elapsed_time_between_preview_frames() {
    let mut edit = editor(Some(10.0));
    edit.playing = true;
    edit.frame_fps = 2.0;
    let ctx = egui::Context::default();

    // Un repintado temprano no debe perder los 50 ms ya transcurridos.
    // No dormimos ni exigimos un límite superior dependiente del scheduler.
    edit.last_tick = Some(Instant::now() - Duration::from_millis(50));
    advance_playhead(&mut edit, &ctx);

    assert!(
        edit.playhead >= 0.04,
        "el playhead perdió el tiempo entre miniaturas: {}",
        edit.playhead
    );
}

#[test]
fn frame_index_uses_absolute_video_time_after_trimming() {
    let mut edit = editor(Some(10.0));
    edit.frames = frame_paths(20);
    edit.frame_fps = 2.0;
    edit.trim_start = 2.0;
    edit.playhead = 3.0;

    assert_eq!(frame_index(&edit), Some(6));
}

#[test]
fn extracted_frames_preserve_restored_trim_when_duration_was_unknown() {
    let mut doc = Document::new(1920.0, 1080.0);
    doc.add_layer(
        "Clip",
        Transform::new(0.0, 0.0, 1920.0, 1080.0),
        LayerContent::Video(canvas_core::VideoContent {
            source_path: Some(PathBuf::from("clip.mp4")),
            natural_width: 1920,
            natural_height: 1080,
            crop: None,
            duration_secs: Some(30.0),
            poster_time: 2.0,
            trim_start: 2.0,
            trim_end: Some(8.0),
        }),
    )
    .unwrap();
    let mut edit = VideoEdit::open(
        "clip.mp4",
        "Clip".to_owned(),
        PathBuf::from("clip.mp4"),
        None,
        (1920.0, 1080.0),
        &doc,
    );
    assert_eq!((edit.trim_start, edit.trim_end), (2.0, 8.0));

    edit.set_frames(frame_paths(60), 2.0, 30.0, Some((1920.0, 1080.0)));

    assert_eq!((edit.trim_start, edit.trim_end), (2.0, 8.0));
    assert_eq!(edit.playhead, 2.0);
}

#[test]
fn accept_uses_poster_at_trim_start_instead_of_first_video_frame() {
    let mut edit = editor(Some(10.0));
    edit.frames = frame_paths(20);
    edit.frame_fps = 2.0;
    edit.trim_start = 2.0;
    // El póster representa el inicio del clip aceptado, incluso al pausar
    // la vista previa en otro momento del vídeo.
    edit.playhead = 5.0;

    let accept = build_accept(&edit);

    assert_eq!(accept.poster, edit.frames[4]);
}

#[test]
fn cached_frame_textures_remain_available_without_reopening_source_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("preview.png");
    image::RgbaImage::from_pixel(8, 8, image::Rgba([40, 80, 120, 255]))
        .save(&path)
        .unwrap();
    let mut edit = editor(Some(1.0));
    edit.frames = vec![path.clone()];
    let ctx = egui::Context::default();
    let initial = wait_for_textures(&mut edit, &ctx, 0);
    assert!(
        initial.1.is_some(),
        "el fondo borroso también debe cargarse"
    );

    // Una textura cacheada ya no depende del fichero decodificado.
    std::fs::remove_file(&path).unwrap();

    assert_eq!(frame_textures(&mut edit, &ctx, 0), Some(initial));
}

fn wait_for_textures(
    edit: &mut VideoEdit,
    ctx: &egui::Context,
    index: usize,
) -> (egui::TextureId, Option<egui::TextureId>) {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(textures) = frame_textures(edit, ctx, index) {
            if edit.blur <= 0.5 || textures.1.is_some() {
                return textures;
            }
        }
        assert!(
            Instant::now() < deadline,
            "preview no llegó: {:?}",
            edit.frames_error
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn changing_blur_reuses_decoded_pixels_without_reopening_the_png() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("preview.png");
    image::RgbaImage::from_pixel(8, 8, image::Rgba([40, 80, 120, 255]))
        .save(&path)
        .unwrap();
    let mut edit = editor(Some(1.0));
    edit.frames = vec![path.clone()];
    let ctx = egui::Context::default();
    let initial = wait_for_textures(&mut edit, &ctx, 0);
    std::fs::remove_file(path).unwrap();
    edit.blur = 25.0;
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let current = frame_textures(&mut edit, &ctx, 0).unwrap();
        assert_eq!(current.0, initial.0);
        if current.1 != initial.1 {
            assert!(current.1.is_some());
            break;
        }
        assert!(
            Instant::now() < deadline,
            "blur no llegó: {:?}",
            edit.frames_error
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn quantize_snaps_to_frame_grid() {
    assert_eq!(quantize_playhead(0.6, 0.0, 2.0), 0.5);
    assert_eq!(quantize_playhead(2.0, 2.0, 2.0), 2.0);
    assert_eq!(quantize_playhead(5.0, 0.0, 0.0), 5.0);
}

#[test]
fn grown_rect_enlarges_around_contain_center() {
    // Contain 607.5×1080 en (656.25, 0): zoom 1 = igual, zoom 2 = doble
    // centrado (la caja de preview lo recorta).
    assert_eq!(
        grown_rect(656.25, 0.0, 607.5, 1080.0, 1.0),
        (656.25, 0.0, 607.5, 1080.0)
    );
    assert_eq!(
        grown_rect(656.25, 0.0, 607.5, 1080.0, 2.0),
        (352.5, -540.0, 1215.0, 2160.0)
    );
}

#[test]
fn mappings_round_trip() {
    assert_eq!(blur_radius_for_slider(100.0), 50.0);
    assert_eq!(slider_for_radius(50.0), 100.0);
    // 9:16 en Full HD: toca arriba/abajo, bandas laterales.
    let (x, y, w, h) = contain_rect(1920.0, 1080.0, 1080.0, 1920.0);
    assert_eq!((x, y, w, h), (656.25, 0.0, 607.5, 1080.0));
    // Zoom 2 sobre la base contain, centrado.
    let t = zoom_transform(1080.0, 1920.0, 1920.0, 1080.0, 2.0);
    assert_eq!(
        (t.x, t.y, t.width, t.height),
        (352.5, -540.0, 1215.0, 2160.0)
    );
    assert_eq!(
        zoom_for_transform(1215.0, 1080.0, 1920.0, 1920.0, 1080.0),
        2.0
    );
    assert_eq!(
        zoom_for_transform(607.5, 1080.0, 1920.0, 1920.0, 1080.0),
        1.0
    );
}

#[test]
fn layer_for_clip_matches_by_file_name() {
    use canvas_core::{LayerContent, Transform, VideoContent};
    let mut doc = Document::new(800.0, 600.0);
    let link = |f: &str| VideoContent {
        source_path: Some(PathBuf::from(f)),
        natural_width: 2,
        natural_height: 2,
        crop: None,
        duration_secs: Some(10.0),
        poster_time: 0.0,
        trim_start: 0.0,
        trim_end: None,
    };
    let shape = || {
        LayerContent::Shape(canvas_core::ShapeContent {
            kind: canvas_core::ShapeKind::Rect,
            ..Default::default()
        })
    };
    let a = doc
        .add_layer("a", Transform::new(0.0, 0.0, 8.0, 6.0), shape())
        .unwrap();
    doc.layer_mut(a).unwrap().content = LayerContent::Video(link("C:/v/clip.mp4"));
    let b = doc
        .add_layer("b", Transform::new(0.0, 0.0, 8.0, 6.0), shape())
        .unwrap();
    doc.layer_mut(b).unwrap().content = LayerContent::Video(link("C:/v/clip.mp4"));
    assert_eq!(layer_for_clip(&doc, "clip.mp4"), Some(b));
    assert_eq!(layer_for_clip(&doc, "other.mp4"), None);
}

#[test]
fn open_restores_sliders_from_layers() {
    use canvas_core::{ImageContent, LayerContent, Transform, VideoContent};
    let mut doc = Document::new(1920.0, 1080.0);
    // Fondo receta imágenes al fondo + vídeo 9:16 en contain.
    let bg = doc
        .add_layer(
            "Blurred background",
            Transform::new(0.0, 0.0, 1920.0, 1080.0),
            LayerContent::Image(ImageContent {
                source_path: None,
                natural_width: 64,
                natural_height: 36,
                crop: None,
            }),
        )
        .unwrap();
    doc.layer_mut(bg).unwrap().effects.blur_radius = 20.0;
    doc.add_layer(
        "Clip",
        Transform::new(480.0, 0.0, 960.0, 1080.0),
        LayerContent::Video(VideoContent {
            source_path: Some(PathBuf::from("C:/v/clip.mp4")),
            natural_width: 32,
            natural_height: 36,
            crop: None,
            duration_secs: Some(30.0),
            poster_time: 0.0,
            trim_start: 2.0,
            trim_end: Some(8.0),
        }),
    )
    .unwrap();
    let edit = VideoEdit::open(
        "clip.mp4",
        "Clip".to_owned(),
        PathBuf::from("C:/v/clip.mp4"),
        Some(30.0),
        (1920.0, 1080.0),
        &doc,
    );
    assert!(edit.matches("clip.mp4"));
    assert!(!edit.matches("other.mp4"));
    assert_eq!((edit.trim_start, edit.trim_end), (2.0, 8.0));
    // Blur del fondo (20→40), zoom 1 contra la base contain 960.
    assert_eq!(edit.blur, 40.0);
    assert_eq!(edit.zoom, 1.0);
}

#[test]
fn bg_blur_is_zero_without_background() {
    let doc = Document::new(800.0, 600.0);
    assert_eq!(bg_blur_radius(&doc), 0.0);
}

#[test]
fn default_accept_is_neutral_full_hd() {
    let accept = default_accept(
        PathBuf::from("C:/v/clip.mp4"),
        "clip".to_owned(),
        (1920.0, 1080.0),
    );
    assert_eq!(accept.trim_start, 0.0);
    assert_eq!(accept.trim_end, None);
    assert_eq!(accept.blur_radius, 50.0);
    assert_eq!(accept.zoom, 1.0);
    assert_eq!(accept.size, (1920.0, 1080.0));
    assert_eq!(accept.video_size, None);
}

fn transport_frame(
    ctx: &egui::Context,
    edit: &mut VideoEdit,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            advance_playhead(edit, ui.ctx());
            transport_ui(edit, ui);
        },
    )
}

fn click_transport(ctx: &egui::Context, edit: &mut VideoEdit, label: &str) {
    let output = transport_frame(ctx, edit, Vec::new());
    let pos = output
        .shapes
        .iter()
        .find_map(|shape| {
            if let egui::epaint::Shape::Text(text) = &shape.shape {
                (text.galley.job.text == label).then(|| text.pos + text.galley.size() / 2.0)
            } else {
                None
            }
        })
        .expect("botón de transporte visible");
    for pressed in [true, false] {
        transport_frame(
            ctx,
            edit,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
}

#[test]
fn play_after_adjusting_end_previews_the_whole_trim() {
    let mut edit = editor(Some(30.0));
    edit.set_frames(frame_paths(30), 1.0, 30.0, Some((16.0, 9.0)));
    edit.set_trim_edge(TrimEdge::Start, 2.0);
    edit.set_trim_duration(7.0);
    click_transport(&egui::Context::default(), &mut edit, "Play");
    assert!(edit.playing);
    assert_eq!(edit.playhead, 2.0);
}

#[test]
fn real_play_pause_and_restart_buttons_work_between_frequent_repaints() {
    let mut edit = editor(Some(10.0));
    edit.set_frames(frame_paths(20), 2.0, 10.0, Some((16.0, 9.0)));
    edit.trim_start = 2.0;
    edit.playhead = 2.0;
    let ctx = egui::Context::default();
    click_transport(&ctx, &mut edit, "Play");
    assert!(edit.playing);
    for _ in 0..12 {
        edit.last_tick = Some(Instant::now() - Duration::from_millis(50));
        transport_frame(&ctx, &mut edit, Vec::new());
    }
    assert!(edit.playhead >= 2.6);
    assert!(frame_index(&edit).unwrap() >= 5);
    click_transport(&ctx, &mut edit, "Pause");
    assert!(!edit.playing);
    click_transport(&ctx, &mut edit, "Restart");
    assert!(edit.playing);
    assert_eq!(edit.playhead, edit.trim_start);
}

#[test]
fn download_preview_worker_and_egui_render_real_video_frames() {
    let Some(ffmpeg) = canvas_io::ffmpeg_path() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("clip.mp4");
    let result = canvas_io::media_command(ffmpeg)
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
            "yuv420p",
        ])
        .arg(&path)
        .output()
        .unwrap();
    assert!(result.status.success());
    let ctx = egui::Context::default();
    let (tx, rx) = std::sync::mpsc::channel();
    loader::spawn_ytdlp_frames("clip.mp4".to_owned(), path, None, tx, ctx.clone());
    let outcome = match rx.recv_timeout(Duration::from_secs(10)).unwrap() {
        AppMsg::YtdlpFramesReady(outcome) => outcome,
        AppMsg::YtdlpFramesFailed { error, .. } => panic!("preview: {error}"),
        _ => panic!("mensaje inesperado"),
    };
    assert_eq!(outcome.files.len(), 4);
    assert_eq!(outcome.video_size, Some((64.0, 48.0)));
    let cache_dir = outcome.files[0].parent().unwrap().to_owned();
    let mut edit = editor(Some(2.0));
    edit.set_frames(
        outcome.files,
        outcome.fps,
        outcome.duration,
        outcome.video_size,
    );
    let initial = wait_for_textures(&mut edit, &ctx, 0);
    edit.playhead = 1.0;
    let index = frame_index(&edit).unwrap();
    assert_eq!(index, 2);
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let current = frame_textures(&mut edit, &ctx, index).unwrap();
        if current.0 != initial.0 && current.1.is_some() {
            break;
        }
        assert!(Instant::now() < deadline, "no llegó el segundo fotograma");
        std::thread::sleep(Duration::from_millis(5));
    }
    let output = ctx.run_ui(egui::RawInput::default(), |ui| preview_ui(&mut edit, ui));
    assert!(output.shapes.len() >= 3);
    drop(edit);
    std::fs::remove_dir_all(cache_dir).unwrap();
}
