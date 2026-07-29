//! Portapapeles interno de capas: serializa un subconjunto del documento a
//! JSON para cortar/copiar/pegar/duplicar sin pasar por el portapapeles del
//! sistema operativo (que se reserva para pegar imágenes externas, ver
//! `canvas-app/src/clipboard.rs`, y así no machaca el portapapeles de texto
//! del usuario).

use std::path::Path;

use canvas_core::Layer;
use serde::{Deserialize, Serialize};

use crate::IoError;

/// Versión del formato del portapapeles. Independiente de `SIDECAR_VERSION`:
/// vive solo en memoria durante la sesión, nunca en disco.
const CLIPBOARD_VERSION: u32 = 1;

/// Documento del portapapeles interno, en JSON.
#[derive(Debug, Serialize, Deserialize)]
pub struct ClipboardDoc {
    pub version: u32,
    /// Capas en orden de pila (preorden), con `parent_id` ya recortado a
    /// este conjunto: un padre que quedó fuera se convierte en raíz.
    pub layers: Vec<Layer>,
    /// Píxeles de las capas raster: (id crudo, PNG en base64).
    pub images: Vec<(u64, String)>,
}

impl ClipboardDoc {
    /// Construye un documento del portapapeles con la versión actual: el
    /// llamador (canvas-app) no tiene por qué conocer `CLIPBOARD_VERSION`.
    pub fn new(layers: Vec<Layer>, images: Vec<(u64, String)>) -> Self {
        Self {
            version: CLIPBOARD_VERSION,
            layers,
            images,
        }
    }
}

/// Contexto usado en los mensajes de error: no hay una ruta de archivo real.
fn context() -> &'static Path {
    Path::new("clipboard")
}

pub fn write_clipboard(doc: &ClipboardDoc) -> Result<String, IoError> {
    serde_json::to_string(doc).map_err(|e| IoError::Encode {
        path: context().to_owned(),
        message: format!("serializing the clipboard: {e}"),
    })
}

pub fn read_clipboard(json: &str) -> Result<ClipboardDoc, IoError> {
    let doc: ClipboardDoc = serde_json::from_str(json).map_err(|e| IoError::Decode {
        path: context().to_owned(),
        source: image::ImageError::IoError(std::io::Error::other(format!(
            "corrupt clipboard data: {e}"
        ))),
    })?;
    if doc.version > CLIPBOARD_VERSION {
        return Err(IoError::Decode {
            path: context().to_owned(),
            source: image::ImageError::IoError(std::io::Error::other(
                "clipboard data was written by a newer version of Canvas Desktop".to_owned(),
            )),
        });
    }
    Ok(doc)
}

/// Codifica un buffer RGBA como PNG en base64, para meterlo en `images`.
pub fn encode_layer_png(rgba: &[u8], width: u32, height: u32) -> Result<String, IoError> {
    crate::png_codec::encode_layer_png(rgba, width, height, context())
}

/// Decodifica una entrada de `images` a RGBA + dimensiones.
pub fn decode_layer_png(png_base64: &str) -> Result<(Vec<u8>, u32, u32), IoError> {
    crate::png_codec::decode_layer_png(png_base64, context())
}

#[cfg(test)]
mod tests {
    use super::*;
    use canvas_core::{ImageContent, LayerContent, Transform};

    fn sample_layer(id: u64) -> Layer {
        Layer::new(
            canvas_core::LayerId::from_raw(id),
            "capa",
            Transform::new(0.0, 0.0, 4.0, 2.0),
            LayerContent::Image(ImageContent {
                source_path: None,
                natural_width: 4,
                natural_height: 2,
                crop: None,
            }),
        )
    }

    #[test]
    fn clipboard_roundtrips_layers_and_pixels() {
        let rgba: Vec<u8> = (0..4 * 2 * 4).map(|i| (i * 5 % 256) as u8).collect();
        let png_base64 = encode_layer_png(&rgba, 4, 2).unwrap();
        let doc = ClipboardDoc::new(vec![sample_layer(1)], vec![(1, png_base64)]);

        let json = write_clipboard(&doc).unwrap();
        let restored = read_clipboard(&json).unwrap();
        assert_eq!(restored.layers.len(), 1);
        assert_eq!(restored.layers[0].id, doc.layers[0].id);
        assert_eq!(restored.images.len(), 1);
        let (decoded, w, h) = decode_layer_png(&restored.images[0].1).unwrap();
        assert_eq!((w, h), (4, 2));
        assert_eq!(decoded, rgba);
    }

    #[test]
    fn clipboard_rejects_a_newer_payload() {
        let json = serde_json::json!({
            "version": CLIPBOARD_VERSION + 1,
            "layers": [],
            "images": [],
        })
        .to_string();
        match read_clipboard(&json) {
            Err(IoError::Decode { .. }) => {}
            other => panic!("se esperaba IoError::Decode, se obtuvo otro resultado: {other:?}"),
        }
    }
}
