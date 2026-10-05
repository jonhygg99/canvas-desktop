//! UI de la pestaña «Download»: URL(s), trim por tiempos, sin audio y
//! destino `clip-[carpeta](x).mp4`.

use std::path::PathBuf;
use std::sync::mpsc::Sender;

use eframe::egui;

use crate::loader;
use crate::settings::AppSettings;

use super::api;
use super::state::Panel;
#[path = "download_jobs.rs"]
mod jobs;
#[path = "download_options.rs"]
mod options;

/// Contenido de la pestaña «Download» del panel lateral izquierdo.
/// `dest` es la carpeta donde se guardará el clip (baraja/archivo/galería,
/// última usada o Vídeos).
pub fn panel_ui(
    panel: &mut Panel,
    settings: &mut AppSettings,
    dest: Option<PathBuf>,
    ui: &mut egui::Ui,
    tx: &Sender<loader::AppMsg>,
) {
    let Some(ytdlp) = api::ytdlp_path() else {
        ui.add_space(8.0);
        ui.label(crate::i18n::tr("yt-dlp.exe not found"));
        ui.add_space(4.0);
        ui.weak(crate::i18n::tr(
            "Expected at C:\\Users\\jonhy\\Documents\\code-projects\\yt-dlp\\\n\
             or set YTDLP_PATH and restart the app.",
        ));
        return;
    };
    let _ = ytdlp;

    download_form(panel, settings, dest, ui, tx);
    jobs::show(panel, ui, tx);
    downloaded_results(panel, ui, tx);
}

fn download_form(
    panel: &mut Panel,
    settings: &mut AppSettings,
    dest: Option<PathBuf>,
    ui: &mut egui::Ui,
    tx: &Sender<loader::AppMsg>,
) {
    ui.add_space(6.0);
    ui.label(crate::i18n::tr(
        "Video URL (one per line, playlist allowed):",
    ));
    ui.add(
        egui::TextEdit::multiline(&mut panel.urls)
            .hint_text(crate::i18n::tr("https://youtube.com/watch?v=…"))
            .desired_rows(3)
            .desired_width(f32::INFINITY),
    );

    ui.add_space(6.0);
    let section = options::show(panel, ui);

    ui.add_space(4.0);
    match &dest {
        Some(d) => {
            ui.weak(format!("Save to: {}", d.display()));
        }
        None => {
            ui.weak(crate::i18n::tr(
                "Open a folder or canvas first to choose where to save.",
            ));
        }
    }

    ui.add_space(6.0);
    let urls = api::split_urls(&panel.urls);
    let has_urls = !urls.is_empty();
    let can_download = !panel.downloading && has_urls && dest.is_some() && section.is_ok();
    if ui
        .add_enabled(can_download, egui::Button::new(crate::i18n::tr("Download")))
        .clicked()
    {
        panel.downloading = true;
        panel.progress = "Starting…".to_owned();
        panel.error = None;
        // La lista NO se vacía: los clips se quedan hasta que se borren.
        // La carpeta usada se recuerda para la próxima (detrás del contexto
        // vivo en la resolución, como `serper_last_folder`).
        settings.ytdlp_last_folder = dest.clone();
        settings.save_in_background();
        let request = loader::YtdlpDownloadRequest {
            urls,
            start: section.expect("validated section").0,
            end: section.expect("validated section").1,
            mute: panel.mute,
            dest: dest.clone().expect("destino comprobado"),
            cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            target: None,
        };
        jobs::start(panel, request, tx, ui.ctx());
    }
    if !has_urls && !panel.urls.trim().is_empty() {
        ui.weak(crate::i18n::tr("Paste an http(s):// URL first."));
    }
}

fn downloaded_results(panel: &mut Panel, ui: &mut egui::Ui, tx: &Sender<loader::AppMsg>) {
    if let Some(err) = &panel.error {
        ui.add_space(4.0);
        ui.colored_label(ui.visuals().error_fg_color, err);
    }
    if !panel.done.is_empty() {
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.label(crate::i18n::tr("Downloaded:"));
            // Limpieza total a la derecha del título.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let clear =
                    ui.add_enabled(!panel.downloading, egui::Button::new("Move all to trash"));
                if clear.clicked() {
                    loader::spawn_ytdlp_delete(panel.done.clone(), tx.clone(), ui.ctx().clone());
                }
                clear.on_hover_text(crate::i18n::tr("Move every downloaded file to the trash"));
            });
        });
        super::clip_cards::show(panel, ui, tx);
    }
    ui.add_space(4.0);
    ui.weak(crate::i18n::tr(
        "Files are saved as clip-[folder](x).mp4.\nTrim uses times; .part resumes on retry.",
    ));
}
