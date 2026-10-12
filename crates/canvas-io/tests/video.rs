use canvas_io::{
    export_video_composition, ffmpeg_path, media_command, probe_video_size, VideoComposition,
    VideoFrameStream,
};

fn lavfi_source(
    dir: &std::path::Path,
    name: &str,
    size: &str,
    seconds: &str,
) -> std::path::PathBuf {
    let ffmpeg = ffmpeg_path().expect("ffmpeg para el fixture");
    let source = dir.join(name);
    let make = media_command(&ffmpeg)
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            &format!("testsrc=size={size}:rate=10"),
            "-t",
            seconds,
            "-pix_fmt",
            "yuv420p",
        ])
        .arg(&source)
        .output()
        .unwrap();
    assert!(
        make.status.success(),
        "{}",
        String::from_utf8_lossy(&make.stderr)
    );
    source
}

/// Fuente con pista de audio real (tono): la rama de audio del render solo
/// se ejecuta con clips con sonido — todos los de teléfono — y las fixtures
/// mudas no la tocan.
fn lavfi_source_with_audio(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    let ffmpeg = ffmpeg_path().expect("ffmpeg para el fixture");
    let source = dir.join(name);
    let make = media_command(&ffmpeg)
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=90x160:rate=10",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=6",
            "-t",
            "6",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-shortest",
        ])
        .arg(&source)
        .output()
        .unwrap();
    assert!(
        make.status.success(),
        "{}",
        String::from_utf8_lossy(&make.stderr)
    );
    source
}

fn has_audio(path: &std::path::Path) -> bool {
    let ffmpeg = ffmpeg_path().expect("ffmpeg para el sondeo");
    let probe = ffmpeg
        .parent()
        .map(|d| d.join("ffprobe"))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| std::path::PathBuf::from("ffprobe"));
    media_command(probe)
        .args([
            "-v",
            "error",
            "-select_streams",
            "a",
            "-show_entries",
            "stream=index",
        ])
        .arg(path)
        .output()
        .is_ok_and(|out| out.status.success() && !out.stdout.is_empty())
}

#[test]
fn rendered_composition_keeps_the_audio_track() {
    if ffmpeg_path().is_none() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let source = lavfi_source_with_audio(dir.path(), "sounding.mp4");
    assert!(has_audio(&source), "el fixture trae audio");
    let dst = dir.path().join("sounding-render.mp4");
    export_video_composition(&VideoComposition {
        src: source,
        start_s: 1.0,
        end_s: Some(3.0),
        page_w: 320,
        page_h: 180,
        fg_x: 115,
        fg_y: 10,
        fg_w: 90,
        fg_h: 160,
        bg_blur: 8.0,
        dst: dst.clone(),
    })
    .unwrap();
    assert!(has_audio(&dst), "el render conserva la pista de audio");
    let (_, _, duration) = probe_video_size(&dst).unwrap();
    assert!((duration.unwrap() - 2.0).abs() < 0.5);
}

#[test]
fn rendered_composition_matches_the_page_not_the_source() {
    // El caso del informe: fuente vertical 9:16 en página 1920×1080 con
    // fondo — la salida es la página (con fondo), no la fuente recortada.
    if ffmpeg_path().is_none() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let source = lavfi_source(dir.path(), "source.mp4", "90x160", "6");
    let dst = dir.path().join("render.mp4");
    let rendered = export_video_composition(&VideoComposition {
        src: source,
        start_s: 1.0,
        end_s: Some(3.0),
        page_w: 320,
        page_h: 180,
        fg_x: 115,
        fg_y: 10,
        fg_w: 90,
        fg_h: 160,
        bg_blur: 8.0,
        dst: dst.clone(),
    })
    .unwrap();
    assert!((rendered.duration_secs - 2.0).abs() < 0.01);
    assert_eq!(
        (rendered.width, rendered.height),
        (320, 180),
        "el render informa la página: la capa se re-ancla a él"
    );
    // Sin rastro del temporal atómico.
    assert!(!dir.path().join("render.exporting").exists());
    let (w, h, duration) = probe_video_size(&dst).unwrap();
    assert_eq!((w, h), (320, 180), "la salida es la página, no la fuente");
    assert!((duration.unwrap() - 2.0).abs() < 0.5);
    // Guardar en el sitio (el destino ya existe) sustituye sin fallar: es el
    // camino de `Ctrl+S` tras el primer guardado.
    let again = export_video_composition(&VideoComposition {
        src: dst.clone(),
        start_s: 0.0,
        end_s: None,
        page_w: 320,
        page_h: 180,
        fg_x: 0,
        fg_y: 0,
        fg_w: 320,
        fg_h: 180,
        bg_blur: 0.0,
        dst: dst.clone(),
    })
    .unwrap();
    assert!((again.duration_secs - 2.0).abs() < 0.5);
    // Tramo inválido: error limpio sin tocar el disco.
    let bad = dir.path().join("bad.mp4");
    assert!(export_video_composition(&VideoComposition {
        src: dst,
        start_s: 3.0,
        end_s: Some(1.0),
        page_w: 320,
        page_h: 180,
        fg_x: 0,
        fg_y: 0,
        fg_w: 320,
        fg_h: 180,
        bg_blur: 0.0,
        dst: bad.clone(),
    })
    .is_err());
    assert!(!bad.exists());
}

#[test]
fn stream_decodes_changing_frames_and_seeks_inside_the_clip() {
    let Some(ffmpeg) = ffmpeg_path() else { return };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("moving clip.mp4");
    assert!(media_command(ffmpeg)
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=64x48:rate=20",
            "-t",
            "1",
            "-pix_fmt",
            "yuv420p"
        ])
        .arg(&path)
        .output()
        .unwrap()
        .status
        .success());
    let (w, h, duration) = probe_video_size(&path).unwrap();
    assert_eq!((w, h), (64, 48));
    assert!((duration.unwrap() - 1.0).abs() < 0.1);
    let mut stream = VideoFrameStream::open(&path, 0.0, (w, h), 20).unwrap();
    let first = stream.next_frame().unwrap().unwrap();
    let next = stream.next_frame().unwrap().unwrap();
    assert_eq!(first.rgba.len(), 64 * 48 * 4);
    assert!(first.rgba != next.rgba, "the displayed pixels must advance");
    let mut seek = VideoFrameStream::open(&path, 0.5, (w, h), 20).unwrap();
    assert!(
        first.rgba != seek.next_frame().unwrap().unwrap().rgba,
        "seeking must change the frame"
    );
    assert!(VideoFrameStream::open(&path, 0.0, (0, h), 20).is_err());
}

#[test]
fn stream_reports_a_missing_source() {
    if ffmpeg_path().is_none() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let mut stream =
        VideoFrameStream::open(&dir.path().join("missing.mp4"), 0.0, (64, 48), 20).unwrap();
    assert!(stream.next_frame().is_err());
}

#[cfg(windows)]
#[test]
fn media_child_has_no_console() {
    if std::env::var_os("CANVAS_TEST_MEDIA_CHILD").is_some() {
        #[link(name = "kernel32")]
        extern "system" {
            fn GetConsoleWindow() -> *mut std::ffi::c_void;
        }
        assert!(
            unsafe { GetConsoleWindow() }.is_null(),
            "media helper opened a console window"
        );
        return;
    }
    let output = media_command(std::env::current_exe().unwrap())
        .args(["--exact", "media_child_has_no_console"])
        .env("CANVAS_TEST_MEDIA_CHILD", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn rotated_video_probe_matches_the_decoded_poster() {
    let Some(ffmpeg) = ffmpeg_path() else { return };
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.mp4");
    let rotated = dir.path().join("rotated.mp4");
    let make = media_command(&ffmpeg)
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=64x48:rate=5",
            "-t",
            "0.4",
            "-pix_fmt",
            "yuv420p",
        ])
        .arg(&source)
        .output()
        .unwrap();
    assert!(
        make.status.success(),
        "{}",
        String::from_utf8_lossy(&make.stderr)
    );
    let rotate = media_command(&ffmpeg)
        .args(["-v", "error", "-display_rotation", "90", "-i"])
        .arg(&source)
        .args(["-c", "copy"])
        .arg(&rotated)
        .output()
        .unwrap();
    assert!(
        rotate.status.success(),
        "{}",
        String::from_utf8_lossy(&rotate.stderr)
    );
    let poster = canvas_io::load_video_frame(&rotated, 0.0).unwrap();
    assert_eq!((poster.width, poster.height), (48, 64));
    let (width, height, _) = probe_video_size(&rotated).unwrap();
    assert_eq!((width, height), (poster.width, poster.height));
}
