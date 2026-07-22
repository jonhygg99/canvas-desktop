//! Preservación de metadatos al guardar: perfil ICC y bloque EXIF se extraen
//! del archivo original y se reinsertan TAL CUAL en el archivo recodificado
//! (`img-parts`: JPEG APP1/APP2, PNG iCCP/eXIf, WebP RIFF). Sin esto, guardar
//! un Adobe RGB cambia los colores y el usuario pierde fecha y GPS.
//!
//! Matiz importante: los píxeles se hornean con la orientación EXIF ya
//! aplicada, así que al reinsertar el EXIF hay que dejar `Orientation = 1`
//! o el visor volvería a rotar la imagen.

use img_parts::{ImageEXIF, ImageICC};

/// Bloques de metadatos extraídos del archivo original, listos para
/// reinsertarse en el recodificado.
#[derive(Clone, Debug, Default)]
pub struct ImageMetadata {
    pub icc: Option<Vec<u8>>,
    pub exif: Option<Vec<u8>>,
}

impl ImageMetadata {
    pub fn is_empty(&self) -> bool {
        self.icc.is_none() && self.exif.is_none()
    }
}

/// ¿El contenedor de esta extensión sabe llevar ICC/EXIF con `img-parts`?
fn container_kind(path: &std::path::Path) -> Option<Container> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "jpg" | "jpeg" => Some(Container::Jpeg),
        "png" => Some(Container::Png),
        "webp" => Some(Container::WebP),
        _ => None, // GIF y BMP no llevan ICC/EXIF: no-op
    }
}

#[derive(Clone, Copy)]
enum Container {
    Jpeg,
    Png,
    WebP,
}

/// Extrae ICC y EXIF de los bytes de un archivo (mejor esfuerzo: un archivo
/// que `img-parts` no sepa parsear devuelve metadatos vacíos, nunca error).
pub fn extract_metadata(path: &std::path::Path, bytes: &[u8]) -> ImageMetadata {
    let Some(kind) = container_kind(path) else {
        return ImageMetadata::default();
    };
    let data = img_parts::Bytes::copy_from_slice(bytes);
    let (icc, exif) = match kind {
        Container::Jpeg => match img_parts::jpeg::Jpeg::from_bytes(data) {
            Ok(img) => (img.icc_profile(), img.exif()),
            Err(_) => (None, None),
        },
        Container::Png => match img_parts::png::Png::from_bytes(data) {
            Ok(img) => (img.icc_profile(), img.exif()),
            Err(_) => (None, None),
        },
        Container::WebP => match img_parts::webp::WebP::from_bytes(data) {
            Ok(img) => (img.icc_profile(), img.exif()),
            Err(_) => (None, None),
        },
    };
    ImageMetadata {
        icc: icc.map(|b| b.to_vec()),
        exif: exif.map(|b| b.to_vec()),
    }
}

/// Lee un archivo y extrae sus metadatos (mejor esfuerzo).
pub fn extract_metadata_from_file(path: &std::path::Path) -> ImageMetadata {
    match std::fs::read(path) {
        Ok(bytes) => extract_metadata(path, &bytes),
        Err(_) => ImageMetadata::default(),
    }
}

/// Reinsertar los metadatos en unos bytes recién codificados. Mejor esfuerzo:
/// si algo falla, devuelve los bytes originales intactos (el guardado NUNCA
/// se aborta por un problema de metadatos).
pub fn reinject_metadata(
    path: &std::path::Path,
    encoded: Vec<u8>,
    metadata: &ImageMetadata,
) -> Vec<u8> {
    if metadata.is_empty() {
        return encoded;
    }
    let Some(kind) = container_kind(path) else {
        return encoded;
    };

    // Los píxeles ya llevan la orientación aplicada: el EXIF reinsertado debe
    // decir «Orientation = 1» para no rotar dos veces.
    let exif = metadata.exif.clone().map(|mut blob| {
        patch_orientation_to_1(&mut blob);
        img_parts::Bytes::from(blob)
    });
    let icc = metadata.icc.clone().map(img_parts::Bytes::from);

    let data = img_parts::Bytes::from(encoded.clone());
    let result: Option<Vec<u8>> = match kind {
        Container::Jpeg => img_parts::jpeg::Jpeg::from_bytes(data).ok().map(|mut img| {
            if icc.is_some() {
                img.set_icc_profile(icc);
            }
            if exif.is_some() {
                img.set_exif(exif);
            }
            img.encoder().bytes().to_vec()
        }),
        Container::Png => img_parts::png::Png::from_bytes(data).ok().map(|mut img| {
            if icc.is_some() {
                img.set_icc_profile(icc);
            }
            if exif.is_some() {
                img.set_exif(exif);
            }
            img.encoder().bytes().to_vec()
        }),
        Container::WebP => img_parts::webp::WebP::from_bytes(data).ok().map(|mut img| {
            if icc.is_some() {
                img.set_icc_profile(icc);
            }
            if exif.is_some() {
                img.set_exif(exif);
            }
            img.encoder().bytes().to_vec()
        }),
    };
    match result {
        Some(bytes) => bytes,
        None => {
            tracing::warn!(
                "no se pudieron reinsertar los metadatos en {}; se guarda sin ellos",
                path.display()
            );
            encoded
        }
    }
}

/// Pone `Orientation = 1` en un blob EXIF (stream TIFF, con o sin el prefijo
/// `Exif\0\0` de APP1). Fallo suave: si el blob no se entiende o no contiene
/// el tag 0x0112, se deja intacto — nunca se aborta un guardado por esto.
pub fn patch_orientation_to_1(exif: &mut [u8]) {
    // Prefijo APP1 opcional.
    let tiff_start = if exif.starts_with(b"Exif\0\0") { 6 } else { 0 };
    let tiff = &exif[tiff_start..];
    if tiff.len() < 8 {
        return;
    }
    let big_endian = match &tiff[0..2] {
        b"MM" => true,
        b"II" => false,
        _ => return,
    };
    let u16_at = |data: &[u8], off: usize| -> Option<u16> {
        let b = data.get(off..off + 2)?;
        Some(if big_endian {
            u16::from_be_bytes([b[0], b[1]])
        } else {
            u16::from_le_bytes([b[0], b[1]])
        })
    };
    let u32_at = |data: &[u8], off: usize| -> Option<u32> {
        let b = data.get(off..off + 4)?;
        Some(if big_endian {
            u32::from_be_bytes([b[0], b[1], b[2], b[3]])
        } else {
            u32::from_le_bytes([b[0], b[1], b[2], b[3]])
        })
    };
    if u16_at(tiff, 2) != Some(42) {
        return;
    }
    let Some(ifd0) = u32_at(tiff, 4).map(|v| v as usize) else {
        return;
    };
    let Some(count) = u16_at(tiff, ifd0) else {
        return;
    };
    for i in 0..count as usize {
        let entry = ifd0 + 2 + i * 12;
        let Some(tag) = u16_at(tiff, entry) else {
            return;
        };
        if tag != 0x0112 {
            continue;
        }
        // Tipo SHORT (3), cuenta 1: el valor vive inline en entry+8.
        if u16_at(tiff, entry + 2) != Some(3) || u32_at(tiff, entry + 4) != Some(1) {
            return;
        }
        let value_off = tiff_start + entry + 8;
        if let Some(slot) = exif.get_mut(value_off..value_off + 2) {
            let one: [u8; 2] = if big_endian {
                1u16.to_be_bytes()
            } else {
                1u16.to_le_bytes()
            };
            slot.copy_from_slice(&one);
        }
        return;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// TIFF mínimo little-endian con un solo tag: Orientation = `value`.
    fn tiff_with_orientation(value: u16) -> Vec<u8> {
        let mut t = Vec::new();
        t.extend_from_slice(b"II"); // little-endian
        t.extend_from_slice(&42u16.to_le_bytes());
        t.extend_from_slice(&8u32.to_le_bytes()); // IFD0 en offset 8
        t.extend_from_slice(&1u16.to_le_bytes()); // 1 entrada
        t.extend_from_slice(&0x0112u16.to_le_bytes()); // tag Orientation
        t.extend_from_slice(&3u16.to_le_bytes()); // tipo SHORT
        t.extend_from_slice(&1u32.to_le_bytes()); // cuenta 1
        t.extend_from_slice(&value.to_le_bytes()); // valor inline
        t.extend_from_slice(&0u16.to_le_bytes()); // relleno del valor
        t.extend_from_slice(&0u32.to_le_bytes()); // siguiente IFD: ninguno
        t
    }

    fn read_orientation(tiff: &[u8]) -> u16 {
        u16::from_le_bytes([tiff[18], tiff[19]])
    }

    fn tiny_jpeg() -> Vec<u8> {
        let img = image::RgbImage::from_pixel(4, 4, image::Rgb([200, 100, 50]));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Jpeg)
            .expect("jpeg");
        out.into_inner()
    }

    fn tiny_png() -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(4, 4, image::Rgba([1, 2, 3, 255]));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png)
            .expect("png");
        out.into_inner()
    }

    #[test]
    fn patches_orientation_to_1_little_endian() {
        let mut blob = tiff_with_orientation(6);
        patch_orientation_to_1(&mut blob);
        assert_eq!(read_orientation(&blob), 1);
    }

    #[test]
    fn patches_orientation_with_exif_prefix() {
        let mut blob = b"Exif\0\0".to_vec();
        blob.extend(tiff_with_orientation(8));
        patch_orientation_to_1(&mut blob);
        assert_eq!(u16::from_le_bytes([blob[24], blob[25]]), 1);
    }

    #[test]
    fn garbage_exif_is_left_intact() {
        let mut blob = b"not a tiff at all".to_vec();
        let before = blob.clone();
        patch_orientation_to_1(&mut blob);
        assert_eq!(blob, before);
    }

    #[test]
    fn jpeg_roundtrip_preserves_icc_and_exif_with_orientation_reset() {
        let path = Path::new("photo.jpg");
        let icc = vec![1u8, 2, 3, 4, 5];
        let exif = tiff_with_orientation(6);

        // Siembra un JPEG con metadatos, como haría una cámara.
        let mut seeded = img_parts::jpeg::Jpeg::from_bytes(tiny_jpeg().into()).expect("parse");
        seeded.set_icc_profile(Some(icc.clone().into()));
        seeded.set_exif(Some(exif.clone().into()));
        let original = seeded.encoder().bytes().to_vec();

        let meta = extract_metadata(path, &original);
        assert_eq!(meta.icc.as_deref(), Some(icc.as_slice()));
        assert_eq!(meta.exif.as_deref(), Some(exif.as_slice()));

        // «Guardado»: recodificación limpia + reinserción.
        let saved = reinject_metadata(path, tiny_jpeg(), &meta);
        let back = extract_metadata(path, &saved);
        assert_eq!(back.icc.as_deref(), Some(icc.as_slice()));
        let back_exif = back.exif.expect("exif preservado");
        assert_eq!(read_orientation(&back_exif), 1); // reseteada
        assert_eq!(back_exif.len(), exif.len()); // resto intacto
        assert_eq!(back_exif[..18], exif[..18]);
    }

    #[test]
    fn png_roundtrip_preserves_icc() {
        let path = Path::new("img.png");
        let icc = vec![9u8; 32];
        let mut seeded = img_parts::png::Png::from_bytes(tiny_png().into()).expect("parse");
        seeded.set_icc_profile(Some(icc.clone().into()));
        let original = seeded.encoder().bytes().to_vec();

        let meta = extract_metadata(path, &original);
        assert_eq!(meta.icc.as_deref(), Some(icc.as_slice()));

        let saved = reinject_metadata(path, tiny_png(), &meta);
        let back = extract_metadata(path, &saved);
        assert_eq!(back.icc.as_deref(), Some(icc.as_slice()));
        // El PNG con iCCP reinsertado sigue siendo decodificable.
        image::load_from_memory(&saved).expect("png válido");
    }

    #[test]
    fn bmp_has_no_metadata_and_reinject_is_noop() {
        let path = Path::new("img.bmp");
        let meta = ImageMetadata {
            icc: Some(vec![1, 2, 3]),
            exif: None,
        };
        let bytes = vec![0xAA; 16];
        assert_eq!(reinject_metadata(path, bytes.clone(), &meta), bytes);
        assert!(extract_metadata(path, &bytes).is_empty());
    }
}
