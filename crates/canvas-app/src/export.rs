//! Diálogo de exportación (File > Export…): formato, escala 1x/2x/3x y
//! calidad JPEG. La orquestación (hornear/generar SVG, escribir a disco)
//! vive en `main.rs` (`start_export`) y `loader.rs` (los hilos de trabajo),
//! siguiendo el mismo reparto de trabajo que el guardado normal.

use canvas_io::ExportFormat;
use eframe::egui;

/// Estado del diálogo mientras está abierto.
pub struct ExportDialog {
    pub format: ExportFormat,
    /// 1, 2 o 3.
    pub scale: u32,
    pub jpeg_quality: u8,
}

impl Default for ExportDialog {
    fn default() -> Self {
        Self {
            format: ExportFormat::Png,
            scale: 1,
            jpeg_quality: 92,
        }
    }
}

/// Ajustes ya congelados al pulsar «Export…»: no cambian mientras se
/// resuelve la ruta de archivo y se exporta de verdad.
#[derive(Debug, Clone, Copy)]
pub struct ExportSettings {
    pub format: ExportFormat,
    pub scale: u32,
    pub jpeg_quality: u8,
}

pub enum ExportChoice {
    None,
    Cancel,
    Pick(ExportSettings),
}

/// Dibuja el modal. `page_size` es `(ancho, alto)` de la página, para la
/// etiqueta de salida en vivo.
pub fn export_modal(
    dialog: &mut ExportDialog,
    ctx: &egui::Context,
    page_size: (f64, f64),
) -> ExportChoice {
    let mut choice = ExportChoice::None;
    let modal = egui::Modal::new(egui::Id::new("export_dialog")).show(ctx, |ui| {
        ui.set_max_width(320.0);
        ui.heading("Export");
        ui.add_space(6.0);

        ui.label("Format");
        ui.horizontal(|ui| {
            for (fmt, label) in [
                (ExportFormat::Png, "PNG"),
                (ExportFormat::Jpeg, "JPEG"),
                (ExportFormat::Svg, "SVG"),
                (ExportFormat::Pdf, "PDF"),
            ] {
                if ui.selectable_label(dialog.format == fmt, label).clicked() {
                    dialog.format = fmt;
                }
            }
        });
        ui.add_space(6.0);

        ui.label("Scale");
        ui.horizontal(|ui| {
            for s in [1, 2, 3] {
                if ui
                    .selectable_label(dialog.scale == s, format!("{s}x"))
                    .clicked()
                {
                    dialog.scale = s;
                }
            }
        });
        let (w, h) = page_size;
        let out_w = (w * f64::from(dialog.scale)).round() as i64;
        let out_h = (h * f64::from(dialog.scale)).round() as i64;
        if dialog.format.needs_bake() {
            ui.label(format!("Output: {out_w} × {out_h} px"));
        } else {
            ui.weak(format!(
                "Vector format: {out_w} × {out_h} declared size, full detail at any zoom."
            ));
        }

        if dialog.format == ExportFormat::Jpeg {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label("Quality");
                ui.add(egui::Slider::new(&mut dialog.jpeg_quality, 1..=100));
            });
        }

        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui.button("Export…").clicked() {
                choice = ExportChoice::Pick(ExportSettings {
                    format: dialog.format,
                    scale: dialog.scale,
                    jpeg_quality: dialog.jpeg_quality,
                });
            }
            if ui.button("Cancel").clicked() {
                choice = ExportChoice::Cancel;
            }
        });
    });
    // Clic fuera o Esc equivalen a cancelar.
    if modal.should_close() && matches!(choice, ExportChoice::None) {
        choice = ExportChoice::Cancel;
    }
    choice
}
