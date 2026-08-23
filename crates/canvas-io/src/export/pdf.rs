//! Conversion del SVG ya generado a PDF.
//!
//! Va por `svg2pdf` sobre **`resvg::usvg`** (no la usvg propia de svg2pdf) para
//! que ambos compartan una sola version de usvg en el arbol: reverificar con
//! `cargo tree -i usvg` despues de tocar `resvg` o `svg2pdf`.

use crate::IoError;

use super::export_error;

/// Convierte un SVG (típicamente el que genera `document_to_svg`) a PDF de
/// una página, sin rasterizar texto ni formas.
pub fn svg_to_pdf(svg: &str) -> Result<Vec<u8>, IoError> {
    let mut options = resvg::usvg::Options::default();
    options.fontdb_mut().load_system_fonts();
    let tree = resvg::usvg::Tree::from_str(svg, &options).map_err(|e| {
        export_error(format!(
            "the generated SVG could not be parsed for PDF conversion: {e}"
        ))
    })?;
    svg2pdf::to_pdf(
        &tree,
        svg2pdf::ConversionOptions::default(),
        svg2pdf::PageOptions::default(),
    )
    .map_err(|e| export_error(format!("SVG to PDF conversion failed: {e:?}")))
}
