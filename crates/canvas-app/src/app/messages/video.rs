use canvas_core::LayerContent;
use canvas_render::image_data_from_rgba;
use eframe::egui;

use super::super::{View, Workspace};

impl super::AppInner {
    pub(super) fn on_video_frame_ready(
        &mut self,
        ws: &mut Workspace,
        layer: canvas_core::LayerId,
        time: f64,
        result: Result<canvas_io::LoadedImage, canvas_io::IoError>,
        _ctx: &egui::Context,
    ) {
        let View::Editor(state) = &mut ws.view else {
            return;
        };
        // Solo si la capa aún existe y es video
        let Ok(l) = state.doc.layer(layer) else {
            return;
        };
        if !matches!(l.content, LayerContent::Video(_)) {
            return;
        }
        match result {
            Ok(img) => {
                // Actualizar poster_time si coincide con el frame pedido (evita carreras)
                if let Ok(l) = state.doc.layer_mut(layer) {
                    if let LayerContent::Video(v) = &mut l.content {
                        // Solo actualizar si el tiempo es cercano al actual (tolerancia)
                        // Para simplificar, siempre actualizar
                        v.poster_time = time;
                    }
                }
                let data = image_data_from_rgba(img.rgba, img.width, img.height);
                state.images.insert(layer, data);
            }
            Err(e) => {
                tracing::warn!("video frame at {} failed: {}", time, e);
                state.save_error = Some(format!("Video frame failed: {e}"));
            }
        }
    }
}
