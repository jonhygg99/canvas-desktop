//! UI de la pestaña «Download»: URL(s), trim por tiempos, sin audio y
//! destino `clip-[carpeta](x).mp4`.

use std::path::PathBuf;
use std::sync::mpsc::Sender;

use eframe::egui;

use crate::loader;

use super::api;
use super::state::Panel;

/// Contenido de la pestaña «Download» del panel lateral izquierdo.
/// `dest` es la carpeta donde se guardará el clip (baraja/archivo/galería).
pub fn panel_ui(
    panel: &mut Panel,
    dest: Option<PathBuf>,
    ui: &mut egui::Ui,
    tx: &Sender<loader::AppMsg>,
) {
    let Some(ytdlp) = api::ytdlp_path() else {
        ui.add_space(8.0);
        ui.label("yt-dlp.exe not found");
        ui.add_space(4.0);
        ui.weak(
            "Expected at C:\\Users\\jonhy\\Documents\\code-projects\\yt-dlp\\\n\
             or set YTDLP_PATH and restart the app.",
        );
        return;
    };
    let _ = ytdlp;

    ui.add_space(6.0);
    ui.label("Video URL (one per line, playlist allowed):");
    ui.add(
        egui::TextEdit::multiline(&mut panel.urls)
            .hint_text("https://youtube.com/watch?v=…")
            .desired_rows(3)
            .desired_width(f32::INFINITY),
    );

    ui.add_space(6.0);
    ui.label("Trim (optional, HH:MM:SS):");
    ui.horizontal(|ui| {
        ui.label("From");
        ui.add(
            egui::TextEdit::singleline(&mut panel.start)
                .hint_text("0:00")
                .desired_width(70.0),
        );
        ui.label("To");
        ui.add(
            egui::TextEdit::singleline(&mut panel.end)
                .hint_text("end")
                .desired_width(70.0),
        );
    });
    ui.add_space(2.0);
    ui.checkbox(&mut panel.mute, "Mute (no audio)");

    ui.add_space(4.0);
    match &dest {
        Some(d) => {
            ui.weak(format!("Save to: {}", d.display()));
        }
        None => {
            ui.weak("Open a folder or canvas first to choose where to save.");
        }
    }

    ui.add_space(6.0);
    let urls = api::split_urls(&panel.urls);
    let has_urls = !urls.is_empty();
    let can_download = !panel.downloading && has_urls && dest.is_some();
    if ui
        .add_enabled(can_download, egui::Button::new("Download"))
        .clicked()
    {
        panel.downloading = true;
        panel.progress = "Starting…".to_owned();
        panel.error = None;
        panel.done.clear();
        let request = loader::YtdlpDownloadRequest {
            urls,
            start: api::parse_time(&panel.start),
            end: api::parse_time(&panel.end),
            mute: panel.mute,
            dest: dest.clone().expect("destino comprobado"),
        };
        loader::spawn_ytdlp_download(request, tx.clone(), ui.ctx().clone());
    }
    if !has_urls && !panel.urls.trim().is_empty() {
        ui.weak("Paste an http(s):// URL first.");
    }

    if panel.downloading {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.spinner();
            ui.weak(&panel.progress);
        });
    }
    if let Some(err) = &panel.error {
        ui.add_space(4.0);
        ui.colored_label(ui.visuals().error_fg_color, err);
    }
    if !panel.done.is_empty() {
        ui.add_space(4.0);
        ui.label("Downloaded:");
        for p in &panel.done {
            ui.weak(
                p.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            );
        }
    }
    ui.add_space(4.0);
    ui.weak("Files are saved as clip-[folder](x).mp4.\nTrim uses times; .part resumes on retry.");
}
