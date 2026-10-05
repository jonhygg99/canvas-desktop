//! Estados de descarga y reintento con las opciones originales de cada URL.
use crate::loader::{self, AppMsg, YtdlpDownloadRequest};
use crate::ytdlp::Panel;
use eframe::egui;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::Sender,
    Arc,
};

pub(super) fn start(
    panel: &mut Panel,
    request: YtdlpDownloadRequest,
    tx: &Sender<AppMsg>,
    ctx: &egui::Context,
) {
    panel.downloading = true;
    panel.error = None;
    panel.progress = "Starting…".into();
    panel.cancel = Some(request.cancel.clone());
    for url in &request.urls {
        let mut retry = request.clone();
        retry.urls = vec![url.clone()];
        panel.retry_requests.insert(url.clone(), retry);
        panel.failed.retain(|(failed, _)| failed != url);
    }
    loader::spawn_ytdlp_download(request, tx.clone(), ctx.clone());
}

pub(super) fn show(panel: &mut Panel, ui: &mut egui::Ui, tx: &Sender<AppMsg>) {
    if panel.downloading {
        ui.add_space(6.0);
        if let Some(fraction) = fraction(&panel.progress) {
            ui.add(
                egui::ProgressBar::new(fraction)
                    .desired_width(ui.available_width())
                    .show_percentage(),
            );
        }
        ui.horizontal_wrapped(|ui| {
            ui.spinner();
            let cancelling = panel
                .cancel
                .as_ref()
                .is_some_and(|flag| flag.load(Ordering::Relaxed));
            if ui
                .add_enabled(
                    !cancelling,
                    egui::Button::new(if cancelling {
                        "Cancelling…"
                    } else {
                        "Cancel download"
                    }),
                )
                .clicked()
            {
                if let Some(flag) = &panel.cancel {
                    flag.store(true, Ordering::Relaxed);
                }
            }
        });
        ui.weak(&panel.progress);
    } else if !panel.progress.is_empty() {
        ui.weak(&panel.progress);
    }
    let mut retry_url = None;
    for (url, error) in &panel.failed {
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.label(url);
            ui.colored_label(ui.visuals().error_fg_color, error);
            if ui
                .add_enabled(
                    !panel.downloading && panel.retry_requests.contains_key(url),
                    egui::Button::new("Retry this video"),
                )
                .clicked()
            {
                retry_url = Some(url.clone());
            }
        });
    }
    if let Some(url) = retry_url {
        if let Some(mut request) = panel.retry_requests.get(&url).cloned() {
            request.cancel = Arc::new(AtomicBool::new(false));
            start(panel, request, tx, ui.ctx());
        }
    }
}

fn fraction(text: &str) -> Option<f32> {
    let before = text.split_once('%')?.0;
    let value = before.split_whitespace().last()?.parse::<f32>().ok()?;
    value.is_finite().then_some((value / 100.0).clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::fraction;
    #[test]
    fn progress_bar_only_uses_known_percentages() {
        assert_eq!(fraction("[1/2] Downloading 42.5% · 2 MB/s"), Some(0.425));
        assert_eq!(fraction("Finalizing video…"), None);
        assert_eq!(fraction("Downloading 2.0 MB"), None);
    }
}
