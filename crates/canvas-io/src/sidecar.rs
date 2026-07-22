//! Sidecar `.canvas`: preserva la editabilidad al guardar sobre un PNG/JPEG.
//!
//! Junto a `foto.png` se escribe `foto.png.canvas` con el documento
//! serializado y los píxeles de cada capa embebidos (PNG en base64: el
//! archivo original se sobrescribe al guardar, así que no se pueden
//! recuperar de disco). Al reabrir la imagen, si el hash coincide se
//! restauran las capas editables; si no (alguien la editó por fuera), el
//! llamador avisa y deja elegir.

use std::path::{Path, PathBuf};

use base64::Engine;
use canvas_core::Document;
use serde::{Deserialize, Serialize};

use crate::{write_atomic, IoError, LoadedImage};

/// Versión del formato. v2 añade capas de texto/forma/SVG; los sidecar v1
/// se leen sin migración (los campos nuevos tienen serde(default)).
const SIDECAR_VERSION: u32 = 2;

/// Píxeles de una capa a embeber: (id crudo, RGBA, ancho, alto).
pub type LayerPixels = (u64, Vec<u8>, u32, u32);

/// Ruta del sidecar de una imagen: nombre completo + `.canvas`
/// (`foto.png` → `foto.png.canvas`, sin colisiones entre extensiones).
pub fn sidecar_path(image_path: &Path) -> PathBuf {
    let mut name = image_path.as_os_str().to_owned();
    name.push(".canvas");
    PathBuf::from(name)
}

/// FNV-1a de 64 bits: determinista entre ejecuciones y versiones de Rust
/// (el `DefaultHasher` de std no lo garantiza).
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[derive(Debug, Serialize, Deserialize)]
struct SidecarImage {
    layer: u64,
    png_base64: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct SidecarFile {
    version: u32,
    /// FNV-1a 64 (hex) de los bytes del archivo de imagen que acompaña.
    image_hash: String,
    /// Id crudo de la capa de «fondo desenfocado», si estaba activa.
    background_layer: Option<u64>,
    document: Document,
    images: Vec<SidecarImage>,
}

/// Documento restaurado desde un sidecar, con los píxeles ya decodificados.
pub struct RestoredDocument {
    pub document: Document,
    /// (id crudo de capa, píxeles RGBA decodificados)
    pub images: Vec<(u64, LoadedImage)>,
    pub background_layer: Option<u64>,
    /// false si la imagen fue modificada por fuera desde el último guardado.
    pub hash_matches: bool,
}

/// Escribe (atómico) el sidecar de `image_path`. `image_bytes` son los bytes
/// codificados de la imagen recién guardada (para el hash) y `images` los
/// píxeles RGBA de cada capa.
pub fn write_sidecar(
    image_path: &Path,
    image_bytes: &[u8],
    document: &Document,
    images: &[LayerPixels],
    background_layer: Option<u64>,
) -> Result<(), IoError> {
    let path = sidecar_path(image_path);
    let mut encoded = Vec::with_capacity(images.len());
    for (layer, rgba, w, h) in images {
        let img =
            image::RgbaImage::from_raw(*w, *h, rgba.clone()).ok_or_else(|| IoError::Encode {
                path: path.clone(),
                message: format!("layer {layer} pixels do not match its dimensions"),
            })?;
        let mut png = std::io::Cursor::new(Vec::new());
        img.write_to(&mut png, image::ImageFormat::Png)
            .map_err(|e| IoError::Encode {
                path: path.clone(),
                message: format!("layer {layer}: {e}"),
            })?;
        encoded.push(SidecarImage {
            layer: *layer,
            png_base64: base64::engine::general_purpose::STANDARD.encode(png.into_inner()),
        });
    }

    let file = SidecarFile {
        version: SIDECAR_VERSION,
        image_hash: format!("{:016x}", fnv1a64(image_bytes)),
        background_layer,
        document: document.clone(),
        images: encoded,
    };
    let json = serde_json::to_vec_pretty(&file).map_err(|e| IoError::Encode {
        path: path.clone(),
        message: format!("serializing the sidecar: {e}"),
    })?;
    write_atomic(&path, &json)
}

/// Lee el sidecar de `image_path`, si existe. Devuelve `Ok(None)` si no hay
/// sidecar; error solo si existe pero está corrupto o es de versión futura.
pub fn read_sidecar(image_path: &Path) -> Result<Option<RestoredDocument>, IoError> {
    let path = sidecar_path(image_path);
    let json = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(IoError::Open { path, source }),
    };
    let file: SidecarFile = serde_json::from_slice(&json).map_err(|e| IoError::Decode {
        path: path.clone(),
        source: image::ImageError::IoError(std::io::Error::other(format!("corrupt sidecar: {e}"))),
    })?;
    if file.version > SIDECAR_VERSION {
        return Err(IoError::Decode {
            path: path.clone(),
            source: image::ImageError::IoError(std::io::Error::other(format!(
                "this file was created with a newer version of Canvas Desktop \
                 (version {}, this app understands up to {SIDECAR_VERSION})",
                file.version
            ))),
        });
    }

    // ¿El archivo de imagen sigue siendo el que este sidecar acompañaba?
    let image_bytes = std::fs::read(image_path).map_err(|source| IoError::Open {
        path: image_path.to_owned(),
        source,
    })?;
    let hash_matches = format!("{:016x}", fnv1a64(&image_bytes)) == file.image_hash;

    let mut images = Vec::with_capacity(file.images.len());
    for entry in &file.images {
        let png = base64::engine::general_purpose::STANDARD
            .decode(&entry.png_base64)
            .map_err(|e| IoError::Decode {
                path: path.clone(),
                source: image::ImageError::IoError(std::io::Error::other(format!(
                    "layer {} base64: {e}",
                    entry.layer
                ))),
            })?;
        let img = image::load_from_memory_with_format(&png, image::ImageFormat::Png)
            .map_err(|source| IoError::Decode {
                path: path.clone(),
                source,
            })?
            .to_rgba8();
        let (width, height) = img.dimensions();
        images.push((
            entry.layer,
            LoadedImage {
                rgba: img.into_raw(),
                width,
                height,
            },
        ));
    }

    Ok(Some(RestoredDocument {
        document: file.document,
        images,
        background_layer: file.background_layer,
        hash_matches,
    }))
}

/// Borra el sidecar si existe (guardado con el sidecar desactivado).
pub fn delete_sidecar(image_path: &Path) {
    let path = sidecar_path(image_path);
    if path.exists() {
        if let Err(e) = std::fs::remove_file(&path) {
            tracing::warn!("no se pudo borrar el sidecar {}: {e}", path.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use canvas_core::{ImageContent, LayerContent, Transform};

    fn sample_doc() -> (Document, Vec<LayerPixels>) {
        let mut doc = Document::new(200.0, 100.0);
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
            .unwrap();
        doc.layer_mut(id).unwrap().effects.blur_radius = 7.0;
        let rgba: Vec<u8> = (0..4 * 2 * 4).map(|i| (i * 7 % 256) as u8).collect();
        (doc, vec![(id.raw(), rgba, 4, 2)])
    }

    #[test]
    fn roundtrip_restores_document_and_pixels() {
        let dir = tempfile::tempdir().expect("tempdir");
        let image_path = dir.path().join("foto.png");
        let fake_image = b"bytes de la imagen guardada";
        std::fs::write(&image_path, fake_image).unwrap();

        let (doc, images) = sample_doc();
        write_sidecar(&image_path, fake_image, &doc, &images, None).expect("escribir");
        assert!(sidecar_path(&image_path).exists());

        let restored = read_sidecar(&image_path)
            .expect("leer")
            .expect("hay sidecar");
        assert!(restored.hash_matches);
        assert_eq!(restored.document, doc);
        assert_eq!(restored.images.len(), 1);
        let (layer, pixels) = &restored.images[0];
        assert_eq!(*layer, images[0].0);
        assert_eq!((pixels.width, pixels.height), (4, 2));
        assert_eq!(pixels.rgba, images[0].1);
    }

    #[test]
    fn detects_externally_modified_image() {
        let dir = tempfile::tempdir().expect("tempdir");
        let image_path = dir.path().join("foto.png");
        std::fs::write(&image_path, b"original").unwrap();

        let (doc, images) = sample_doc();
        write_sidecar(&image_path, b"original", &doc, &images, None).expect("escribir");

        // Alguien edita la imagen por fuera.
        std::fs::write(&image_path, b"modificada por otro programa").unwrap();
        let restored = read_sidecar(&image_path)
            .expect("leer")
            .expect("hay sidecar");
        assert!(!restored.hash_matches);
    }

    #[test]
    fn missing_sidecar_is_none_and_delete_is_quiet() {
        let dir = tempfile::tempdir().expect("tempdir");
        let image_path = dir.path().join("foto.png");
        std::fs::write(&image_path, b"x").unwrap();
        assert!(read_sidecar(&image_path).expect("leer").is_none());
        delete_sidecar(&image_path); // no explota sin sidecar

        let (doc, images) = sample_doc();
        write_sidecar(&image_path, b"x", &doc, &images, Some(7)).expect("escribir");
        let restored = read_sidecar(&image_path).unwrap().unwrap();
        assert_eq!(restored.background_layer, Some(7));
        delete_sidecar(&image_path);
        assert!(!sidecar_path(&image_path).exists());
    }
}
