use std::fs::{File, OpenOptions};
use std::io::BufReader;
use std::path::{Path, PathBuf};

use image::DynamicImage;

use crate::sidecar::{read_design, read_sidecar, RestoredDocument};
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

/// Qué devolvió abrir una ruta de disco. `Flat` es la imagen aplanada;
/// `Restored` trae las capas editables del sidecar de una imagen; `Design`
/// es un `.canvas` autónomo leído como documento completo.
pub enum OpenOutcome {
    Flat(LoadedImage),
    Restored(RestoredDocument),
    Design(RestoredDocument),
}

/// Punto ÚNICO de entrada para abrir una ruta de disco, venga de argv,
/// galería, baraja del editor o «abrir con». La política vive aquí y solo
/// aquí (antes estaba copiada en `spawn_load_image` y `spawn_load_slot`):
///
/// - un `.canvas` ES el documento: se lee con `read_design`;
/// - una imagen restaura sus capas desde su sidecar si lo tiene y es
///   legible; un sidecar corrupto degrada a imagen aplanada con un warning
///   (nunca impide abrir — el original siempre gana);
/// - `with_sidecar` en `false` salta la restauración («Editable sidecar»
///   desactivado) y va directo a los píxeles.
pub fn open_document(path: &Path, with_sidecar: bool) -> Result<OpenOutcome, IoError> {
    if is_canvas_file(path) {
        return read_design(path).map(OpenOutcome::Design);
    }
    if with_sidecar {
        match read_sidecar(path) {
            Ok(Some(restored)) => return Ok(OpenOutcome::Restored(restored)),
            Ok(None) => {}
            Err(e) => {
                tracing::warn!("sidecar ilegible ({e}); abriendo la imagen aplanada")
            }
        }
    }
    load_image(path).map(OpenOutcome::Flat)
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

/// Como `reserve_unique_path`, pero para lienzos nuevos sin nombre: en vez
/// de `Untitled.ext`/`Untitled 2.ext`, reserva el primer NÚMERO libre
/// (`1.ext`, `2.ext`…).
pub fn reserve_numbered_path(folder: &Path, ext: &str) -> Result<PathBuf, IoError> {
    for n in 1..=10_000u64 {
        let candidate = folder.join(format!("{n}.{ext}"));
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
        path: folder.join(format!("1.{ext}")),
        message: "too many name collisions".to_owned(),
    })
}

/// Como `peek_unique_path`, pero numerado (ver `reserve_numbered_path`). La
/// búsqueda arranca en `hint` (normalmente el `next_id` de la baraja) en vez
/// de en 1: así, si el usuario crea varias ranuras provisionales seguidas
/// antes de que ninguna se reserve de verdad en disco, cada una se enseña
/// con un número distinto en vez de todas mostrando el mismo "1".
pub fn peek_numbered_path(folder: &Path, ext: &str, hint: u64) -> PathBuf {
    for n in 0..10_000u64 {
        let candidate = folder.join(format!("{}.{ext}", hint + n));
        if !candidate.exists() {
            return candidate;
        }
    }
    folder.join(format!("{hint}.{ext}"))
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
    fn numbered_path_walks_past_collisions() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p1 = reserve_numbered_path(dir.path(), "png").expect("primero");
        assert_eq!(p1, dir.path().join("1.png"));
        let p2 = reserve_numbered_path(dir.path(), "png").expect("segundo");
        assert_eq!(p2, dir.path().join("2.png"));
        let p3 = reserve_numbered_path(dir.path(), "png").expect("tercero");
        assert_eq!(p3, dir.path().join("3.png"));
        assert!(p1.is_file());
        assert!(p2.is_file());
        assert!(p3.is_file());
    }

    #[test]
    fn peek_numbered_path_does_not_create_anything() {
        let dir = tempfile::tempdir().expect("tempdir");
        let peeked = peek_numbered_path(dir.path(), "png", 1);
        assert_eq!(peeked, dir.path().join("1.png"));
        assert!(!peeked.exists());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn peek_numbered_path_skips_taken_names_from_the_hint_up() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("5.png"), b"").expect("crear archivo");
        let peeked = peek_numbered_path(dir.path(), "png", 5);
        assert_eq!(peeked, dir.path().join("6.png"));
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

    // ——— open_document: la política única de apertura ———

    use canvas_core::{ImageContent, LayerContent, Transform};

    /// Documento de prueba con una capa de imagen 4×2 (blur 7) y sus píxeles.
    fn sample_payload() -> (crate::CanvasPayload, Vec<u8>) {
        let mut doc = canvas_core::Document::new(200.0, 100.0);
        let id = doc
            .add_layer(
                "img",
                Transform::new(25.0, 10.0, 50.0, 40.0),
                LayerContent::Image(ImageContent {
                    source_path: None,
                    natural_width: 4,
                    natural_height: 2,
                    crop: None,
                }),
            )
            .expect("añadir capa");
        doc.layer_mut(id).expect("capa").effects.blur_radius = 7.0;
        let rgba: Vec<u8> = (0..4 * 2 * 4).map(|i| (i * 7 % 256) as u8).collect();
        (
            crate::CanvasPayload {
                document: doc,
                images: vec![(id.raw(), rgba, 4, 2)],
                background_layer: None,
                preview: None,
            },
            b"bytes de la imagen guardada".to_vec(),
        )
    }

    fn tiny_png(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        let img = image::RgbaImage::from_fn(4, 2, |x, _| image::Rgba([x as u8, 0, 0, 255]));
        img.save(&path).expect("guardar png de prueba");
        path
    }

    #[test]
    fn open_document_without_sidecar_loads_flat() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = tiny_png(dir.path(), "foto.png");

        let outcome = open_document(&path, true).expect("abrir");
        let OpenOutcome::Flat(loaded) = outcome else {
            panic!("se esperaba Flat para una imagen sin sidecar");
        };
        assert_eq!((loaded.width, loaded.height), (4, 2));
    }

    #[test]
    fn open_document_restores_editable_layers_from_sidecar() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = tiny_png(dir.path(), "foto.png");
        let (payload, _) = sample_payload();
        // El sidecar hashea los bytes REALES en disco (los del PNG), no unos
        // cualesquiera: así `hash_matches` sale true.
        let image_bytes = std::fs::read(&path).expect("leer png");
        crate::write_sidecar(&path, &image_bytes, &payload).expect("escribir sidecar");

        let outcome = open_document(&path, true).expect("abrir");
        let OpenOutcome::Restored(restored) = outcome else {
            panic!("se esperaba Restored para una imagen con sidecar válido");
        };
        assert!(restored.hash_matches);
        let id = restored
            .document
            .page()
            .expect("página")
            .layers
            .first()
            .expect("capa")
            .id;
        assert_eq!(
            restored.document.layer(id).unwrap().effects.blur_radius,
            7.0
        );
    }

    #[test]
    fn open_document_skips_sidecar_when_disabled() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = tiny_png(dir.path(), "foto.png");
        let (payload, image_bytes) = sample_payload();
        crate::write_sidecar(&path, &image_bytes, &payload).expect("escribir sidecar");

        let outcome = open_document(&path, false).expect("abrir");
        assert!(matches!(outcome, OpenOutcome::Flat(_)));
    }

    #[test]
    fn open_document_corrupt_sidecar_degrades_to_flat() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = tiny_png(dir.path(), "foto.png");
        let sidecar = crate::sidecar_path(&path);
        std::fs::create_dir_all(sidecar.parent().expect("padre")).unwrap();
        std::fs::write(&sidecar, b"no es un contenedor v5 ni un json").unwrap();

        let outcome = open_document(&path, true).expect("abrir la imagen igualmente");
        assert!(matches!(outcome, OpenOutcome::Flat(_)));
    }

    #[test]
    fn open_document_reads_a_canvas_file_as_design() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("Untitled.canvas");
        let (payload, _image_bytes) = sample_payload();
        crate::write_design(&path, &payload).expect("escribir diseño");

        let outcome = open_document(&path, true).expect("abrir");
        assert!(matches!(outcome, OpenOutcome::Design(_)));
    }
}
