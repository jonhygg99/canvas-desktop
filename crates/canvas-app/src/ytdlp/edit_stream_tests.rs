use super::*;
use std::hash::{Hash, Hasher};
use std::time::{Duration, Instant};

static STREAM_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct RealVideo {
    // El vídeo y las miniaturas deben vivir hasta que se cierre el editor.
    _video_dir: tempfile::TempDir,
    thumbnail_dir: PathBuf,
    _serial: std::sync::MutexGuard<'static, ()>,
}

impl Drop for RealVideo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.thumbnail_dir);
    }
}

fn real_video_editor() -> Option<(RealVideo, VideoEdit, egui::Context)> {
    // Estas mediciones de cadencia no compiten entre sí por el decoder.
    let serial = STREAM_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(ffmpeg) = canvas_io::ffmpeg_path() else {
        eprintln!("Prueba de reproducción omitida: FFmpeg no está instalado");
        return None;
    };
    let video_dir = tempfile::tempdir().unwrap();
    let path = video_dir.path().join("clip.mp4");
    let result = canvas_io::media_command(ffmpeg)
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=640x360:rate=30",
            "-t",
            "3",
            "-pix_fmt",
            "yuv420p",
        ])
        .arg(&path)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "ffmpeg: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let ctx = egui::Context::default();
    let (tx, rx) = std::sync::mpsc::channel();
    loader::spawn_ytdlp_frames("clip.mp4".to_owned(), path.clone(), None, tx, ctx.clone());
    let outcome = match rx.recv_timeout(Duration::from_secs(10)).unwrap() {
        AppMsg::YtdlpFramesReady(outcome) => outcome,
        AppMsg::YtdlpFramesFailed { error, .. } => panic!("preview: {error}"),
        _ => panic!("mensaje inesperado"),
    };
    // Se conserva la extracción barata para scrub/póster. El play debe
    // superar su cadencia sin convertirla en una extracción masiva.
    assert_eq!(outcome.fps, 2.0);
    assert_eq!(outcome.files.len(), 6);
    let thumbnail_dir = outcome.files[0].parent().unwrap().to_owned();
    let mut edit = VideoEdit::open(
        "clip.mp4",
        "Clip".to_owned(),
        path,
        None,
        (1920.0, 1080.0),
        &Document::new(1920.0, 1080.0),
    );
    edit.set_frames(
        outcome.files,
        outcome.fps,
        outcome.duration,
        outcome.video_size,
    );
    edit.trim_start = 0.25;
    edit.trim_end = 2.75;
    edit.playhead = edit.trim_start;
    Some((
        RealVideo {
            _video_dir: video_dir,
            thumbnail_dir,
            _serial: serial,
        },
        edit,
        ctx,
    ))
}

fn render_preview(edit: &mut VideoEdit, ctx: &egui::Context) -> egui::FullOutput {
    ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            ..Default::default()
        },
        |ui| {
            advance_playhead(edit, ui.ctx());
            preview_ui(edit, ui);
        },
    )
}

fn foreground_texture(output: &egui::FullOutput) -> Option<egui::TextureId> {
    // preview_ui pinta fondo y después vídeo nítido: la última malla
    // visible es el foreground, independientemente del ID de la textura.
    output.shapes.iter().rev().find_map(|shape| {
        let egui::epaint::Shape::Mesh(mesh) = &shape.shape else {
            return None;
        };
        (!mesh.vertices.is_empty() && shape.clip_rect.is_positive()).then_some(mesh.texture_id)
    })
}

fn foreground_updated(output: &egui::FullOutput) -> bool {
    foreground_texture(output).is_some_and(|foreground| {
        output
            .textures_delta
            .set
            .iter()
            .any(|(texture, _)| *texture == foreground)
    })
}

fn foreground_signature(output: &egui::FullOutput) -> Option<u64> {
    let foreground = foreground_texture(output)?;
    let (_, delta) = output
        .textures_delta
        .set
        .iter()
        .find(|(id, _)| *id == foreground)?;
    let egui::ImageData::Color(image) = &delta.image;
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    image.pixels.hash(&mut hash);
    Some(hash.finish())
}

fn start_and_wait_for_foreground(edit: &mut VideoEdit, ctx: &egui::Context) {
    edit.playing = true;
    edit.last_tick = Some(Instant::now());
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let output = render_preview(edit, ctx);
        if foreground_updated(&output) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "no llegó el primer fotograma nítido: {:?}",
            edit.frames_error
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn real_video_play_updates_foreground_at_least_twenty_times_per_second() {
    let Some((_video, mut edit, ctx)) = real_video_editor() else {
        return;
    };
    start_and_wait_for_foreground(&mut edit, &ctx);
    let start = Instant::now();
    let start_playhead = edit.playhead;
    let mut updates = 0;
    let mut texture = None;
    let mut signatures = std::collections::HashSet::new();
    while start.elapsed() < Duration::from_secs(1) {
        std::thread::sleep(Duration::from_millis(5));
        let output = render_preview(&mut edit, &ctx);
        if let Some(id) = foreground_texture(&output) {
            assert_eq!(*texture.get_or_insert(id), id, "Play reasignó la textura");
        }
        updates += usize::from(foreground_updated(&output));
        if let Some(signature) = foreground_signature(&output) {
            signatures.insert(signature);
        }
    }
    let elapsed = start.elapsed().as_secs_f64();
    let rate = updates as f64 / elapsed;
    eprintln!("Preview continua: {rate:.1} fps ({updates} uploads en {elapsed:.3}s)");

    assert!(
        rate >= 20.0,
        "Play mostró {updates} actualizaciones nítidas en {elapsed:.3}s ({rate:.1} fps); las miniaturas de 2 fps no son reproducción fluida"
    );
    assert!(
        signatures.len() as f64 / elapsed >= 20.0,
        "Play repitió fotogramas congelados"
    );
    assert!(
        (edit.playhead - start_playhead - elapsed).abs() < 0.15,
        "El reloj se desvió de la reproducción"
    );
}

#[test]
fn real_video_pause_stops_foreground_updates_and_keeps_playhead() {
    let Some((_video, mut edit, ctx)) = real_video_editor() else {
        return;
    };
    start_and_wait_for_foreground(&mut edit, &ctx);
    let playback = Instant::now();
    while playback.elapsed() < Duration::from_millis(250) {
        std::thread::sleep(Duration::from_millis(5));
        render_preview(&mut edit, &ctx);
    }
    edit.playing = false;
    let paused_at = edit.playhead;

    // Permite completar el último fotograma pedido antes de Pause.
    // La cola debe quedar quieta pronto, sin consumir el resto del vídeo.
    let settling = Instant::now();
    while settling.elapsed() < Duration::from_millis(200) {
        std::thread::sleep(Duration::from_millis(5));
        render_preview(&mut edit, &ctx);
    }
    let paused = Instant::now();
    while paused.elapsed() < Duration::from_millis(300) {
        std::thread::sleep(Duration::from_millis(5));
        let output = render_preview(&mut edit, &ctx);
        assert!(
            !foreground_updated(&output),
            "Pause siguió subiendo nuevos fotogramas nítidos"
        );
        assert_eq!(edit.playhead, paused_at, "Pause avanzó el reloj");
        assert!(foreground_texture(&output).is_some());
    }
}

#[test]
fn accepting_after_real_video_play_preserves_selected_trim_and_poster() {
    let Some((_video, mut edit, ctx)) = real_video_editor() else {
        return;
    };
    start_and_wait_for_foreground(&mut edit, &ctx);
    let playback = Instant::now();
    while playback.elapsed() < Duration::from_millis(150) {
        std::thread::sleep(Duration::from_millis(5));
        render_preview(&mut edit, &ctx);
    }

    let accepted = build_accept(&edit);

    assert_eq!(accepted.path, edit.path);
    assert_eq!((accepted.trim_start, accepted.trim_end), (0.25, Some(2.75)));
    assert_eq!(accepted.poster, edit.frames[0]);
}

#[test]
fn paused_live_frame_updates_blur_without_replacing_foreground() {
    let Some((_video, mut edit, ctx)) = real_video_editor() else {
        return;
    };
    edit.blur = 100.0;
    start_and_wait_for_foreground(&mut edit, &ctx);
    edit.playing = false;
    let paused_at = edit.playhead;
    let sharp = foreground_texture(&render_preview(&mut edit, &ctx)).unwrap();
    edit.blur = 0.0;
    let output = render_preview(&mut edit, &ctx);
    let meshes = output.shapes.iter().filter(|shape| {
        matches!(&shape.shape, egui::epaint::Shape::Mesh(mesh) if !mesh.vertices.is_empty())
    }).count();
    assert_eq!(meshes, 1, "Blur cero debe ocultar el fondo en Pause");
    edit.blur = 25.0;
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let output = render_preview(&mut edit, &ctx);
        assert_eq!(foreground_texture(&output), Some(sharp));
        assert!(
            !foreground_updated(&output),
            "Blur reemplazó el fotograma pausado"
        );
        assert_eq!(edit.playhead, paused_at);
        if output.textures_delta.set.iter().any(|(id, _)| *id != sharp) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "No se actualizó el blur en Pause"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn live_play_stops_at_trim_end_and_retains_last_frame() {
    let Some((_video, mut edit, ctx)) = real_video_editor() else {
        return;
    };
    edit.trim_end = 0.45;
    start_and_wait_for_foreground(&mut edit, &ctx);
    let deadline = Instant::now() + Duration::from_secs(3);
    while edit.playing {
        render_preview(&mut edit, &ctx);
        assert!(Instant::now() < deadline, "Play sobrepasó el trim");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(edit.playhead, edit.trim_end);
    let output = render_preview(&mut edit, &ctx);
    assert!(foreground_texture(&output).is_some());
    assert!(!foreground_updated(&output));
}
