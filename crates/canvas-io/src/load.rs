use std::fs::{File, OpenOptions};
use std::io::BufReader;
use std::path::{Path, PathBuf};

use image::DynamicImage;

use crate::IoError;

/// Extensiones de imagen que la app sabe abrir (minúsculas).
pub const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp", "gif", "bmp", "svg"];

/// Extensión de un archivo de diseño (sidecar de imagen o diseño autónomo).
pub const CANVAS_EXTENSION: &str = "canvas";

pub fn is_image_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| IMAGE_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

pub fn is_canvas_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case(CANVAS_EXTENSION))
}

/// ¿Este `.canvas` es un diseño de pleno derecho? Un sidecar (dentro de
/// `.canvas/`, o — legado — un `foto.png.canvas` hermano de `foto.png`) no lo
/// es: ya sale por sí solo en la galería como la imagen que acompaña, listarlo
/// también sería duplicarlo.
pub fn is_standalone_design(path: &Path) -> bool {
    if !is_canvas_file(path) {
        return false;
    }
    // Cualquier `.canvas` dentro de la carpeta oculta de sidecars es, por
    // definición, un sidecar — nunca un diseño autónomo, aunque su nombre
    // interno (`foto.png.canvas`) no tenga imagen hermana a la vista (podría
    // haberse borrado, o vivir en otra ubicación).
    let in_sidecar_dir = path
        .parent()
        .and_then(|p| p.file_name())
        .is_some_and(|n| n == crate::sidecar::SIDECAR_DIR);
    if in_sidecar_dir {
        return false;
    }
    let inner = path.with_extension("");
    !(is_image_file(&inner) && inner.is_file())
}

/// Reserva una ruta libre en `folder`: `{stem}.{ext}`, `{stem} 2.{ext}`…
/// Crea el archivo (vacío) con `create_new` para ganar la carrera contra
/// otro proceso u otra ventana que estuviera creando el mismo nombre; un
/// `exists()` suelto dejaría una ventana TOCTOU en la que la escritura
/// posterior machacaría un archivo del usuario en silencio.
pub fn reserve_unique_path(folder: &Path, stem: &str, ext: &str) -> Result<PathBuf, IoError> {
    for n in 0..10_000u32 {
        let name = if n == 0 {
            format!("{stem}.{ext}")
        } else {
            format!("{stem} {}.{ext}", n + 1)
        };
        let candidate = folder.join(&name);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(_) => return Ok(candidate),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(source) => {
                return Err(IoError::Write {
                    path: candidate,
                    message: source.to_string(),
                })
            }
        }
    }
    Err(IoError::Write {
        path: folder.join(format!("{stem}.{ext}")),
        message: "too many name collisions".to_owned(),
    })
}

/// Primer nombre LIBRE en `folder` (`{stem}.{ext}`, `{stem} 2.{ext}`…) SIN
/// reservarlo: solo un vistazo, para poder enseñar un nombre plausible en la
/// interfaz antes de que el archivo exista. A diferencia de
/// `reserve_unique_path`, NO crea nada. Quien vaya a ESCRIBIR debe llamar de
/// todas formas a `reserve_unique_path`: entre este vistazo y la escritura
/// hay una ventana TOCTOU que solo `create_new` cierra.
pub fn peek_unique_path(folder: &Path, stem: &str, ext: &str) -> PathBuf {
    for n in 0..10_000u32 {
        let name = if n == 0 {
            format!("{stem}.{ext}")
        } else {
            format!("{stem} {}.{ext}", n + 1)
        };
        let candidate = folder.join(&name);
        if !candidate.exists() {
            return candidate;
        }
    }
    folder.join(format!("{stem}.{ext}"))
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
/// mayoría de formatos ni siquiera llevan EXIF. `pub(crate)`: también la usa
/// `probe::probe_page_size` para saber si hay que intercambiar ancho/alto sin
/// decodificar la imagen entera.
pub(crate) fn exif_orientation(path: &Path) -> Option<u32> {
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

    #[test]
    fn unique_path_walks_past_collisions() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p1 = reserve_unique_path(dir.path(), "Untitled", "canvas").expect("primero");
        assert_eq!(p1, dir.path().join("Untitled.canvas"));
        let p2 = reserve_unique_path(dir.path(), "Untitled", "canvas").expect("segundo");
        assert_eq!(p2, dir.path().join("Untitled 2.canvas"));
        let p3 = reserve_unique_path(dir.path(), "Untitled", "canvas").expect("tercero");
        assert_eq!(p3, dir.path().join("Untitled 3.canvas"));
        assert!(p1.is_file());
        assert!(p2.is_file());
        assert!(p3.is_file());
    }

    #[test]
    fn peek_unique_path_does_not_create_anything() {
        let dir = tempfile::tempdir().expect("tempdir");
        let peeked = peek_unique_path(dir.path(), "Untitled", "canvas");
        assert_eq!(peeked, dir.path().join("Untitled.canvas"));
        assert!(!peeked.exists());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn peek_unique_path_skips_taken_names() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("Untitled.canvas"), b"").expect("crear archivo");
        let peeked = peek_unique_path(dir.path(), "Untitled", "canvas");
        assert_eq!(peeked, dir.path().join("Untitled 2.canvas"));
        assert!(!peeked.exists());
    }

    #[test]
    fn standalone_design_ignores_image_sidecars() {
        let dir = tempfile::tempdir().expect("tempdir");
        let image = dir.path().join("foto.png");
        std::fs::write(&image, b"x").unwrap();
        let sidecar = dir.path().join("foto.png.canvas");
        std::fs::write(&sidecar, b"{}").unwrap();
        // Sidecar de una imagen presente: no es un diseño autónomo.
        assert!(!is_standalone_design(&sidecar));

        let orphan = dir.path().join("huerfano.png.canvas");
        std::fs::write(&orphan, b"{}").unwrap();
        // La imagen que acompañaba ya no está: se trata como diseño.
        assert!(is_standalone_design(&orphan));

        let design = dir.path().join("Untitled.canvas");
        std::fs::write(&design, b"{}").unwrap();
        assert!(is_standalone_design(&design));

        // No es un `.canvas` en absoluto.
        assert!(!is_standalone_design(&image));
    }
}
