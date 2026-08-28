//! Exportar el documento abierto: raster (PNG/JPEG) o vectorial (SVG/PDF), y
//! su diálogo «Guardar como…».

use std::path::PathBuf;
use std::sync::mpsc::Sender;

use canvas_io::{ExportFormat, LayerPixels};
use eframe::egui;

use super::AppMsg;

/// Diálogo «Guardar como…» de Export: un único filtro para el formato ya
/// elegido en el modal (a diferencia de `spawn_pick_save_path`, que ofrece
/// los cinco formatos rasterizables de Guardar).
pub fn spawn_pick_export_path(
    suggested_name: String,
    format: ExportFormat,
    tx: Sender<AppMsg>,
    ctx: egui::Context,
) {
    std::thread::spawn(move || {
        let picked = rfd::FileDialog::new()
            .set_title(format!("Export as {}…", format.label()))
            .add_filter(format.label(), &[format.extension()])
            .set_file_name(suggested_name)
            .save_file();
        let _ = tx.send(AppMsg::ExportPathPicked(picked));
        ctx.request_repaint();
    });
}

/// Codifica y escribe un export raster (PNG/JPEG) en un hilo de trabajo; el
/// RGBA ya viene horneado de la GPU a la escala elegida. Sin metadatos: no
/// es el archivo original que se está sobrescribiendo, es un export nuevo.
pub fn spawn_export_raster(
    path: PathBuf,
    rgba: Vec<u8>,
    width: u32,
    height: u32,
    jpeg_quality: u8,
    tx: Sender<AppMsg>,
    ctx: egui::Context,
) {
    std::thread::spawn(move || {
        let result =
            canvas_io::save_rgba(&path, rgba, width, height, jpeg_quality, None).map(|_bytes| ());
        let _ = tx.send(AppMsg::Exported { path, result });
        ctx.request_repaint();
    });
}

/// Genera el SVG (y, si el formato lo pide, el PDF a partir de él) en un
/// hilo de trabajo: codifica cada capa raster a PNG, monta el documento a
/// mano y escribe atómicamente.
pub fn spawn_export_vector(
    path: PathBuf,
    document: canvas_core::Document,
    images: Vec<LayerPixels>,
    format: ExportFormat,
    scale: f64,
    tx: Sender<AppMsg>,
    ctx: egui::Context,
) {
    std::thread::spawn(move || {
        let result = (|| -> Result<(), canvas_io::IoError> {
            let mut export_images = canvas_io::ExportImages::new();
            for (id, rgba, w, h) in &images {
                let png_base64 = canvas_io::encode_layer_png(rgba, *w, *h)?;
                export_images.insert(*id, png_base64);
            }
            let svg = canvas_io::document_to_svg(
                &document,
                &export_images,
                scale,
                &canvas_render::text_lines,
            )?;
            let bytes = if format == ExportFormat::Pdf {
                canvas_io::svg_to_pdf(&svg)?
            } else {
                svg.into_bytes()
            };
            canvas_io::write_atomic(&path, &bytes)
        })();
        let _ = tx.send(AppMsg::Exported { path, result });
        ctx.request_repaint();
    });
}
