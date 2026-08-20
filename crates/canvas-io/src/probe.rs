//! Sondeo del tamaño de página de un archivo SIN decodificarlo entero: la
//! cabecera de la imagen, o el JSON de un `.canvas`. Es lo que permite apilar
//! N lienzos de la baraja del editor antes de haber cargado ninguno — sin
//! esto haría falta decodificar cada imagen completa (potencialmente decenas
//! de MP) solo para saber cuánto sitio reservarle en la pila.

use std::path::Path;

use crate::{is_canvas_file, IoError};

/// Tamaño de página de `path`, sin decodificar el archivo entero. En este
/// orden:
/// 1. `.canvas` → tamaño de `document.pages[0]` (`sidecar::read_page_size`).
/// 2. Imagen con sidecar → el sidecar manda: su página puede no coincidir con
///    los píxeles del archivo (`from_restored` usa su documento, no la
///    imagen). Resuelve el sidecar con `find_sidecar` (uno por archivo); para
///    sondear una carpeta entera, usar `probe_page_size_with` con un sidecar
///    ya resuelto de un solo listado (evita N `is_file()`).
/// 3. Imagen rasterizada → solo la cabecera (`image::image_dimensions`), con
///    el intercambio ancho/alto que corresponda a su orientación EXIF — sin
///    esto, una foto de móvil en vertical (`Orientation: 6`) probaría un
///    tamaño apaisado, la mitad al revés de como `load_image` la carga.
/// 4. SVG → tamaño del árbol `usvg` (viewBox/width/height), sin rasterizar.
pub fn probe_page_size(path: &Path) -> Result<(f64, f64), IoError> {
    probe_page_size_with(path, crate::find_sidecar(path).as_deref())
}

/// Como `probe_page_size`, pero con el sidecar (si lo hay) YA resuelto por el
/// llamador. Pensado para sondear una carpeta entera en paralelo: listar
/// `.canvas/` una vez fuera del bucle es muchísimo más barato que un
/// `is_file()` por archivo.
pub fn probe_page_size_with(path: &Path, sidecar: Option<&Path>) -> Result<(f64, f64), IoError> {
    if is_canvas_file(path) {
        return crate::sidecar::read_page_size(path);
    }
    if let Some(sidecar) = sidecar {
        return crate::sidecar::read_page_size(sidecar);
    }
    let is_svg = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("svg"));
    if is_svg {
        return probe_svg_size(path);
    }
    probe_raster_size(path)
}

/// Cabecera de una imagen rasterizada (PNG/JPEG/WebP/GIF/BMP), orientada.
fn probe_raster_size(path: &Path) -> Result<(f64, f64), IoError> {
    let (w, h) = image::image_dimensions(path).map_err(|source| IoError::Decode {
        path: path.to_owned(),
        source,
    })?;
    // Orientaciones 5..=8 rotan 90°: intercambian ancho y alto respecto a lo
    // que dice la cabecera (`load_image` aplica la misma rotación al decodificar).
    let (w, h) = match crate::load::exif_orientation(path) {
        Some(5..=8) => (h, w),
        _ => (w, h),
    };
    Ok((f64::from(w), f64::from(h)))
}

/// Tamaño declarado de un SVG (viewBox o `width`/`height`), sin rasterizar:
/// evita cargar el catálogo de fuentes del sistema, que no hace falta para
/// medir el documento.
fn probe_svg_size(path: &Path) -> Result<(f64, f64), IoError> {
    let data = std::fs::read(path).map_err(|source| IoError::Open {
        path: path.to_owned(),
        source,
    })?;
    let options = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_data(&data, &options).map_err(|e| IoError::Decode {
        path: path.to_owned(),
        source: image::ImageError::IoError(std::io::Error::other(format!("SVG: {e}"))),
    })?;
    let size = tree.size();
    Ok((
        f64::from(size.width()).max(1.0),
        f64::from(size.height()).max(1.0),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probes_a_plain_png() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("foto.png");
        image::RgbaImage::new(30, 20).save(&path).expect("guardar");
        assert_eq!(probe_page_size(&path).expect("probe"), (30.0, 20.0));
    }

    #[test]
    fn probes_svg_without_rasterizing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("dibujo.svg");
        std::fs::write(
            &path,
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="120" height="80"></svg>"##,
        )
        .expect("escribir");
        assert_eq!(probe_page_size(&path).expect("probe"), (120.0, 80.0));
    }

    #[test]
    fn probes_a_standalone_design() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("Untitled.canvas");
        let payload = crate::blank_design(640.0, 480.0);
        crate::write_design(&path, &payload).expect("escribir diseño");
        assert_eq!(probe_page_size(&path).expect("probe"), (640.0, 480.0));
    }

    #[test]
    fn image_with_sidecar_defers_to_the_sidecar_page_size() {
        let dir = tempfile::tempdir().expect("tempdir");
        let image_path = dir.path().join("foto.png");
        image::RgbaImage::new(100, 100)
            .save(&image_path)
            .expect("guardar imagen");
        // El sidecar dice un tamaño de página distinto al de los píxeles: el
        // sidecar manda, porque es lo que `from_restored` reabriría.
        let payload = crate::blank_design(200.0, 50.0);
        crate::write_sidecar(&image_path, b"dummy image bytes", &payload).expect("sidecar");
        assert_eq!(probe_page_size(&image_path).expect("probe"), (200.0, 50.0));
    }

    #[test]
    fn missing_file_is_a_clear_error() {
        let err = probe_page_size(Path::new("Z:/no/existe.png")).unwrap_err();
        assert!(err.to_string().contains("existe.png"));
    }

    /// El caso que motiva el intercambio ancho/alto: una foto de móvil en
    /// vertical, guardada apaisada con `Orientation: 6` (rota 90°). Sin el
    /// intercambio, la sonda daría el tamaño "crudo" del archivo (6×4) en vez
    /// del tamaño con el que realmente se ve y se carga (4×6) — la mitad de
    /// las fotos de un móvil quedarían con el hueco al revés en la baraja.
    #[test]
    fn probe_matches_load_image_for_exif_orientation_6() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("photo.jpg");

        let img = image::RgbImage::from_pixel(6, 4, image::Rgb([200, 100, 50]));
        let mut jpeg_bytes = std::io::Cursor::new(Vec::new());
        img.write_to(&mut jpeg_bytes, image::ImageFormat::Jpeg)
            .expect("codificar jpeg");

        // TIFF mínimo little-endian con un solo tag: Orientation = 6.
        let mut exif = Vec::new();
        exif.extend_from_slice(b"II");
        exif.extend_from_slice(&42u16.to_le_bytes());
        exif.extend_from_slice(&8u32.to_le_bytes());
        exif.extend_from_slice(&1u16.to_le_bytes());
        exif.extend_from_slice(&0x0112u16.to_le_bytes());
        exif.extend_from_slice(&3u16.to_le_bytes());
        exif.extend_from_slice(&1u32.to_le_bytes());
        exif.extend_from_slice(&6u16.to_le_bytes());
        exif.extend_from_slice(&0u16.to_le_bytes());
        exif.extend_from_slice(&0u32.to_le_bytes());

        use img_parts::ImageEXIF;
        let mut jpeg = img_parts::jpeg::Jpeg::from_bytes(jpeg_bytes.into_inner().into())
            .expect("parsear jpeg");
        jpeg.set_exif(Some(exif.into()));
        std::fs::write(&path, jpeg.encoder().bytes()).expect("escribir jpeg");

        let loaded = crate::load_image(&path).expect("load_image");
        let probed = probe_page_size(&path).expect("probe");
        assert_eq!(probed, (f64::from(loaded.width), f64::from(loaded.height)));
        assert_eq!(probed, (4.0, 6.0));
    }
}
