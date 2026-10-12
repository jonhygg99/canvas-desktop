//! Construcción y codificación del contenido embebido en un `.canvas`
//! (documento + píxeles de cada capa + miniatura opcional).

use std::path::Path;

use canvas_core::Document;

use crate::{IoError, LoadedImage};

use super::{SidecarFile, SidecarImage, PREVIEW_MAX_DIM, SIDECAR_VERSION};

/// Píxeles de una capa a embeber: (id crudo, RGBA, ancho, alto).
pub type LayerPixels = (u64, Vec<u8>, u32, u32);

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

pub(super) const MAX_DECODED_CANVAS_BYTES: u64 = 512 * 1024 * 1024;

impl CanvasPayload {
    /// Valida antes de reemplazar la imagen o el diseño en disco.
    pub fn validate_for_save(&self, path: &Path) -> Result<(), IoError> {
        let mut total = 0u64;
        let mut ids = std::collections::HashSet::new();
        for (id, rgba, width, height) in &self.images {
            let bytes = crate::png_codec::supported_rgba_bytes(*width, *height)
                .filter(|bytes| *bytes == rgba.len() as u64)
                .ok_or_else(|| IoError::Encode {
                    path: path.to_owned(),
                    message:
                        "layer pixels exceed supported save limits or do not match their dimensions"
                            .into(),
                })?;
            total += bytes;
            if total > MAX_DECODED_CANVAS_BYTES || !ids.insert(*id) {
                return Err(IoError::Encode {
                    path: path.to_owned(),
                    message:
                        "editable layers exceed the 512 MiB decoded limit or repeat a layer ID"
                            .into(),
                });
            }
        }
        if let Some(preview) = &self.preview {
            if crate::png_codec::supported_rgba_bytes(preview.width, preview.height)
                != Some(preview.rgba.len() as u64)
            {
                return Err(IoError::Encode {
                    path: path.to_owned(),
                    message: "invalid design preview dimensions".into(),
                });
            }
        }
        Ok(())
    }
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

pub(super) fn encode_payload(
    path: &Path,
    image_hash: Option<String>,
    payload: &CanvasPayload,
) -> Result<Vec<u8>, IoError> {
    payload.validate_for_save(path)?;
    // Los píxeles de las capas van a la sección BINARIA del contenedor v5
    // (PNG crudo, sin base64): ~25 % menos de archivo y sin el coste de
    // codificar/decodificar base64 de un documento con fotos grandes.
    let mut encoded = Vec::with_capacity(payload.images.len());
    let mut blobs: Vec<Vec<u8>> = Vec::with_capacity(payload.images.len());
    for (layer, rgba, w, h) in &payload.images {
        let png = crate::png_codec::encode_png(rgba, *w, *h, path)?;
        let blob = blobs.len() as u32;
        blobs.push(png);
        encoded.push(SidecarImage {
            layer: *layer,
            png_base64: None,
            blob: Some(blob),
        });
    }
    // Solo un diseño autónomo (sin imagen que lo acompañe) necesita su propia
    // miniatura embebida: el hilo de miniaturas de la galería no tiene GPU
    // para hornear la página él mismo (`thumbs::thumbnail`). El sidecar de
    // una imagen es peso muerto aquí — su miniatura sale del propio raster.
    // La miniatura es pequeña (≤ `PREVIEW_MAX_DIM`): sigue en base64 dentro
    // del JSON de cabecera, que es lo único que leen los probes.
    let preview_png = match (&payload.preview, image_hash.is_none()) {
        (Some(p), true) => Some(crate::png_codec::encode_layer_png(
            &p.rgba, p.width, p.height, path,
        )?),
        _ => None,
    };
    let file = SidecarFile {
        version: SIDECAR_VERSION,
        image_hash,
        background_layer: payload.background_layer,
        preview_png,
        document: payload.document.clone(),
        images: encoded,
    };
    let json = serde_json::to_vec_pretty(&file).map_err(|e| IoError::Encode {
        path: path.to_owned(),
        message: format!("serializing the sidecar: {e}"),
    })?;
    let bytes = super::container::encode_container(&json, &blobs);
    if bytes.len() as u64 > super::io::MAX_CANVAS_BYTES {
        return Err(IoError::Encode {
            path: path.to_owned(),
            message: "canvas file exceeds the 512 MiB read limit".into(),
        });
    }
    Ok(bytes)
}
