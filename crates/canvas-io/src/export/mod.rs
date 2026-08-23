//! Exportacion a PNG/JPEG/SVG/PDF. PNG/JPEG reutilizan `bake_page` de
//! canvas-render (el llamador se encarga de hornear y llamar a `save_rgba`);
//! este modulo solo cubre el camino vectorial.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use canvas_core::{TextContent, TextLine};

use crate::IoError;

mod pdf;
mod svg;

#[cfg(test)]
mod tests;

pub use pdf::svg_to_pdf;
pub use svg::document_to_svg;

/// Formato de exportación elegido en el diálogo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Png,
    Jpeg,
    Svg,
    Pdf,
}

impl ExportFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Svg => "svg",
            Self::Pdf => "pdf",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Png => "PNG",
            Self::Jpeg => "JPEG",
            Self::Svg => "SVG",
            Self::Pdf => "PDF",
        }
    }

    /// ¿Necesita el horneado de la GPU (`bake_page`)? PNG/JPEG sí; SVG/PDF
    /// se generan a mano a partir del documento.
    pub fn needs_bake(self) -> bool {
        matches!(self, Self::Png | Self::Jpeg)
    }

    pub fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_lowercase();
        match ext.as_str() {
            "png" => Some(Self::Png),
            "jpg" | "jpeg" => Some(Self::Jpeg),
            "svg" => Some(Self::Svg),
            "pdf" => Some(Self::Pdf),
            _ => None,
        }
    }
}

/// PNG en base64 (mismo formato que produce `encode_layer_png`) de cada capa
/// raster (Image/Svg), por id crudo de capa. El llamador ya debe haber
/// aplicado desenfoque/ajustes de color antes de codificar: aquí no se
/// reprocesa nada.
pub type ExportImages = HashMap<u64, String>;

/// Resuelve el salto de línea de un texto: lo implementa `canvas-render`
/// con parley (el mismo layout que ve el lienzo), inyectado así para que
/// este crate no dependa de un motor de texto.
pub type TextLineBreaker<'a> = dyn Fn(&TextContent, f64) -> Vec<TextLine> + 'a;

fn export_error(message: impl Into<String>) -> IoError {
    IoError::Encode {
        path: PathBuf::from("export"),
        message: message.into(),
    }
}
