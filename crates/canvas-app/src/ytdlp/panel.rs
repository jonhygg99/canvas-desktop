//! UI de la pestaña «Download»: URL(s), trim por tiempos, sin audio y
//! destino siempre temporal (`clip(x).mp4` en el staging de la app).

use std::path::PathBuf;
use std::sync::mpsc::Sender;

use eframe::egui;

use crate::loader;

use super::api;
use super::state::Panel;
use egui::containers::scroll_area::ScrollAreaOutput;
#[path = "download_jobs.rs"]
mod jobs;
#[path = "download_options.rs"]
mod options;

/// Contenido de la pestaña «Download» del panel lateral izquierdo.
/// `dest` es la carpeta temporal de descargas: el proyecto nunca recibe el
/// mp4 (valor, no `Option` — no hay caso «sin destino» que mostrar).
/// Todo rueda, formulario incluido (como la pestaña Web): la lista crece sin
/// cota y en ventanas bajas hasta el botón Download quedaba fuera de alcance.
/// Devuelve la salida del área para poder afirmarla en tests.
pub fn panel_ui(
    panel: &mut Panel,
    dest: PathBuf,
    ui: &mut egui::Ui,
    tx: &Sender<loader::AppMsg>,
) -> ScrollAreaOutput<()> {
    egui::ScrollArea::vertical()
        .id_salt("download")
        .show(ui, |ui| {
            // Solo el formulario necesita el binario; la lista (progreso y
            // clips de la sesión) se muestra igual sin él.
            if api::ytdlp_path().is_none() {
                ui.add_space(8.0);
                ui.label(crate::i18n::tr("yt-dlp.exe not found"));
                ui.add_space(4.0);
                ui.weak(crate::i18n::tr(
                    "Install yt-dlp and make sure it is on PATH,\n\
                     or set YTDLP_PATH and restart the app.",
                ));
            } else {
                download_form(panel, dest, ui, tx);
            }
            jobs::show(panel, ui, tx);
            downloaded_results(panel, ui, tx);
        })
}

fn download_form(panel: &mut Panel, dest: PathBuf, ui: &mut egui::Ui, tx: &Sender<loader::AppMsg>) {
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
    ui.weak(format!("Save to (temporary): {}", dest.display()));

    ui.add_space(6.0);
    let urls = api::split_urls(&panel.urls);
    let has_urls = !urls.is_empty();
    let can_download = !panel.downloading && has_urls && section.is_ok();
    if ui
        .add_enabled(can_download, egui::Button::new(crate::i18n::tr("Download")))
        .clicked()
    {
        panel.downloading = true;
        panel.progress = "Starting…".to_owned();
        panel.error = None;
        // La lista NO se vacía: los clips se quedan hasta que se borren.
        // El destino es siempre el temporal (el ajuste `ytdlp_last_folder`
        // murió con el «Save to» por contexto), así que no hay nada que
        // recordar en ajustes.
        let request = loader::YtdlpDownloadRequest {
            urls,
            start: section.expect("validated section").0,
            end: section.expect("validated section").1,
            mute: panel.mute,
            dest: dest.clone(),
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
        "Files are saved as clip(x).mp4 in a temporary folder.\nThey stay in Download until moved to trash.\nTrim uses times; .part resumes on retry.",
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// El sidebar de Download rueda entero: con más contenido que viewport,
    /// el área lo recoge con scroll. Va por `panel_ui` (la entrada real) para
    /// que también quede cableado el formulario, no solo la lista. Vale con
    /// y sin binario: en ambos casos las tarjetas desbordan igual.
    #[test]
    fn download_sidebar_scrolls_when_content_overflows() {
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut panel = Panel::default();
        panel.done = (0..6)
            .map(|i| PathBuf::from(format!("clip{i}.mp4")))
            .collect();
        let ctx = egui::Context::default();
        ctx.set_fonts(egui::FontDefinitions::empty());
        let input = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(300.0, 200.0),
            )),
            ..Default::default()
        };
        let last: Cell<Option<ScrollAreaOutput<()>>> = Cell::new(None);
        // Dos frames: el tamaño del contenido se conoce tras el primero.
        for _ in 0..2 {
            let _ = ctx.run_ui(input(), |ui| {
                last.set(Some(panel_ui(&mut panel, PathBuf::from("tmp"), ui, &tx)));
            });
        }
        let out = last.take().expect("scroll area shown");
        assert!(
            out.content_size.y > out.inner_rect.height(),
            "el contenido ({:.0}px) debe desbordar el viewport ({:.0}px)",
            out.content_size.y,
            out.inner_rect.height()
        );
    }
}
