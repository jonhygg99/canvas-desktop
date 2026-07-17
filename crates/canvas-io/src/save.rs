//! Guardado sobre el archivo original con escritura atómica: primero a un
//! temporal EN EL MISMO DIRECTORIO (para que el renombrado no cruce sistemas
//! de ficheros), `fsync`, y luego sustitución del original. Una caída a mitad
//! de guardado nunca deja el archivo del usuario a medias.

use std::io::Write;
use std::path::Path;

use image::ImageFormat;

use crate::IoError;

/// Formatos en los que sabemos codificar al guardar.
pub fn save_format_from_path(path: &Path) -> Option<ImageFormat> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "png" => Some(ImageFormat::Png),
        "jpg" | "jpeg" => Some(ImageFormat::Jpeg),
        "webp" => Some(ImageFormat::WebP),
        "gif" => Some(ImageFormat::Gif),
        "bmp" => Some(ImageFormat::Bmp),
        _ => None,
    }
}

/// Codifica el RGBA horneado y lo escribe atómicamente en `path`, en el
/// formato que dicta la extensión del propio `path`.
pub fn save_rgba(path: &Path, rgba: Vec<u8>, width: u32, height: u32) -> Result<(), IoError> {
    let format = save_format_from_path(path).ok_or_else(|| IoError::UnsupportedFormat {
        path: path.to_owned(),
    })?;
    let bytes = encode(rgba, width, height, format, path)?;
    write_atomic(path, &bytes)
}

fn encode(
    rgba: Vec<u8>,
    width: u32,
    height: u32,
    format: ImageFormat,
    path: &Path,
) -> Result<Vec<u8>, IoError> {
    let img = image::RgbaImage::from_raw(width, height, rgba).ok_or_else(|| IoError::Encode {
        path: path.to_owned(),
        message: "el buffer horneado no cuadra con las dimensiones".into(),
    })?;
    let mut out = std::io::Cursor::new(Vec::new());
    let result = match format {
        // JPEG no tiene alfa: aplana sobre blanco.
        ImageFormat::Jpeg => {
            let mut rgb = image::RgbImage::new(width, height);
            for (dst, src) in rgb.pixels_mut().zip(img.pixels()) {
                let a = u32::from(src[3]);
                for c in 0..3 {
                    dst[c] = ((u32::from(src[c]) * a + 255 * (255 - a)) / 255) as u8;
                }
            }
            rgb.write_to(&mut out, format)
        }
        _ => img.write_to(&mut out, format),
    };
    result.map_err(|e| IoError::Encode {
        path: path.to_owned(),
        message: e.to_string(),
    })?;
    Ok(out.into_inner())
}

/// Escritura atómica: temporal en el mismo directorio + fsync + sustitución.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), IoError> {
    let dir = path
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .ok_or_else(|| IoError::Write {
            path: path.to_owned(),
            message: "la ruta no tiene directorio padre".into(),
        })?;

    let mut tmp = tempfile::Builder::new()
        .prefix(".canvas-desktop-")
        .tempfile_in(dir)
        .map_err(|e| IoError::Write {
            path: path.to_owned(),
            message: format!("no se pudo crear el temporal: {e}"),
        })?;
    tmp.write_all(bytes).map_err(|e| IoError::Write {
        path: path.to_owned(),
        message: format!("escribiendo el temporal: {e}"),
    })?;
    tmp.as_file().sync_all().map_err(|e| IoError::Write {
        path: path.to_owned(),
        message: format!("fsync del temporal: {e}"),
    })?;

    // Cierra el handle antes de sustituir (obligatorio en Windows).
    let (_file, tmp_path) = tmp
        .keep()
        .map_err(|e| IoError::Write {
            path: path.to_owned(),
            message: format!("conservando el temporal: {e}"),
        })
        .map(|(f, p)| (drop(f), p))?;

    if let Err(e) = replace_file(&tmp_path, path) {
        // No dejes basura si la sustitución falla.
        let _ = std::fs::remove_file(&tmp_path);
        return Err(IoError::Write {
            path: path.to_owned(),
            message: e.to_string(),
        });
    }
    Ok(())
}

/// Sustituye `dest` por `tmp`.
///
/// En Windows el renombrado sobre un destino existente no es fiable con
/// `std::fs::rename` (y no conserva atributos/ACLs), así que si el destino
/// existe usamos `ReplaceFileW`, que es la primitiva pensada para esto.
#[cfg(windows)]
fn replace_file(tmp: &Path, dest: &Path) -> std::io::Result<()> {
    if dest.exists() {
        use windows::core::HSTRING;
        use windows::Win32::Storage::FileSystem::{ReplaceFileW, REPLACE_FILE_FLAGS};
        unsafe {
            ReplaceFileW(
                &HSTRING::from(dest.as_os_str()),
                &HSTRING::from(tmp.as_os_str()),
                None,
                REPLACE_FILE_FLAGS(0),
                None,
                None,
            )
        }
        .map_err(|e| std::io::Error::other(format!("ReplaceFileW: {e}")))
    } else {
        std::fs::rename(tmp, dest)
    }
}

#[cfg(not(windows))]
fn replace_file(tmp: &Path, dest: &Path) -> std::io::Result<()> {
    // En POSIX, rename(2) sobre el mismo sistema de ficheros ya es atómico.
    std::fs::rename(tmp, dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checkered(width: u32, height: u32) -> Vec<u8> {
        let mut v = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                let c = if (x + y) % 2 == 0 { 255 } else { 0 };
                v.extend_from_slice(&[c, 0, 255 - c, 255]);
            }
        }
        v
    }

    #[test]
    fn saves_new_png() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("nuevo.png");
        save_rgba(&path, checkered(8, 4), 8, 4).expect("guardar");
        let back = image::open(&path).expect("reabrir").to_rgba8();
        assert_eq!(back.dimensions(), (8, 4));
        assert_eq!(back.get_pixel(0, 0).0, [255, 0, 0, 255]);
    }

    #[test]
    fn replaces_existing_file_atomically() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("existente.png");
        std::fs::write(&path, b"contenido viejo que no es png").expect("sembrar");

        save_rgba(&path, checkered(4, 4), 4, 4).expect("sustituir");
        let back = image::open(&path).expect("reabrir");
        assert_eq!(back.width(), 4);

        // No queda ningún temporal huérfano en el directorio.
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .expect("leer dir")
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with(".canvas-desktop-")
            })
            .collect();
        assert!(leftovers.is_empty());
    }

    #[test]
    fn jpeg_flattens_alpha_over_white() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("t.jpg");
        // Totalmente transparente → debe quedar blanco.
        save_rgba(&path, vec![0u8; 4 * 4 * 4], 4, 4).expect("guardar jpg");
        let back = image::open(&path).expect("reabrir").to_rgb8();
        let p = back.get_pixel(1, 1).0;
        assert!(
            p[0] > 240 && p[1] > 240 && p[2] > 240,
            "esperaba blanco, fue {p:?}"
        );
    }

    #[test]
    fn unsupported_extension_is_a_clear_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("dibujo.tiff");
        let err = save_rgba(&path, checkered(2, 2), 2, 2).unwrap_err();
        assert!(err.to_string().contains("dibujo.tiff"));
    }
}
