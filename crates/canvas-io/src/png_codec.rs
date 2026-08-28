//! Codec PNG↔base64 compartido por el sidecar (`sidecar.rs`) y el
//! portapapeles interno (`clipboard.rs`): ambos embeben los píxeles de las
//! capas raster como PNG en base64 dentro de un documento JSON.

use std::path::Path;

use base64::Engine;

use crate::IoError;

/// Tope de dimensiones de un PNG embebido (sidecar, portapapeles, miniatura):
/// cualquier capa legítima —incluidas fotos de 100 MP— se queda muy por
/// debajo; un PNG hostil con dimensiones desbocadas se rechaza de entrada.
const MAX_LAYER_PNG_DIM: u32 = 20_000;
/// Tope de asignación al decodificar: 512 MiB de RGBA ≈ 134 megapíxeles,
/// por encima de cualquier capa legítima. Sin este tope (y sin el de
/// dimensiones), un sidecar modificado a mano podría pedir gigabytes de
/// memoria en `to_rgba8` antes de que nadie mire el contenido.
const MAX_LAYER_PNG_ALLOC: u64 = 512 * 1024 * 1024;

/// Codifica un buffer RGBA como PNG crudo. `context` solo se usa para dar
/// contexto al mensaje de error si algo falla. Lo usan el contenedor v4 del
/// sidecar (sección binaria) y `encode_layer_png` (portapapeles).
pub(crate) fn encode_png(
    rgba: &[u8],
    width: u32,
    height: u32,
    context: &Path,
) -> Result<Vec<u8>, IoError> {
    let img = image::RgbaImage::from_raw(width, height, rgba.to_vec()).ok_or_else(|| {
        IoError::Encode {
            path: context.to_owned(),
            message: "layer pixels do not match its dimensions".to_owned(),
        }
    })?;
    let mut png = std::io::Cursor::new(Vec::new());
    img.write_to(&mut png, image::ImageFormat::Png)
        .map_err(|e| IoError::Encode {
            path: context.to_owned(),
            message: e.to_string(),
        })?;
    Ok(png.into_inner())
}

/// Codifica un buffer RGBA como PNG y lo empaqueta en base64 (portapapeles
/// interno y miniaturas del sidecar). `context` solo se usa para dar
/// contexto al mensaje de error si algo falla.
pub(crate) fn encode_layer_png(
    rgba: &[u8],
    width: u32,
    height: u32,
    context: &Path,
) -> Result<String, IoError> {
    let png = encode_png(rgba, width, height, context)?;
    Ok(base64::engine::general_purpose::STANDARD.encode(png))
}

/// Decodifica un PNG en base64 a RGBA + dimensiones (portapapeles interno,
/// sidecar legacy v1–v4 y miniaturas).
pub(crate) fn decode_layer_png(
    png_base64: &str,
    context: &Path,
) -> Result<(Vec<u8>, u32, u32), IoError> {
    let png = base64::engine::general_purpose::STANDARD
        .decode(png_base64)
        .map_err(|e| IoError::Decode {
            path: context.to_owned(),
            source: image::ImageError::IoError(std::io::Error::other(format!(
                "invalid base64: {e}"
            ))),
        })?;
    decode_png_bytes(&png, context)
}

/// Decodifica bytes PNG crudos a RGBA + dimensiones (blobs binarios del
/// contenedor v5). Con límites explícitos de dimensiones y asignación: el
/// contenido puede venir de un archivo compartido o modificado fuera de la
/// app, y un PNG con cabeceras infladas no debe poder pedir memoria sin
/// control.
pub(crate) fn decode_png_bytes(png: &[u8], context: &Path) -> Result<(Vec<u8>, u32, u32), IoError> {
    let mut reader =
        image::ImageReader::with_format(std::io::Cursor::new(png), image::ImageFormat::Png);
    // `Limits` es #[non_exhaustive]: se parte de `default()` y se fija cada
    // campo, para no depender de cuáles trae por defecto la versión actual.
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(MAX_LAYER_PNG_ALLOC);
    limits.max_image_width = Some(MAX_LAYER_PNG_DIM);
    limits.max_image_height = Some(MAX_LAYER_PNG_DIM);
    reader.limits(limits);
    let img = reader
        .decode()
        .map_err(|source| IoError::Decode {
            path: context.to_owned(),
            source,
        })?
        .to_rgba8();
    let (width, height) = img.dimensions();
    Ok((img.into_raw(), width, height))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips_rgba_pixels() {
        let rgba: Vec<u8> = (0..4 * 3 * 4).map(|i| (i * 11 % 256) as u8).collect();
        let encoded = encode_layer_png(&rgba, 4, 3, Path::new("test")).unwrap();
        let (decoded, w, h) = decode_layer_png(&encoded, Path::new("test")).unwrap();
        assert_eq!((w, h), (4, 3));
        assert_eq!(decoded, rgba);
    }

    #[test]
    fn rejects_mismatched_dimensions() {
        let rgba = vec![0u8; 10]; // ni de lejos 4*4*4
        let err = encode_layer_png(&rgba, 4, 4, Path::new("test")).unwrap_err();
        assert!(matches!(err, IoError::Encode { .. }));
    }

    #[test]
    fn rejects_garbage_base64() {
        let err = decode_layer_png("no es base64 válido!!", Path::new("test")).unwrap_err();
        assert!(matches!(err, IoError::Decode { .. }));
    }
}
