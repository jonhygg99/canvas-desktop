//! Carga de frames de video vía ffmpeg en hilo aparte.

use std::path::PathBuf;
use std::sync::mpsc::Sender;

use canvas_core::LayerId;
use eframe::egui;

/// Pide un frame de `path` en `time_secs` y lo entrega por `AppMsg::VideoFrameReady`.
pub fn spawn_video_frame(
    path: PathBuf,
    layer: LayerId,
    time_secs: f64,
    tx: Sender<super::AppMsg>,
    ctx: egui::Context,
) {
    std::thread::spawn(move || {
        let result = canvas_io::load_video_frame(&path, time_secs);
        let _ = tx.send(super::AppMsg::VideoFrameReady {
            layer,
            time: time_secs,
            result,
        });
        ctx.request_repaint();
    });
}
