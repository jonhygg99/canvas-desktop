//! Apertura de SVG: se rasteriza a su tamaño natural con `resvg`. Un lienzo
//! raster no puede reescribir un SVG vectorial, así que estos archivos se
//! abren pero nunca se sobrescriben (la app redirige a «Save as…»).

use std::path::Path;

use crate::{IoError, LoadedImage};

/// Rasteriza un SVG a RGBA a su tamaño natural (mínimo 1×1).
pub fn load_svg(path: &Path) -> Result<LoadedImage, IoError> {
    let data = std::fs::read(path).map_err(|source| IoError::Open {
        path: path.to_owned(),
        source,
    })?;

    let mut options = resvg::usvg::Options::default();
    // Texto dentro del SVG: hace falta el catálogo de fuentes del sistema.
    options.fontdb_mut().load_system_fonts();
    let tree = resvg::usvg::Tree::from_data(&data, &options).map_err(|e| IoError::Decode {
        path: path.to_owned(),
        source: image::ImageError::IoError(std::io::Error::other(format!("SVG: {e}"))),
    })?;

    let size = tree.size();
    let width = (size.width().ceil() as u32).max(1);
    let height = (size.height().ceil() as u32).max(1);
    let mut pixmap =
        resvg::tiny_skia::Pixmap::new(width, height).ok_or_else(|| IoError::Decode {
            path: path.to_owned(),
            source: image::ImageError::IoError(std::io::Error::other(format!(
                "SVG too large to rasterize ({width}×{height})"
            ))),
        })?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::identity(),
        &mut pixmap.as_mut(),
    );

    // tiny-skia trabaja con alfa premultiplicado; el resto de la app espera
    // RGBA directo.
    let rgba: Vec<u8> = pixmap
        .pixels()
        .iter()
        .flat_map(|p| {
            let d = p.demultiply();
            [d.red(), d.green(), d.blue(), d.alpha()]
        })
        .collect();

    Ok(LoadedImage {
        rgba,
        width,
        height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rasterizes_svg_at_natural_size() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("dibujo.svg");
        std::fs::write(
            &path,
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20">
                 <rect x="0" y="0" width="40" height="20" fill="#ff0000"/>
               </svg>"##,
        )
        .expect("escribir svg");

        let img = load_svg(&path).expect("rasterizar");
        assert_eq!((img.width, img.height), (40, 20));
        // Píxel del centro: rojo opaco.
        let center = ((10 * 40 + 20) * 4) as usize;
        assert_eq!(&img.rgba[center..center + 4], &[255, 0, 0, 255]);
    }

    #[test]
    fn invalid_svg_is_a_clear_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("roto.svg");
        std::fs::write(&path, "this is not xml").expect("escribir");
        let err = load_svg(&path).unwrap_err();
        assert!(err.to_string().contains("roto.svg"));
    }
}
