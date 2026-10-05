//! Descargas por URL con progreso, cancelación y destino estable al reintentar.
use super::{ytdlp_process, AppMsg};
use crate::ytdlp::api;
use eframe::egui;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::Sender,
    Arc,
};

#[derive(Clone)]
pub struct YtdlpDownloadRequest {
    pub urls: Vec<String>,
    pub start: Option<f64>,
    pub end: Option<f64>,
    pub mute: bool,
    pub dest: PathBuf,
    pub cancel: Arc<AtomicBool>,
    pub target: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct DownloadedClip {
    pub path: PathBuf,
    pub title: String,
}

pub struct YtdlpItemOutcome {
    pub url: String,
    pub target: Option<PathBuf>,
    pub clips: Vec<DownloadedClip>,
    pub error: Option<String>,
}

pub fn spawn_ytdlp_download(request: YtdlpDownloadRequest, tx: Sender<AppMsg>, ctx: egui::Context) {
    std::thread::spawn(move || run_batch(request, tx, ctx));
}

fn run_batch(request: YtdlpDownloadRequest, tx: Sender<AppMsg>, ctx: egui::Context) {
    let ytdlp = api::ytdlp_path();
    let mut paths = Vec::new();
    let mut errors = Vec::new();
    for (index, url) in request.urls.iter().enumerate() {
        let progress = |text| {
            let _ = tx.send(AppMsg::YtdlpDownloadProgress {
                index,
                total: request.urls.len(),
                text,
            });
            ctx.request_repaint();
        };
        progress("Reading video information…".into());
        let mut outcome = YtdlpItemOutcome {
            url: url.clone(),
            target: None,
            clips: Vec::new(),
            error: None,
        };
        if request.cancel.load(Ordering::Relaxed) {
            outcome.error = Some("Cancelled. Retry to resume.".into());
        } else if let Some(ytdlp) = &ytdlp {
            let result = download_one(ytdlp, url, &request, &mut outcome, progress);
            outcome.error = result.err();
        } else {
            outcome.error = Some("yt-dlp not found".into());
        }
        paths.extend(outcome.clips.iter().map(|clip| clip.path.clone()));
        if let Some(error) = &outcome.error {
            errors.push(format!("{url}: {error}"));
        }
        let _ = tx.send(AppMsg::YtdlpItemFinished(outcome));
        ctx.request_repaint();
    }
    let _ = tx.send(AppMsg::YtdlpDownloadDone {
        folder: request.dest,
        paths,
        errors,
    });
    ctx.request_repaint();
}

fn download_one(
    ytdlp: &Path,
    url: &str,
    request: &YtdlpDownloadRequest,
    outcome: &mut YtdlpItemOutcome,
    progress: impl Fn(String),
) -> Result<(), String> {
    let target = match &request.target {
        Some(target) => target.clone(),
        None => {
            let path = api::reserve_clip_path(&request.dest, &api::clip_stem(&request.dest))
                .map_err(|error| error.to_string())?;
            std::fs::remove_file(&path).map_err(|error| error.to_string())?;
            path
        }
    };
    outcome.target = Some(target.clone());
    let stem = target
        .file_stem()
        .ok_or("Invalid download filename")?
        .to_string_lossy();
    let name = if api::is_playlist_url(url) {
        format!("{stem}-%(playlist_index)03d.%(ext)s")
    } else {
        format!("{stem}.%(ext)s")
    };
    // Los porcentajes literales del directorio no son campos de yt-dlp.
    let template = format!(
        "{}{}{}",
        request.dest.to_string_lossy().replace('%', "%%"),
        std::path::MAIN_SEPARATOR,
        name
    );
    let mut command = canvas_io::media_command(ytdlp);
    command.args([
        "--continue",
        "--no-simulate",
        "--newline",
        "--progress",
        "--progress-delta",
        "0.2",
        "--merge-output-format",
        "mp4",
        "--encoding",
        "utf-8",
        "--progress-template",
        "download:CANVAS_PROGRESS:%(progress)j",
        "--progress-template",
        "postprocess:CANVAS_POST:%(progress.status)s",
        "--print",
        r#"after_move:CANVAS_CLIP:{"path":%(filepath)j,"title":%(title)j}"#,
    ]);
    if let Some(location) = api::ffmpeg_location() {
        command.arg("--ffmpeg-location").arg(location);
    }
    if !api::is_playlist_url(url) {
        command.arg("--no-playlist");
    }
    if request.mute {
        command.args(["-f", "bv"]);
    }
    if let Some(section) = api::section_arg(request.start, request.end) {
        command.args(["--download-sections", &section, "--force-keyframes-at-cuts"]);
    }
    command.arg("-o").arg(template).arg(url);
    let result = ytdlp_process::run(&mut command, &request.cancel, |line| {
        if let Some(text) = ytdlp_process::progress(line) {
            progress(text);
        }
        if let Some(clip) = parse_clip(line) {
            if clip.path.is_file() && !outcome.clips.iter().any(|item| item.path == clip.path) {
                outcome.clips.push(clip);
            }
        }
    });
    result?;
    if outcome.clips.is_empty() {
        return Err("Download finished but no video file was found.".into());
    }
    progress("Ready".into());
    Ok(())
}

fn parse_clip(line: &str) -> Option<DownloadedClip> {
    let value: serde_json::Value = serde_json::from_str(line.strip_prefix("CANVAS_CLIP:")?).ok()?;
    Some(DownloadedClip {
        path: PathBuf::from(value.get("path")?.as_str()?),
        title: value.get("title")?.as_str()?.to_owned(),
    })
}

#[cfg(test)]
#[path = "ytdlp_download_tests.rs"]
mod tests;
