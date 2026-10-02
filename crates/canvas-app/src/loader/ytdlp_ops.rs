//! Descargas yt-dlp en hilo aparte: trim por tiempos, mute y naming
//! `clip-[carpeta](x).mp4`. Conserva el `.part` al fallar (reanuda).

use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::time::{SystemTime, UNIX_EPOCH};

use eframe::egui;

use crate::ytdlp::api;

use super::AppMsg;

/// Parámetros de una tanda de descargas (agrupados: sin `too_many_arguments`).
pub struct YtdlpDownloadRequest {
    pub urls: Vec<String>,
    pub start: Option<f64>,
    pub end: Option<f64>,
    pub mute: bool,
    pub dest: PathBuf,
}

pub fn spawn_ytdlp_download(request: YtdlpDownloadRequest, tx: Sender<AppMsg>, ctx: egui::Context) {
    std::thread::spawn(move || {
        let total = request.urls.len();
        let mut done: Vec<PathBuf> = Vec::new();
        let mut errors: Vec<String> = Vec::new();
        let stem = api::clip_stem(&request.dest);
        let section = api::section_arg(request.start, request.end);

        let Some(ytdlp) = api::ytdlp_path() else {
            let _ = tx.send(AppMsg::YtdlpDownloadDone {
                folder: request.dest,
                paths: Vec::new(),
                errors: vec!["yt-dlp.exe not found".to_owned()],
            });
            ctx.request_repaint();
            return;
        };

        for (i, url) in request.urls.iter().enumerate() {
            let _ = tx.send(AppMsg::YtdlpDownloadProgress {
                index: i,
                total,
                text: format!("[{}/{total}] Downloading…", i + 1),
            });
            ctx.request_repaint();
            match download_one(
                &ytdlp,
                url,
                section.as_deref(),
                request.mute,
                &request.dest,
                &stem,
            ) {
                Ok(path) => done.push(path),
                Err(e) => errors.push(format!("{url}: {e}")),
            }
        }

        let _ = tx.send(AppMsg::YtdlpDownloadDone {
            folder: request.dest,
            paths: done,
            errors,
        });
        ctx.request_repaint();
    });
}

/// Manda rutas a la papelera del SO (uno o todos los descargados) en un
/// hilo aparte; el resultado vuelve como `YtdlpFilesDeleted`.
pub fn spawn_ytdlp_delete(paths: Vec<PathBuf>, tx: Sender<AppMsg>, ctx: egui::Context) {
    std::thread::spawn(move || {
        let (removed, errors) = delete_to_trash(&paths);
        let _ = tx.send(AppMsg::YtdlpFilesDeleted { removed, errors });
        ctx.request_repaint();
    });
}

/// Borra a la papelera, tolerando lo que ya no exista. Pura salvo el disco.
fn delete_to_trash(paths: &[PathBuf]) -> (Vec<PathBuf>, Vec<String>) {
    let mut removed = Vec::new();
    let mut errors = Vec::new();
    for path in paths {
        // Se devuelve la ruta ORIGINAL (la lista filtra por ella); solo el
        // borrado usa la normalizada.
        match trash::delete(trashable_path(path)) {
            Ok(()) => removed.push(path.clone()),
            Err(e) => errors.push(format!("{}: {e}", trashable_path(path).display())),
        }
    }
    (removed, errors)
}

/// Sin el prefijo verbatim `\\?\` (lo añade `canonicalize` en Windows y la
/// papelera del SO lo rechaza): la ruta sigue siendo la misma.
fn trashable_path(path: &Path) -> PathBuf {
    const VERBATIM: &str = r"\\?\";
    const UNC: &str = r"\\?\UNC\";
    let s = path.to_string_lossy();
    if let Some(rest) = s.strip_prefix(UNC) {
        PathBuf::from(format!(r"\\{rest}"))
    } else if let Some(rest) = s.strip_prefix(VERBATIM) {
        PathBuf::from(rest)
    } else {
        path.to_path_buf()
    }
}

/// Fotogramas de vista previa extraídos (ver `AppMsg::YtdlpFramesReady`).
/// `files` llevan ruta completa dentro del temporal.
#[derive(Debug)]
pub struct YtdlpFramesOutcome {
    pub clip_id: String,
    pub files: Vec<PathBuf>,
    pub fps: f64,
    pub duration: f64,
    /// Dims reales del vídeo (para el contain).
    pub video_size: Option<(f64, f64)>,
}

/// Extrae miniaturas repartidas por todo el clip para la vista previa del
/// editor. Sin `ffmpeg`/`ffprobe` falla con guía.
pub fn spawn_ytdlp_frames(
    clip_id: String,
    path: PathBuf,
    duration: Option<f64>,
    tx: Sender<AppMsg>,
    ctx: egui::Context,
) {
    std::thread::spawn(move || {
        run_frames(&clip_id, &path, duration, &tx, &ctx);
    });
}

fn run_frames(
    clip_id: &str,
    path: &Path,
    duration: Option<f64>,
    tx: &Sender<AppMsg>,
    ctx: &egui::Context,
) {
    let fail = |error: String| {
        let _ = tx.send(AppMsg::YtdlpFramesFailed {
            clip_id: clip_id.to_owned(),
            error,
        });
        ctx.request_repaint();
    };
    if !path.is_file() {
        fail("The video file is gone. Download it again.".to_owned());
        return;
    }
    let probe = canvas_io::probe_video_size(path).ok();
    let video_size = probe.map(|(w, h, _)| (f64::from(w), f64::from(h)));
    let duration = duration
        .filter(|d| *d > 0.0)
        .or_else(|| probe.and_then(|(_, _, d)| d))
        .filter(|d| *d > 0.0);
    let Some(duration) = duration else {
        fail("Could not read the video duration.".to_owned());
        return;
    };
    let Some(ffmpeg) = canvas_io::ffmpeg_path() else {
        fail("Install ffmpeg for preview.".to_owned());
        return;
    };
    let fps = api::preview_fps(duration);
    let dir = std::env::temp_dir().join(format!("canvas-frames-{}", now_millis()));
    if std::fs::create_dir_all(&dir).is_err() {
        fail("Preview cache is not writable.".to_owned());
        return;
    }
    let ok = canvas_io::media_command(&ffmpeg)
        .args([
            "-y",
            "-i",
            &path.to_string_lossy(),
            "-vf",
            &format!("fps={fps},scale=320:-2"),
            "-frames:v",
            "120",
            &dir.join("frame-%03d.png").to_string_lossy(),
        ])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !ok {
        fail("ffmpeg could not read the video.".to_owned());
        return;
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("png")))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    if files.is_empty() {
        fail("ffmpeg left no preview frames.".to_owned());
        return;
    }
    let _ = tx.send(AppMsg::YtdlpFramesReady(YtdlpFramesOutcome {
        clip_id: clip_id.to_owned(),
        files,
        fps,
        duration,
        video_size,
    }));
    ctx.request_repaint();
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn download_one(
    ytdlp: &PathBuf,
    url: &str,
    section: Option<&str>,
    mute: bool,
    dest: &PathBuf,
    stem: &str,
) -> Result<PathBuf, String> {
    // Reserva el nombre final ANTES de descargar (create_new anti-TOCTOU) y
    // lo libera para que yt-dlp escriba justo ahí: el template apunta al
    // nombre reservado, así el resultado es determinista
    // (`clip-Nombre.mp4`, `clip-Nombre (2).mp4`…).
    let final_path = api::reserve_clip_path(dest, stem).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&final_path);
    let file_stem = final_path
        .file_stem()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| stem.to_owned());
    let template = dest.join(format!("{file_stem}.%(ext)s"));

    let mut cmd = canvas_io::media_command(ytdlp);
    // Reanuda el `.part` al reintentar (lo promete la ayuda del panel).
    cmd.arg("--continue")
        .arg("--merge-output-format")
        .arg("mp4");
    if let Some(loc) = api::ffmpeg_location() {
        cmd.arg("--ffmpeg-location").arg(&loc);
    }
    if !api::is_playlist_url(url) {
        cmd.arg("--no-playlist");
    }
    if mute {
        // Solo mejor video (sin pista de audio).
        cmd.arg("-f").arg("bv");
    }
    if let Some(sec) = section {
        cmd.arg("--download-sections").arg(sec);
        cmd.arg("--force-keyframes-at-cuts");
    }
    cmd.arg("-o").arg(&template).arg(url);

    let out = cmd.output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let stdout = String::from_utf8_lossy(&out.stdout);
        let msg = format!("{stderr}{stdout}");
        return Err(msg.trim().chars().take(300).collect());
    }
    // Con `--merge-output-format mp4` el resultado es el template con .mp4.
    if final_path.is_file() {
        return Ok(final_path);
    }
    // Fallback: el merge pudo dejar otra extensión; buscar el más reciente
    // del mismo stem.
    find_newest_clip(dest, &file_stem)
        .ok_or_else(|| "download finished but no file found".to_owned())
}

fn find_newest_clip(dest: &PathBuf, stem: &str) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dest).ok()?;
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    for e in entries.filter_map(Result::ok) {
        let p = e.path();
        if !p.is_file() {
            continue;
        }
        let name = p.file_name()?.to_string_lossy().into_owned();
        if !name.starts_with(stem) {
            continue;
        }
        if p.extension().is_none_or(|e| {
            !matches!(
                e.to_ascii_lowercase().to_str().unwrap_or(""),
                "mp4" | "mkv" | "webm" | "mov"
            )
        }) {
            continue;
        }
        let mtime = std::fs::metadata(&p).and_then(|m| m.modified()).ok()?;
        if best.as_ref().is_none_or(|(t, _)| mtime > *t) {
            best = Some((mtime, p));
        }
    }
    best.map(|(_, p)| p)
}

#[cfg(test)]
#[path = "ytdlp_ops_tests.rs"]
mod tests;
