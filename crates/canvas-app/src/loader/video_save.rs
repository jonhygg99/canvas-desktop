//! Guardado de vídeo con identidad del dueño, reserva al primer save y
//! fuentes editables duraderas. Ningún paso de disco bloquea el hilo de UI.

#[cfg(test)]
#[path = "video_save_tests.rs"]
mod tests;

use super::{drop_empty_reservation, AppMsg, ImageInsertDest};
use canvas_io::{CanvasPayload, IoError, VideoComposition, VideoRenderInfo, VideoSourceLinks};
use eframe::egui;
use std::path::PathBuf;
use std::sync::mpsc::Sender;

pub struct VideoSaveRequest {
    pub target: ImageInsertDest,
    pub capture: (usize, u64),
    pub composition: VideoComposition,
    pub payload: CanvasPayload,
    /// Solo Ctrl+S sobre una provisional: Save as ya confirmó su destino.
    pub reserve: bool,
}

pub struct VideoSaveDone {
    pub target: ImageInsertDest,
    /// Captura del historial al lanzar el render: el ÚNICO juez de «hubo
    /// ediciones mientras se escribía» (ver `on_video_composition_rendered`).
    /// Antes viajaba también el documento entero para compararlo, pero el
    /// preview de vídeo escribe `poster_time` en el documento en cada
    /// fotograma: la comparación nunca coincidía y cada render se daba por
    /// «editado durante el guardado».
    pub capture: (usize, u64),
    pub dst: PathBuf,
    pub result: Result<VideoRenderInfo, IoError>,
    pub links: VideoSourceLinks,
}

pub fn spawn_video_save(mut request: VideoSaveRequest, tx: Sender<AppMsg>, ctx: egui::Context) {
    std::thread::spawn(move || {
        let mut reserved = None;
        let mut links = Vec::new();
        let result = (|| {
            if request.reserve {
                let dst = canvas_io::reserve_dest_path(&request.composition.dst)?;
                reserved = Some(dst.clone());
                request.composition.dst = dst;
            }
            let dst = &request.composition.dst;
            request.payload.validate_for_save(dst)?;
            links = canvas_io::preserve_video_sources(&mut request.payload, dst)?;
            if let Some((_, source)) = links
                .iter()
                .find(|(src, _)| *src == request.composition.src)
            {
                request.composition.src = source.clone();
            }
            let info = canvas_io::export_video_composition(&request.composition)?;
            request.payload.document.source_path = Some(dst.clone());
            // El sidecar conserva trim y geometría originales; el render
            // plano no vuelve a usarse como fuente (evita recortes dobles).
            canvas_io::write_video_sidecar(dst, &request.payload)?;
            Ok(info)
        })();
        if result.is_err() {
            if let Some(path) = reserved {
                drop_empty_reservation(&path);
            }
        }
        let done = VideoSaveDone {
            target: request.target,
            capture: request.capture,
            dst: request.composition.dst,
            result,
            links,
        };
        let _ = tx.send(AppMsg::VideoCompositionRendered(Box::new(done)));
        ctx.request_repaint();
    });
}
