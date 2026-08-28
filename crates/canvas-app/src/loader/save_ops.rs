//! Guardar el archivo abierto en el editor: raster (con su sidecar opcional)
//! o diseño autónomo, y los diálogos «Guardar como…» que les corresponden.

use std::path::PathBuf;
use std::sync::mpsc::Sender;

use canvas_io::{CanvasPayload, ImageMetadata};
use eframe::egui;

use super::AppMsg;

/// Reserva atómicamente (`create_new`, vía `canvas_io::reserve_numbered_path`)
/// un nombre libre en `folder` para una ranura provisional que el usuario
/// acaba de empezar a editar. En un hilo por disciplina: en una unidad de
/// red con cientos de lienzos numerados ya creados, el bucle de reserva sí
/// se nota, y la UI no espera nunca al disco.
pub fn spawn_reserve_canvas_path(
    folder: PathBuf,
    slot: u64,
    ext: String,
    tx: Sender<AppMsg>,
    ctx: egui::Context,
) {
    std::thread::spawn(move || {
        let result = canvas_io::reserve_numbered_path(&folder, &ext);
        let _ = tx.send(AppMsg::CanvasPathReserved {
            folder,
            slot,
            result,
        });
        ctx.request_repaint();
    });
}

/// Reescribe (atómico) un diseño autónomo en `path`, sin rasterizar nada.
pub fn spawn_save_design(
    path: PathBuf,
    payload: CanvasPayload,
    new_source: bool,
    tx: Sender<AppMsg>,
    ctx: egui::Context,
) {
    std::thread::spawn(move || {
        let result = canvas_io::write_design(&path, &payload);
        let _ = tx.send(AppMsg::Saved {
            path,
            result,
            new_source,
        });
        ctx.request_repaint();
    });
}

/// Diálogo «Guardar como…» para un diseño: un único filtro `.canvas`, a
/// diferencia de `spawn_pick_save_path` que ofrece los cinco formatos
/// rasterizables de Guardar.
pub fn spawn_pick_design_path(suggested: Option<String>, tx: Sender<AppMsg>, ctx: egui::Context) {
    let suffix = format!(".{}", canvas_io::CANVAS_EXTENSION);
    let suggested = suggested.map(|name| {
        if name.to_ascii_lowercase().ends_with(&suffix) {
            name
        } else {
            format!("{name}{suffix}")
        }
    });
    std::thread::spawn(move || {
        let mut dialog = rfd::FileDialog::new()
            .set_title("Save design as…")
            .add_filter("Canvas design", &[canvas_io::CANVAS_EXTENSION]);
        if let Some(name) = suggested {
            dialog = dialog.set_file_name(name);
        }
        let picked = dialog.save_file();
        let _ = tx.send(AppMsg::SaveAsPicked(picked));
        ctx.request_repaint();
    });
}

/// Datos que `spawn_save` necesita para codificar y escribir una imagen
/// en un hilo de trabajo. Agrupa los 6 parámetros de píxeles/formato que
/// siempre van juntos, reduciendo la firma de 10 a 5.
pub struct SaveInput {
    pub path: PathBuf,
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub jpeg_quality: u8,
    pub metadata: Option<ImageMetadata>,
    pub new_source: bool,
    pub sidecar: Option<CanvasPayload>,
}

/// Codifica y escribe (atómico) en un hilo de trabajo; el RGBA ya viene
/// horneado de la GPU.
pub fn spawn_save(input: SaveInput, tx: Sender<AppMsg>, ctx: egui::Context) {
    std::thread::spawn(move || {
        let SaveInput {
            path,
            rgba,
            width,
            height,
            jpeg_quality,
            metadata,
            new_source,
            sidecar,
        } = input;
        // La miniatura embebida del sidecar se reduce de este mismo RGBA
        // (ya horneado a tamaño completo): no hace falta una segunda pasada
        // de GPU solo para la celda de la galería.
        let preview = sidecar
            .is_some()
            .then(|| canvas_io::make_preview(&rgba, width, height))
            .flatten();
        // `save_rgba` devuelve los bytes EXACTOS que quedaron en disco (ya
        // con metadatos reinsertados): el hash del sidecar se calcula sobre
        // eso directamente, sin releer el archivo — en un PNG de 30 MP eso
        // ahorra decenas de MB de I/O en cada `Ctrl+S`.
        let save_result =
            canvas_io::save_rgba(&path, rgba, width, height, jpeg_quality, metadata.as_ref());
        let result = match save_result {
            Ok(bytes) => {
                match sidecar {
                    Some(mut payload) => {
                        payload.preview = preview;
                        if let Err(e) = canvas_io::write_sidecar(&path, &bytes, &payload) {
                            tracing::warn!("no se pudo escribir el sidecar: {e}");
                        }
                    }
                    // Sidecar desactivado: retira el que hubiera para no
                    // dejar uno obsoleto que luego avise de hash cambiado.
                    None => canvas_io::delete_sidecar(&path),
                }
                Ok(())
            }
            Err(e) => Err(e),
        };
        let _ = tx.send(AppMsg::Saved {
            path,
            result,
            new_source,
        });
        ctx.request_repaint();
    });
}

pub fn spawn_pick_save_path(suggested: Option<String>, tx: Sender<AppMsg>, ctx: egui::Context) {
    // El lienzo no sabe guardar SVG: sugiere el mismo nombre en .png.
    let suggested = suggested.map(|name| {
        if name.to_ascii_lowercase().ends_with(".svg") {
            format!("{}.png", &name[..name.len() - 4])
        } else {
            name
        }
    });
    std::thread::spawn(move || {
        let mut dialog = rfd::FileDialog::new()
            .set_title("Save as…")
            .add_filter("PNG", &["png"])
            .add_filter("JPEG", &["jpg", "jpeg"])
            .add_filter("WebP", &["webp"])
            .add_filter("GIF", &["gif"])
            .add_filter("BMP", &["bmp"]);
        if let Some(name) = suggested {
            dialog = dialog.set_file_name(name);
        }
        let picked = dialog.save_file();
        let _ = tx.send(AppMsg::SaveAsPicked(picked));
        ctx.request_repaint();
    });
}
