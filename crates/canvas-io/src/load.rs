use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use image::DynamicImage;

use crate::IoError;

/// Extensiones de imagen que la app sabe abrir (minúsculas).
pub const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp", "gif", "bmp", "svg"];

pub fn is_image_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| IMAGE_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

/// ¿`Ctrl+S` puede sobrescribir este archivo? Un SVG es vectorial (un lienzo
/// raster no puede reescribirlo) y un GIF puede ser animado (sobrescribirlo
/// lo aplanaría a un fotograma): ambos se abren pero solo admiten «Save as…».
pub fn can_overwrite(path: &Path) -> bool {
    !path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "svg" | "gif"))
}

/// Mapa de bits decodificado, ya en RGBA8 y con la orientación EXIF aplicada.
#[derive(Clone, Debug)]
pub struct LoadedImage {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Carga una imagen de disco respetando su orientación EXIF. Los SVG se
/// rasterizan a su tamaño natural; de un GIF animado se toma el primer
/// fotograma (comportamiento por defecto de `image`).
pub fn load_image(path: &Path) -> Result<LoadedImage, IoError> {
    let is_svg = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("svg"));
    if is_svg {
        return crate::load_svg(path);
    }
    let reader = image::ImageReader::open(path).map_err(|source| IoError::Open {
        path: path.to_owned(),
        source,
    })?;
    let decoded = reader
        .with_guessed_format()
        .map_err(|source| IoError::Open {
            path: path.to_owned(),
            source,
        })?
        .decode()
        .map_err(|source| IoError::Decode {
            path: path.to_owned(),
            source,
        })?;

    let oriented = match exif_orientation(path) {
        Some(o) => apply_orientation(decoded, o),
        None => decoded,
    };

    let rgba = oriented.to_rgba8();
    let (width, height) = rgba.dimensions();
    Ok(LoadedImage {
        rgba: rgba.into_raw(),
        width,
        height,
    })
}

/// Lee el tag de orientación EXIF (1..=8). Un fallo aquí nunca es fatal: la
/// mayoría de formatos ni siquiera llevan EXIF.
fn exif_orientation(path: &Path) -> Option<u32> {
    let file = File::open(path).ok()?;
    let mut reader = BufReader::new(file);
    let exif = exif::Reader::new().read_from_container(&mut reader).ok()?;
    let field = exif.get_field(exif::Tag::Orientation, exif::In::PRIMARY)?;
    let value = field.value.get_uint(0)?;
    (2..=8).contains(&value).then_some(value)
}

/// Aplica la transformación que corresponde a cada valor EXIF de orientación.
fn apply_orientation(img: DynamicImage, orientation: u32) -> DynamicImage {
    match orientation {
        2 => img.fliph(),
        3 => img.rotate180(),
        4 => img.flipv(),
        5 => img.rotate90().fliph(),
        6 => img.rotate90(),
        7 => img.rotate270().fliph(),
        8 => img.rotate270(),
        _ => img,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_image_extensions_case_insensitive() {
        assert!(is_image_file(Path::new("foto.PNG")));
        assert!(is_image_file(Path::new("foto.jpeg")));
        assert!(is_image_file(Path::new("c:/x/foto.webp")));
        assert!(!is_image_file(Path::new("doc.pdf")));
        assert!(!is_image_file(Path::new("sin_extension")));
    }

    #[test]
    fn loads_png_from_disk() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("test.png");
        let img = image::RgbaImage::from_fn(4, 2, |x, _| image::Rgba([x as u8, 0, 0, 255]));
        img.save(&path).expect("guardar png de prueba");

        let loaded = load_image(&path).expect("cargar");
        assert_eq!((loaded.width, loaded.height), (4, 2));
        assert_eq!(loaded.rgba.len(), 4 * 2 * 4);
        assert_eq!(loaded.rgba[0..4], [0, 0, 0, 255]);
    }

    #[test]
    fn load_missing_file_reports_path() {
        let err = load_image(Path::new("Z:/no/existe.png")).unwrap_err();
        assert!(err.to_string().contains("existe.png"));
    }

    #[test]
    fn orientation_rotate90_swaps_dimensions() {
        let img = DynamicImage::ImageRgba8(image::RgbaImage::new(4, 2));
        let rotated = apply_orientation(img, 6);
        assert_eq!((rotated.width(), rotated.height()), (2, 4));
    }

    #[test]
    fn animated_gif_loads_first_frame_only() {
        use image::codecs::gif::GifEncoder;
        use image::{Delay, Frame, Rgba, RgbaImage};

        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("anim.gif");
        {
            let file = std::fs::File::create(&path).expect("crear gif");
            let mut enc = GifEncoder::new(file);
            let f1 = RgbaImage::from_pixel(6, 4, Rgba([255, 0, 0, 255]));
            let f2 = RgbaImage::from_pixel(6, 4, Rgba([0, 0, 255, 255]));
            let delay = Delay::from_numer_denom_ms(100, 1);
            enc.encode_frames(vec![
                Frame::from_parts(f1, 0, 0, delay),
                Frame::from_parts(f2, 0, 0, delay),
            ])
            .expect("codificar frames");
        }

        let loaded = load_image(&path).expect("cargar gif");
        assert_eq!((loaded.width, loaded.height), (6, 4));
        // Primer fotograma: rojo (el segundo era azul).
        assert_eq!(loaded.rgba[0], 255);
        assert_eq!(loaded.rgba[2], 0);
    }

    #[test]
    fn svg_and_gif_are_not_overwritable() {
        assert!(!can_overwrite(Path::new("a.svg")));
        assert!(!can_overwrite(Path::new("a.GIF")));
        assert!(can_overwrite(Path::new("a.png")));
        assert!(can_overwrite(Path::new("a.jpg")));
    }
}
