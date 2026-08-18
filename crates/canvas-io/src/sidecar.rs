//! Diseños `.canvas`: sidecar de una imagen o diseño autónomo.
//!
//! El mismo formato de archivo sirve para dos papeles, discriminados por un
//! único campo: junto a `foto.png` se escribe `foto.png.canvas` con
//! `image_hash: Some(..)` (preserva la editabilidad — el PNG/JPEG se
//! sobrescribe al guardar, así que sus capas no se pueden recuperar de
//! disco); un diseño nacido en la galería es un `.canvas` autónomo con
//! `image_hash: None`, sin ningún archivo de imagen del que depender. En
//! ambos casos los píxeles de cada capa van embebidos como PNG en base64, y
//! también una miniatura de la página (`preview_png`) para que la galería
//! pinte algo sin tener GPU en su hilo de miniaturas.
//!
//! Al reabrir un sidecar de imagen, si el hash coincide se restauran las
//! capas editables; si no (alguien la editó por fuera), el llamador avisa y
//! deja elegir. Un diseño autónomo no tiene nada que contrastar.

use std::path::{Path, PathBuf};

use canvas_core::Document;
use serde::{Deserialize, Serialize};

use crate::{write_atomic, IoError, LoadedImage};

/// Versión del formato. v2 añadió capas de texto/forma/SVG; v3 añade capas
/// de grupo (`LayerContent::Group`, ilegible para un build v2); v4 hace
/// `image_hash` opcional (diseño autónomo) y añade `preview_png`. Los
/// sidecar v1/v2/v3 se siguen leyendo sin migración (los campos nuevos
/// tienen serde(default)); `parent_id` en particular es `serde(default)` así
/// que todo lo anterior abre como raíz de la pila.
const SIDECAR_VERSION: u32 = 4;

/// Lado mayor de la miniatura embebida en un `.canvas`. Coincide con el
/// `max_dim` que pide la galería, así que el redimensionado posterior en
/// `thumbnail()` es normalmente un no-op.
pub const PREVIEW_MAX_DIM: u32 = 256;

/// Solo el campo `version`, para decidir si merece la pena parsear el resto
/// del archivo. Parsear `SidecarFile` directamente ante un sidecar más nuevo
/// (con variantes de capa que este build no conoce) fallaría con un genérico
/// "corrupt sidecar" en vez del mensaje amable de versión.
#[derive(Debug, Deserialize)]
struct VersionProbe {
    version: u32,
}

/// Como `VersionProbe`, pero también se queda con la miniatura: permite leer
/// solo eso sin decodificar los píxeles de cada capa (lo que pide la
/// galería, por cada celda, en un hilo sin GPU).
#[derive(Debug, Deserialize)]
struct PreviewProbe {
    #[serde(default)]
    preview_png: Option<String>,
}

/// Píxeles de una capa a embeber: (id crudo, RGBA, ancho, alto).
pub type LayerPixels = (u64, Vec<u8>, u32, u32);

/// Ruta del sidecar/diseño de una imagen: nombre completo + `.canvas`
/// (`foto.png` → `foto.png.canvas`, sin colisiones entre extensiones).
pub fn sidecar_path(image_path: &Path) -> PathBuf {
    let mut name = image_path.as_os_str().to_owned();
    name.push(".");
    name.push(crate::CANVAS_EXTENSION);
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
    /// FNV-1a 64 (hex) de los bytes de la imagen que acompaña. `None` en un
    /// diseño autónomo: el `.canvas` ES el documento, no hay imagen que
    /// contrastar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    image_hash: Option<String>,
    /// Id crudo de la capa de «fondo desenfocado», si estaba activa.
    #[serde(default)]
    background_layer: Option<u64>,
    /// Miniatura de la página (PNG en base64, lado mayor ≤ `PREVIEW_MAX_DIM`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    preview_png: Option<String>,
    document: Document,
    images: Vec<SidecarImage>,
}

/// Todo lo que se embebe en un `.canvas`, sea sidecar de imagen o diseño
/// autónomo. Dueño de sus datos porque cruza a un hilo de trabajo.
pub struct CanvasPayload {
    pub document: Document,
    pub images: Vec<LayerPixels>,
    pub background_layer: Option<u64>,
    /// Miniatura ya reducida a `PREVIEW_MAX_DIM` (ver `make_preview`).
    /// `None` si el horneado en GPU falló: el diseño se guarda igual.
    pub preview: Option<LoadedImage>,
}

/// Documento restaurado desde un `.canvas`, con los píxeles ya decodificados.
pub struct RestoredDocument {
    pub document: Document,
    /// (id crudo de capa, píxeles RGBA decodificados)
    pub images: Vec<(u64, LoadedImage)>,
    pub background_layer: Option<u64>,
    /// false si la imagen que acompaña cambió por fuera desde el último
    /// guardado. Siempre `true` en un diseño autónomo: no hay nada que
    /// contrastar.
    pub hash_matches: bool,
    /// El `.canvas` no acompaña a ninguna imagen: `Ctrl+S` lo reescribe tal
    /// cual, sin rasterizar.
    pub standalone: bool,
}

/// Escala a la que hornear la página para obtener la miniatura embebida:
/// nunca agranda, y el lado mayor queda en `PREVIEW_MAX_DIM`.
pub fn preview_scale(page_w: f64, page_h: f64) -> f64 {
    let max = page_w.max(page_h).max(1.0);
    (f64::from(PREVIEW_MAX_DIM) / max).min(1.0)
}

/// Reduce un RGBA ya horneado (a cualquier escala) a la miniatura embebida.
/// `None` si `rgba` no coincide con `width × height × 4`.
pub fn make_preview(rgba: &[u8], width: u32, height: u32) -> Option<LoadedImage> {
    let src = image::RgbaImage::from_raw(width, height, rgba.to_vec())?;
    let (tw, th) = crate::thumbs::fit_within(width, height, PREVIEW_MAX_DIM);
    let thumb = image::imageops::thumbnail(&src, tw, th);
    Some(LoadedImage {
        rgba: thumb.into_raw(),
        width: tw,
        height: th,
    })
}

/// Documento en blanco listo para escribir en disco (`write_design`), con
/// una miniatura de fondo sólido ya sintetizada para que la celda de la
/// galería no salga vacía antes del primer guardado real.
pub fn blank_design(width: f64, height: f64) -> CanvasPayload {
    let mut document = Document::new(width, height);
    if let Ok(page) = document.page_mut() {
        page.background = Some([255, 255, 255, 255]);
    }
    let scale = preview_scale(width, height);
    let pw = ((width * scale).round() as u32).max(1);
    let ph = ((height * scale).round() as u32).max(1);
    let preview = Some(LoadedImage {
        rgba: vec![255u8; pw as usize * ph as usize * 4],
        width: pw,
        height: ph,
    });
    CanvasPayload {
        document,
        images: Vec::new(),
        background_layer: None,
        preview,
    }
}

fn encode_payload(
    path: &Path,
    image_hash: Option<String>,
    payload: &CanvasPayload,
) -> Result<Vec<u8>, IoError> {
    let mut encoded = Vec::with_capacity(payload.images.len());
    for (layer, rgba, w, h) in &payload.images {
        let png_base64 = crate::png_codec::encode_layer_png(rgba, *w, *h, path)?;
        encoded.push(SidecarImage {
            layer: *layer,
            png_base64,
        });
    }
    let preview_png = match &payload.preview {
        Some(p) => Some(crate::png_codec::encode_layer_png(
            &p.rgba, p.width, p.height, path,
        )?),
        None => None,
    };
    let file = SidecarFile {
        version: SIDECAR_VERSION,
        image_hash,
        background_layer: payload.background_layer,
        preview_png,
        document: payload.document.clone(),
        images: encoded,
    };
    serde_json::to_vec_pretty(&file).map_err(|e| IoError::Encode {
        path: path.to_owned(),
        message: format!("serializing the sidecar: {e}"),
    })
}

/// Escribe (atómico) el sidecar de `image_path`. `image_bytes` son los bytes
/// codificados de la imagen recién guardada (para el hash).
pub fn write_sidecar(
    image_path: &Path,
    image_bytes: &[u8],
    payload: &CanvasPayload,
) -> Result<(), IoError> {
    let path = sidecar_path(image_path);
    let hash = Some(format!("{:016x}", fnv1a64(image_bytes)));
    let json = encode_payload(&path, hash, payload)?;
    write_atomic(&path, &json)
}

/// Escribe (atómico) un diseño autónomo en `path`: sin imagen que contrastar.
pub fn write_design(path: &Path, payload: &CanvasPayload) -> Result<(), IoError> {
    let json = encode_payload(path, None, payload)?;
    write_atomic(path, &json)
}

/// (documento + imágenes crudas, píxeles ya decodificados por capa).
type ParsedCanvasFile = (SidecarFile, Vec<(u64, LoadedImage)>);

/// Lee y valida el `.canvas` en `path`: probe de versión, parseo completo,
/// reparación de la invariante de preorden y decodificación de los píxeles
/// de cada capa. Compartido por `read_sidecar` y `read_design`.
fn read_canvas_file(path: &Path) -> Result<Option<ParsedCanvasFile>, IoError> {
    let json = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(IoError::Open {
                path: path.to_owned(),
                source,
            })
        }
    };
    // Comprueba la versión ANTES de intentar parsear el documento completo:
    // un sidecar más nuevo puede traer variantes de capa que este build no
    // conoce, y eso debe reportarse como "versión más reciente", no como
    // "corrupto".
    let probe: VersionProbe = serde_json::from_slice(&json).map_err(|e| IoError::Decode {
        path: path.to_owned(),
        source: image::ImageError::IoError(std::io::Error::other(format!("corrupt sidecar: {e}"))),
    })?;
    if probe.version > SIDECAR_VERSION {
        return Err(IoError::Decode {
            path: path.to_owned(),
            source: image::ImageError::IoError(std::io::Error::other(format!(
                "this file was created with a newer version of Canvas Desktop \
                 (version {}, this app understands up to {SIDECAR_VERSION})",
                probe.version
            ))),
        });
    }
    let mut file: SidecarFile = serde_json::from_slice(&json).map_err(|e| IoError::Decode {
        path: path.to_owned(),
        source: image::ImageError::IoError(std::io::Error::other(format!("corrupt sidecar: {e}"))),
    })?;
    // Repara la invariante de preorden ante un sidecar ajeno o corrupto
    // (padres colgando, ciclos…) antes de que nada más lo toque.
    for page in &mut file.document.pages {
        page.normalize_tree();
    }

    let mut images = Vec::with_capacity(file.images.len());
    for entry in &file.images {
        let (rgba, width, height) = crate::png_codec::decode_layer_png(&entry.png_base64, path)?;
        images.push((
            entry.layer,
            LoadedImage {
                rgba,
                width,
                height,
            },
        ));
    }
    Ok(Some((file, images)))
}

/// Lee el sidecar de `image_path`, si existe. Devuelve `Ok(None)` si no hay
/// sidecar; error solo si existe pero está corrupto o es de versión futura.
/// Si la imagen que acompañaba ya no está en disco, no hay nada que
/// contrastar y `hash_matches` sale en `true`.
pub fn read_sidecar(image_path: &Path) -> Result<Option<RestoredDocument>, IoError> {
    let path = sidecar_path(image_path);
    let Some((file, images)) = read_canvas_file(&path)? else {
        return Ok(None);
    };
    let standalone = file.image_hash.is_none();
    let hash_matches = match (&file.image_hash, std::fs::read(image_path)) {
        (Some(h), Ok(bytes)) => format!("{:016x}", fnv1a64(&bytes)) == *h,
        _ => true,
    };
    Ok(Some(RestoredDocument {
        document: file.document,
        images,
        background_layer: file.background_layer,
        hash_matches,
        standalone,
    }))
}

/// Lee un diseño autónomo en `path`. A diferencia de `read_sidecar`, un
/// diseño ausente es un error real (no hay imagen hermana que abrir en su
/// lugar).
pub fn read_design(path: &Path) -> Result<RestoredDocument, IoError> {
    let Some((file, images)) = read_canvas_file(path)? else {
        return Err(IoError::Open {
            path: path.to_owned(),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "design file not found"),
        });
    };
    Ok(RestoredDocument {
        document: file.document,
        images,
        background_layer: file.background_layer,
        hash_matches: true,
        standalone: true,
    })
}

/// Solo la miniatura embebida de un `.canvas`: no decodifica los PNG de las
/// capas. `Ok(None)` si el archivo no existe o no tiene miniatura (p. ej. un
/// diseño creado pero nunca guardado con éxito).
pub fn read_preview(path: &Path) -> Result<Option<LoadedImage>, IoError> {
    let json = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(IoError::Open {
                path: path.to_owned(),
                source,
            })
        }
    };
    let probe: PreviewProbe = serde_json::from_slice(&json).map_err(|e| IoError::Decode {
        path: path.to_owned(),
        source: image::ImageError::IoError(std::io::Error::other(format!("corrupt sidecar: {e}"))),
    })?;
    match probe.preview_png {
        Some(b64) => {
            let (rgba, width, height) = crate::png_codec::decode_layer_png(&b64, path)?;
            Ok(Some(LoadedImage {
                rgba,
                width,
                height,
            }))
        }
        None => Ok(None),
    }
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

    fn sample_payload(
        document: &Document,
        images: &[LayerPixels],
        background_layer: Option<u64>,
    ) -> CanvasPayload {
        CanvasPayload {
            document: document.clone(),
            images: images.to_vec(),
            background_layer,
            preview: None,
        }
    }

    #[test]
    fn roundtrip_restores_document_and_pixels() {
        let dir = tempfile::tempdir().expect("tempdir");
        let image_path = dir.path().join("foto.png");
        let fake_image = b"bytes de la imagen guardada";
        std::fs::write(&image_path, fake_image).unwrap();

        let (doc, images) = sample_doc();
        let payload = sample_payload(&doc, &images, None);
        write_sidecar(&image_path, fake_image, &payload).expect("escribir");
        assert!(sidecar_path(&image_path).exists());

        let restored = read_sidecar(&image_path)
            .expect("leer")
            .expect("hay sidecar");
        assert!(restored.hash_matches);
        assert!(!restored.standalone);
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
        let payload = sample_payload(&doc, &images, None);
        write_sidecar(&image_path, b"original", &payload).expect("escribir");

        // Alguien edita la imagen por fuera.
        std::fs::write(&image_path, b"modificada por otro programa").unwrap();
        let restored = read_sidecar(&image_path)
            .expect("leer")
            .expect("hay sidecar");
        assert!(!restored.hash_matches);
    }

    #[test]
    fn groups_survive_a_sidecar_roundtrip() {
        use canvas_core::Layer;

        let dir = tempfile::tempdir().expect("tempdir");
        let image_path = dir.path().join("foto.png");
        let fake_image = b"bytes de la imagen guardada";
        std::fs::write(&image_path, fake_image).unwrap();

        let (mut doc, images) = sample_doc();
        let child = doc
            .layer(canvas_core::LayerId::from_raw(images[0].0))
            .unwrap()
            .id;
        let group_id = doc.allocate_layer_id();
        {
            let page = doc.page_mut().unwrap();
            page.insert_child(Layer::group(group_id, "Group"), None, 1);
            page.move_subtree(child, Some(group_id), 0).unwrap();
        }

        let payload = sample_payload(&doc, &images, None);
        write_sidecar(&image_path, fake_image, &payload).expect("escribir");
        let restored = read_sidecar(&image_path)
            .expect("leer")
            .expect("hay sidecar");
        assert_eq!(restored.document, doc);
        assert_eq!(
            restored.document.layer(child).unwrap().parent_id,
            Some(group_id)
        );
        assert!(restored.document.page().unwrap().is_group(group_id));
    }

    #[test]
    fn version_probe_rejects_a_newer_sidecar_before_parsing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let image_path = dir.path().join("foto.png");
        std::fs::write(&image_path, b"x").unwrap();

        // Documento con un campo de version futura y contenido que este
        // build no sabria deserializar (para probar que el rechazo ocurre
        // ANTES de intentar parsear `SidecarFile`).
        let fake_future = serde_json::json!({
            "version": SIDECAR_VERSION + 1,
            "algo_que_no_existe_todavia": { "esto": "no es un Document" },
        });
        let path = sidecar_path(&image_path);
        std::fs::write(&path, serde_json::to_vec(&fake_future).unwrap()).unwrap();

        match read_sidecar(&image_path) {
            Err(IoError::Decode { .. }) => {}
            Err(other) => panic!("se esperaba IoError::Decode, se obtuvo otro error: {other}"),
            Ok(_) => panic!("un sidecar de version futura no deberia leerse con exito"),
        }
    }

    #[test]
    fn missing_sidecar_is_none_and_delete_is_quiet() {
        let dir = tempfile::tempdir().expect("tempdir");
        let image_path = dir.path().join("foto.png");
        std::fs::write(&image_path, b"x").unwrap();
        assert!(read_sidecar(&image_path).expect("leer").is_none());
        delete_sidecar(&image_path); // no explota sin sidecar

        let (doc, images) = sample_doc();
        let payload = sample_payload(&doc, &images, Some(7));
        write_sidecar(&image_path, b"x", &payload).expect("escribir");
        let restored = read_sidecar(&image_path).unwrap().unwrap();
        assert_eq!(restored.background_layer, Some(7));
        delete_sidecar(&image_path);
        assert!(!sidecar_path(&image_path).exists());
    }

    #[test]
    fn design_roundtrips_without_any_image() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("Untitled.canvas");

        let (doc, images) = sample_doc();
        let payload = sample_payload(&doc, &images, None);
        write_design(&path, &payload).expect("escribir diseño");

        let restored = read_design(&path).expect("leer diseño");
        assert!(restored.standalone);
        assert!(restored.hash_matches);
        assert_eq!(restored.document, doc);
        assert_eq!(restored.images.len(), 1);
        assert_eq!(restored.images[0].1.rgba, images[0].1);
    }

    #[test]
    fn design_preview_roundtrips() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("Untitled.canvas");

        let (doc, images) = sample_doc();
        let mut payload = sample_payload(&doc, &images, None);
        let preview_rgba: Vec<u8> = (0..8 * 4 * 4).map(|i| (i * 3 % 256) as u8).collect();
        payload.preview = Some(LoadedImage {
            rgba: preview_rgba.clone(),
            width: 8,
            height: 4,
        });
        write_design(&path, &payload).expect("escribir diseño");

        let preview = read_preview(&path)
            .expect("leer preview")
            .expect("hay preview");
        assert_eq!((preview.width, preview.height), (8, 4));
        assert_eq!(preview.rgba, preview_rgba);

        // Sin miniatura embebida: `Ok(None)`, no error.
        let no_preview_path = dir.path().join("SinPreview.canvas");
        let mut payload_sin = sample_payload(&doc, &images, None);
        payload_sin.preview = None;
        write_design(&no_preview_path, &payload_sin).expect("escribir diseño sin preview");
        assert!(read_preview(&no_preview_path)
            .expect("leer preview")
            .is_none());
    }

    /// Red de compatibilidad hacia atrás: un sidecar v3 (sin `preview_png`,
    /// `image_hash` obligatorio) escrito a mano se sigue leyendo tal cual.
    #[test]
    fn v3_sidecar_still_restores() {
        let dir = tempfile::tempdir().expect("tempdir");
        let image_path = dir.path().join("foto.png");
        let fake_image = b"bytes de la imagen guardada";
        std::fs::write(&image_path, fake_image).unwrap();

        let (doc, images) = sample_doc();
        let encoded_images: Vec<_> = images
            .iter()
            .map(|(layer, rgba, w, h)| {
                serde_json::json!({
                    "layer": layer,
                    "png_base64": crate::png_codec::encode_layer_png(
                        rgba,
                        *w,
                        *h,
                        Path::new("test"),
                    )
                    .unwrap(),
                })
            })
            .collect();
        let v3_json = serde_json::json!({
            "version": 3,
            "image_hash": format!("{:016x}", fnv1a64(fake_image)),
            "background_layer": null,
            "document": doc,
            "images": encoded_images,
        });
        std::fs::write(
            sidecar_path(&image_path),
            serde_json::to_vec(&v3_json).unwrap(),
        )
        .unwrap();

        let restored = read_sidecar(&image_path)
            .expect("leer sidecar v3")
            .expect("hay sidecar");
        assert!(!restored.standalone);
        assert!(restored.hash_matches);
        assert_eq!(restored.document, doc);
    }

    #[test]
    fn sidecar_survives_a_missing_companion_image() {
        let dir = tempfile::tempdir().expect("tempdir");
        let image_path = dir.path().join("foto.png");
        let fake_image = b"bytes de la imagen guardada";
        std::fs::write(&image_path, fake_image).unwrap();

        let (doc, images) = sample_doc();
        let payload = sample_payload(&doc, &images, None);
        write_sidecar(&image_path, fake_image, &payload).expect("escribir");

        // Alguien borra la imagen original; el sidecar sigue ahí.
        std::fs::remove_file(&image_path).unwrap();
        let restored = read_sidecar(&image_path)
            .expect("leer")
            .expect("hay sidecar aunque falte la imagen");
        assert!(restored.hash_matches);
        assert!(!restored.standalone);
    }

    #[test]
    fn an_absent_hash_marks_the_file_standalone() {
        let dir = tempfile::tempdir().expect("tempdir");
        let image_path = dir.path().join("foto.png");
        std::fs::write(&image_path, b"imagen real").unwrap();

        // Un `.canvas` con el nombre de sidecar de `foto.png`, pero escrito
        // como diseño autónomo (sin `image_hash`).
        let (doc, images) = sample_doc();
        let payload = sample_payload(&doc, &images, None);
        write_design(&sidecar_path(&image_path), &payload).expect("escribir diseño");

        let restored = read_sidecar(&image_path)
            .expect("leer")
            .expect("hay archivo en la ruta del sidecar");
        assert!(restored.standalone);
        assert!(restored.hash_matches);
    }
}
