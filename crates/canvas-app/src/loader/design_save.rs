//! Diseño autónomo: destino .canvas validado antes de reservar/copiar nada,
//! fuentes guardadas en `.download` y respuesta ligada a la ranura de origen.

use super::{drop_empty_reservation, AppMsg, ImageInsertDest};
use canvas_io::{CanvasPayload, IoError, VideoSourceLinks};
use eframe::egui;
use std::path::PathBuf;
use std::sync::mpsc::Sender;

pub struct DesignSaveRequest {
    pub target: ImageInsertDest,
    pub capture: (usize, u64),
    pub path: PathBuf,
    pub payload: CanvasPayload,
    pub reserve: bool,
    pub new_source: bool,
}

pub struct DesignSaveDone {
    pub target: ImageInsertDest,
    pub capture: (usize, u64),
    pub document: canvas_core::Document,
    pub path: PathBuf,
    pub result: Result<(), IoError>,
    pub new_source: bool,
    pub links: VideoSourceLinks,
}

pub fn spawn_save_design(mut request: DesignSaveRequest, tx: Sender<AppMsg>, ctx: egui::Context) {
    std::thread::spawn(move || {
        let document = request.payload.document.clone();
        let mut reserved = None;
        let mut links = Vec::new();
        let result = (|| {
            if !canvas_io::is_canvas_file(&request.path) {
                return Err(IoError::Write {
                    path: request.path.clone(),
                    message: "Canvas designs require a .canvas destination".into(),
                });
            }
            request.payload.validate_for_save(&request.path)?;
            if request.reserve {
                // La ruta ya pasó la validación `.canvas`: su extensión es
                // la que reserva.
                request.path = canvas_io::reserve_dest_path(&request.path)?;
                reserved = Some(request.path.clone());
            }
            links = canvas_io::preserve_video_sources(&mut request.payload, &request.path)?;
            request.payload.document.source_path = Some(request.path.clone());
            canvas_io::write_design(&request.path, &request.payload)
        })();
        if result.is_err() {
            if let Some(path) = reserved {
                drop_empty_reservation(&path);
            }
        }
        let _ = tx.send(AppMsg::DesignSaved(Box::new(DesignSaveDone {
            target: request.target,
            capture: request.capture,
            document,
            path: request.path,
            result,
            new_source: request.new_source || request.reserve,
            links,
        })));
        ctx.request_repaint();
    });
}
