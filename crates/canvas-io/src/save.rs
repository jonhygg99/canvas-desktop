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
/// formato que dicta la extensión del propio `path`. `jpeg_quality` (1–100)
/// solo aplica si el destino es JPEG. `metadata` (ICC/EXIF del original) se
/// reinserta tal cual si el contenedor destino lo soporta. Devuelve los bytes
/// EXACTOS escritos en disco (ya con metadatos reinsertados): quien necesite
/// el hash del archivo recién guardado (`write_sidecar`) lo calcula sobre
/// esto, sin releerlo de disco.
pub fn save_rgba(
    path: &Path,
    rgba: Vec<u8>,
    width: u32,
    height: u32,
    jpeg_quality: u8,
    metadata: Option<&crate::ImageMetadata>,
) -> Result<Vec<u8>, IoError> {
    let format = save_format_from_path(path).ok_or_else(|| IoError::UnsupportedFormat {
        path: path.to_owned(),
    })?;
    let mut bytes = encode(rgba, width, height, format, path, jpeg_quality)?;
    if let Some(meta) = metadata {
        bytes = crate::reinject_metadata(path, bytes, meta);
    }
    write_atomic(path, &bytes)?;
    Ok(bytes)
}

fn encode(
    rgba: Vec<u8>,
    width: u32,
    height: u32,
    format: ImageFormat,
    path: &Path,
    jpeg_quality: u8,
) -> Result<Vec<u8>, IoError> {
    let img = image::RgbaImage::from_raw(width, height, rgba).ok_or_else(|| IoError::Encode {
        path: path.to_owned(),
        message: "baked buffer does not match its dimensions".into(),
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
            let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(
                &mut out,
                jpeg_quality.clamp(1, 100),
            );
            rgb.write_with_encoder(encoder)
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

    // Gancho SOLO para el test de integración que mata el proceso a mitad de
    // guardado: alarga la ventana entre escribir el temporal y sustituir el
    // original. Sin la variable de entorno es un no-op.
    if let Some(ms) = std::env::var("CANVAS_IO_TEST_SLEEP_BEFORE_REPLACE_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
    {
        std::thread::sleep(std::time::Duration::from_millis(ms));
    }

    // Cierra el handle antes de sustituir (obligatorio en Windows).
    let (_file, tmp_path) = tmp
        .keep()
        .map_err(|e| IoError::Write {
            path: path.to_owned(),
            message: format!("conservando el temporal: {e}"),
        })
        .map(|(f, p)| (drop(f), p))?;

    if let Err(e) = replace_with_retry(|| replace_file(&tmp_path, path)) {
        // No dejes basura si la sustitución falla.
        let _ = std::fs::remove_file(&tmp_path);
        return Err(IoError::Write {
            path: path.to_owned(),
            message: friendly_replace_message(&e),
        });
    }
    Ok(())
}

/// Sustitución atómica de un archivo ya escrito en un temporal: la misma
/// primitiva que `write_atomic`, para salidas que no nacen de bytes en RAM
/// (p. ej. el mp4 que escribe ffmpeg). Vale para guardar en el sitio: el
/// temporal se sustituye al final, nunca se lee y escribe el mismo archivo a
/// la vez. Sin rastro si falla.
///
/// Ante `sharing violation` (0x80070020: el reproductor propio aún cerrando
/// su handle, Drive/OneDrive subiendo el archivo, el indexador) reintenta
/// con espera en vez de fallar al primer intento: esos bloqueos son
/// transitorios y el reintento los gana. Otros errores fallan a la primera.
pub fn replace_file_with_tmp(tmp: &Path, dest: &Path) -> Result<(), IoError> {
    if let Err(e) = replace_with_retry(|| replace_file(tmp, dest)) {
        let _ = std::fs::remove_file(tmp);
        return Err(IoError::Write {
            path: dest.to_owned(),
            message: friendly_replace_message(&e),
        });
    }
    Ok(())
}

/// Pausas entre reintentos de sustitución (ms): ~3 s en total. Solo para
/// `sharing violation`; cualquier otro error falla sin esperar.
const REPLACE_RETRY_DELAYS_MS: &[u64] = &[200, 400, 800, 1600];

/// ¿Es un bloqueo transitorio por archivo en uso (reintentable)? En Windows
/// es el HRESULT 0x80070020 (`ERROR_SHARING_VIOLATION`); se detecta por
/// código y por texto porque `ReplaceFileW` llega envuelto en varios
/// `io::Error` distintos según el camino. Pura (testeable sin disco).
pub fn is_sharing_violation(error: &std::io::Error) -> bool {
    if let Some(code) = error.raw_os_error() {
        // 32 = ERROR_SHARING_VIOLATION (Win32); el HRESULT 0x80070020 lo
        // contiene en sus 16 bits bajos en algunos envoltorios.
        if code == 32 || (code as u32 & 0xFFFF) == 32 {
            return true;
        }
    }
    let text = error.to_string().to_ascii_lowercase();
    text.contains("0x80070020")
        || text.contains("sharing violation")
        || text.contains("being used by another process")
}

/// Reintenta `op` mientras falle con `sharing violation`, durmiendo entre
/// intentos según `REPLACE_RETRY_DELAYS_MS`. El resto de errores sale a la
/// primera. `op` corre en hilos de trabajo (guardado/export/render), nunca
/// en el hilo de UI, así que dormir aquí no congela nada. Pura en su
/// política (testeable con una `op` falsa).
pub fn replace_with_retry(mut op: impl FnMut() -> std::io::Result<()>) -> std::io::Result<()> {
    let mut last = None;
    for (attempt, delay) in REPLACE_RETRY_DELAYS_MS
        .iter()
        .copied()
        .chain(std::iter::once(0))
        .enumerate()
    {
        match op() {
            Ok(()) => return Ok(()),
            Err(e) if is_sharing_violation(&e) && delay > 0 => {
                tracing::debug!(
                    "sustitución bloqueada (intento {}): {e}; reintentando en {delay} ms",
                    attempt + 1
                );
                std::thread::sleep(std::time::Duration::from_millis(delay));
                last = Some(e);
            }
            Err(e) => return Err(e),
        }
    }
    Err(last.expect("al menos un intento"))
}

/// Tras agotar los reintentos, el críptico `ReplaceFileW: … (0x80070020)` se
/// traduce a quién suele tener el archivo: el propio preview/audio, o la
/// nube (Drive/OneDrive). Pura (testeable).
fn friendly_replace_message(error: &std::io::Error) -> String {
    if is_sharing_violation(error) {
        format!(
            "{error}. The file is locked by another process — usually the video preview/audio still closing, or cloud sync (Drive/OneDrive) uploading it. Stop playback, pause the sync and try again"
        )
    } else {
        error.to_string()
    }
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

    #[test]
    fn sharing_violation_is_detected_by_code_and_text() {
        // 32 = ERROR_SHARING_VIOLATION; el HRESULT 0x80070020 lo contiene.
        let by_code = std::io::Error::from_raw_os_error(32);
        assert!(is_sharing_violation(&by_code));
        let by_text = std::io::Error::other(
            "ReplaceFileW: El proceso no tiene acceso al archivo porque está siendo utilizado por otro proceso. (0x80070020)",
        );
        assert!(is_sharing_violation(&by_text));
        assert!(!is_sharing_violation(&std::io::Error::other(
            "access denied"
        )));
    }

    #[test]
    fn replace_retries_sharing_violations_and_wins_when_released() {
        // El bloqueo se libera al tercer intento: el reintento lo gana.
        let mut calls = 0;
        let result = replace_with_retry(|| {
            calls += 1;
            if calls < 3 {
                Err(std::io::Error::other("sharing violation (0x80070020)"))
            } else {
                Ok(())
            }
        });
        assert!(result.is_ok());
        assert_eq!(calls, 3);
    }

    #[test]
    fn replace_fails_fast_on_non_sharing_errors() {
        // Un error que no es bloqueo no espera ni reintenta.
        let mut calls = 0;
        let result = replace_with_retry(|| {
            calls += 1;
            Err::<(), _>(std::io::Error::other("access denied"))
        });
        assert!(result.is_err());
        assert_eq!(calls, 1);
    }

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
        save_rgba(&path, checkered(8, 4), 8, 4, 92, None).expect("guardar");
        let back = image::open(&path).expect("reabrir").to_rgba8();
        assert_eq!(back.dimensions(), (8, 4));
        assert_eq!(back.get_pixel(0, 0).0, [255, 0, 0, 255]);
    }

    #[test]
    fn replaces_existing_file_atomically() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("existente.png");
        std::fs::write(&path, b"contenido viejo que no es png").expect("sembrar");

        save_rgba(&path, checkered(4, 4), 4, 4, 92, None).expect("sustituir");
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
        save_rgba(&path, vec![0u8; 4 * 4 * 4], 4, 4, 92, None).expect("guardar jpg");
        let back = image::open(&path).expect("reabrir").to_rgb8();
        let p = back.get_pixel(1, 1).0;
        assert!(
            p[0] > 240 && p[1] > 240 && p[2] > 240,
            "esperaba blanco, fue {p:?}"
        );
    }

    #[test]
    fn returned_bytes_match_the_file_on_disk_exactly() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("t.png");
        let meta = crate::ImageMetadata {
            icc: Some(vec![3u8; 16]),
            exif: None,
        };
        let returned = save_rgba(&path, checkered(6, 6), 6, 6, 92, Some(&meta)).expect("guardar");
        let on_disk = std::fs::read(&path).expect("releer");
        assert_eq!(
            returned, on_disk,
            "los bytes devueltos deben ser IDÉNTICOS a los del archivo: \
             quien hashea el sidecar confía en esto para no releer"
        );
    }

    #[test]
    fn unsupported_extension_is_a_clear_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("dibujo.tiff");
        let err = save_rgba(&path, checkered(2, 2), 2, 2, 92, None).unwrap_err();
        assert!(err.to_string().contains("dibujo.tiff"));
    }

    #[test]
    fn save_rgba_preserves_metadata_end_to_end() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("photo.jpg");
        let meta = crate::ImageMetadata {
            icc: Some(vec![7u8; 24]),
            exif: None,
        };
        save_rgba(&path, checkered(8, 8), 8, 8, 92, Some(&meta)).expect("guardar con metadatos");
        // El archivo en disco decodifica y conserva el bloque ICC tal cual.
        image::open(&path).expect("decodificable");
        let back = crate::extract_metadata_from_file(&path);
        assert_eq!(back.icc.as_deref(), Some(&[7u8; 24][..]));
    }

    #[test]
    fn jpeg_quality_changes_file_size() {
        // Ruido determinista: el JPEG de baja calidad debe pesar menos.
        let mut rgba = Vec::with_capacity(64 * 64 * 4);
        let mut seed: u32 = 0x1234_5678;
        for _ in 0..64 * 64 {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            rgba.extend_from_slice(&[(seed >> 8) as u8, (seed >> 16) as u8, seed as u8, 255]);
        }
        let dir = tempfile::tempdir().expect("tempdir");
        let hi = dir.path().join("hi.jpg");
        let lo = dir.path().join("lo.jpg");
        save_rgba(&hi, rgba.clone(), 64, 64, 95, None).expect("calidad alta");
        save_rgba(&lo, rgba, 64, 64, 20, None).expect("calidad baja");
        let hi_len = std::fs::metadata(&hi).expect("metadata").len();
        let lo_len = std::fs::metadata(&lo).expect("metadata").len();
        assert!(lo_len < hi_len, "esperaba lo ({lo_len}) < hi ({hi_len})");
    }
}
