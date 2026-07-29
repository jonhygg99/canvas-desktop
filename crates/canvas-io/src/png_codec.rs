//! Codec PNG↔base64 compartido por el sidecar (`sidecar.rs`) y el
//! portapapeles interno (`clipboard.rs`): ambos embeben los píxeles de las
//! capas raster como PNG en base64 dentro de un documento JSON.

use std::path::Path;

use base64::Engine;

use crate::IoError;

/// Codifica un buffer RGBA como PNG y lo empaqueta en base64. `context` solo
/// se usa para dar contexto al mensaje de error si algo falla.
pub(crate) fn encode_layer_png(
    rgba: &[u8],
    width: u32,
    height: u32,
    context: &Path,
) -> Result<String, IoError> {
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
    Ok(base64::engine::general_purpose::STANDARD.encode(png.into_inner()))
}

/// Decodifica un PNG en base64 a RGBA + dimensiones.
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
    let img = image::load_from_memory_with_format(&png, image::ImageFormat::Png)
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
