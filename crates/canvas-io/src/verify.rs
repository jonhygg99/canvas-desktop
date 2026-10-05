//! Clasificación de archivos ilegibles: distingue un archivo vacío, una
//! descarga truncada, un PNG con checksum roto (reparable: el caso
//! `51.png`, 2 bits volteados en un IDAT) y el resto. La sonda de la app
//! (`probe_page_size`) solo lee cabeceras, así que un PNG corrupto con
//! IHDR válido parece sano hasta que se decodifica: esta clasificación es
//! lo que permite a la UI explicar el fallo y ofrecer la acción correcta
//! (reparar / cuarentena / borrar) en vez del mensaje técnico crudo.
//!
//! Es solo ESTRUCTURAL y barata (streaming, sin decodificar píxeles): un
//! JPEG truncado tras su SOF da `Healthy` aquí y fallará después en
//! `load_image`, como hoy. PNG sí se verifica entero por chunks.

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

/// Diagnóstico estructural de un archivo de imagen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CorruptionKind {
    /// Estructura íntegra (no garantiza píxeles decodificables en JPEG).
    Healthy,
    /// 0 bytes: descarga fallida, nada que reparar.
    Empty,
    /// Cortado a mitad (sin IEND, lectura corta): nada que reparar.
    Truncated,
    /// Estructura completa pero con checksum roto (CRC de chunk PNG o
    /// Adler): candidato a reparación exacta.
    ChecksumMismatch,
    /// Se lee pero no es lo que dice su extensión (o no se entiende).
    Undecodable,
    /// No es una imagen que la app juzgue (vídeo, `.canvas`, resto).
    Unsupported,
}

impl CorruptionKind {
    /// Etiqueta corta en inglés: acaba en la UI.
    pub fn describe(self) -> &'static str {
        match self {
            CorruptionKind::Healthy => "healthy file",
            CorruptionKind::Empty => "empty file",
            CorruptionKind::Truncated => "truncated file",
            CorruptionKind::ChecksumMismatch => "file with checksum errors",
            CorruptionKind::Undecodable => "unreadable file",
            CorruptionKind::Unsupported => "unsupported file",
        }
    }
}

/// Clasifica `path` sin decodificar píxeles. Nunca falla: lo ilegible es
/// `Undecodable`, lo ajeno `Unsupported`.
pub fn classify(path: &Path) -> CorruptionKind {
    let len = match std::fs::metadata(path) {
        Ok(meta) => meta.len(),
        Err(_) => return CorruptionKind::Undecodable,
    };
    if len == 0 {
        return CorruptionKind::Empty;
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    match ext.as_deref() {
        Some("png") => classify_png(path, len),
        Some("jpg" | "jpeg" | "gif" | "bmp" | "webp") => classify_raster(path),
        Some("svg") => classify_svg(path),
        _ => CorruptionKind::Unsupported,
    }
}

/// Resto de rásters: magia correcta + cabecera legible → `Healthy`;
/// magia rota → `Undecodable`; magia bien pero cabecera rota (típico
/// corte temprano) → `Truncated`.
fn classify_raster(path: &Path) -> CorruptionKind {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    let mut head = [0u8; 12];
    let n = match File::open(path).and_then(|mut f| f.read(&mut head)) {
        Ok(n) => n,
        Err(_) => return CorruptionKind::Undecodable,
    };
    let magic_ok = match ext.as_str() {
        "jpg" | "jpeg" => n >= 3 && head[..3] == [0xFF, 0xD8, 0xFF],
        "gif" => n >= 6 && (head[..6] == *b"GIF87a" || head[..6] == *b"GIF89a"),
        "bmp" => n >= 2 && head[..2] == *b"BM",
        "webp" => n >= 12 && head[..4] == *b"RIFF" && head[8..12] == *b"WEBP",
        _ => false,
    };
    if !magic_ok {
        return CorruptionKind::Undecodable;
    }
    // Solo cabecera, sin decodificar píxeles (igual que `probe_raster_size`).
    match image::image_dimensions(path) {
        Ok(_) => CorruptionKind::Healthy,
        Err(_) => CorruptionKind::Truncated,
    }
}

/// SVG: parsea el árbol sin rasterizar (igual que `probe_svg_size`).
fn classify_svg(path: &Path) -> CorruptionKind {
    let data = match std::fs::read(path) {
        Ok(data) => data,
        Err(_) => return CorruptionKind::Undecodable,
    };
    match resvg::usvg::Tree::from_data(&data, &resvg::usvg::Options::default()) {
        Ok(_) => CorruptionKind::Healthy,
        Err(_) => CorruptionKind::Undecodable,
    }
}

const PNG_MAGIC: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];

/// PNG chunk a chunk en streaming (sin cargar el archivo entero): detecta
/// truncados (lectura corta o fin sin IEND) y checksums rotos (primer CRC
/// que no cuadre). Tope de chunks contra longitudes absurdas.
fn classify_png(path: &Path, len: u64) -> CorruptionKind {
    if len < 8 {
        return CorruptionKind::Truncated;
    }
    let file = match File::open(path) {
        Ok(file) => file,
        Err(_) => return CorruptionKind::Undecodable,
    };
    let mut reader = BufReader::new(file);
    let mut magic = [0u8; 8];
    if reader.read_exact(&mut magic).is_err() {
        return CorruptionKind::Truncated;
    }
    if magic != PNG_MAGIC {
        return CorruptionKind::Undecodable;
    }
    let mut pos = 8u64;
    let mut chunks = 0u32;
    let mut buf = [0u8; 8192];
    loop {
        if chunks > 100_000 {
            return CorruptionKind::Undecodable;
        }
        let mut head = [0u8; 8];
        if reader.read_exact(&mut head).is_err() {
            return CorruptionKind::Truncated;
        }
        pos += 8;
        let data_len = u32::from_be_bytes([head[0], head[1], head[2], head[3]]) as u64;
        let chunk_type = [head[4], head[5], head[6], head[7]];
        // Los datos + su CRC tienen que caber en el archivo: si la longitud
        // miente (o el archivo está cortado), es truncado sin leer nada más.
        if pos.saturating_add(data_len).saturating_add(4) > len {
            return CorruptionKind::Truncated;
        }
        let mut crc = crc_start(&chunk_type);
        let mut remaining = data_len;
        while remaining > 0 {
            let n = remaining.min(buf.len() as u64) as usize;
            if reader.read_exact(&mut buf[..n]).is_err() {
                return CorruptionKind::Truncated;
            }
            crc = crc_update(crc, &buf[..n]);
            remaining -= n as u64;
        }
        let mut stored = [0u8; 4];
        if reader.read_exact(&mut stored).is_err() {
            return CorruptionKind::Truncated;
        }
        pos += data_len + 4;
        if crc_end(crc) != u32::from_be_bytes(stored) {
            return CorruptionKind::ChecksumMismatch;
        }
        if chunk_type == *b"IEND" {
            return CorruptionKind::Healthy;
        }
        chunks += 1;
    }
}

/// CRC32 IEEE (el de los chunks PNG) sin dependencias: tabla construida
/// en tiempo de compilación.
const fn build_crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0usize;
    while i < 256 {
        let mut crc = i as u32;
        let mut j = 0;
        while j < 8 {
            crc = if crc & 1 == 1 {
                0xEDB8_8320 ^ (crc >> 1)
            } else {
                crc >> 1
            };
            j += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
}

const CRC_TABLE: [u32; 256] = build_crc_table();

/// CRC32 IEEE por partes (reutilizado por `repair`): `crc_start` con los 4
/// bytes del tipo de chunk, `crc_update` con los datos, `crc_end` al final.
pub(crate) fn crc_start(chunk_type: &[u8; 4]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in chunk_type {
        crc = CRC_TABLE[((crc ^ u32::from(byte)) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc
}

pub(crate) fn crc_update(mut crc: u32, data: &[u8]) -> u32 {
    for &byte in data {
        crc = CRC_TABLE[((crc ^ u32::from(byte)) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc
}

pub(crate) fn crc_end(crc: u32) -> u32 {
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// PNG válido 16×16 en memoria.
    fn valid_png_bytes() -> Vec<u8> {
        let img = image::RgbaImage::from_fn(16, 16, |x, y| {
            image::Rgba([(x * 16) as u8, (y * 16) as u8, 128, 255])
        });
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png)
            .expect("codificar png");
        out.into_inner()
    }

    fn write_temp(dir: &tempfile::TempDir, name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let path = dir.path().join(name);
        let mut file = File::create(&path).expect("crear fixture");
        file.write_all(bytes).expect("escribir fixture");
        path
    }

    #[test]
    fn crc32_matches_the_standard_vector() {
        assert_eq!(
            crc_end(crc_update(crc_start(b"1234"), b"56789")),
            0xCBF4_3926
        );
    }

    #[test]
    fn healthy_png_is_healthy() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = write_temp(&dir, "ok.png", &valid_png_bytes());
        assert_eq!(classify(&path), CorruptionKind::Healthy);
    }

    #[test]
    fn empty_file_is_empty() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = write_temp(&dir, "vacio.png", b"");
        assert_eq!(classify(&path), CorruptionKind::Empty);
    }

    #[test]
    fn cut_png_is_truncated() {
        let dir = tempfile::tempdir().expect("tempdir");
        let bytes = valid_png_bytes();
        let path = write_temp(&dir, "corto.png", &bytes[..bytes.len() * 6 / 10]);
        assert_eq!(classify(&path), CorruptionKind::Truncated);
    }

    #[test]
    fn two_flipped_idat_bits_are_a_checksum_mismatch() {
        // El caso 51.png en miniatura: estructura completa, 2 bits mal.
        let dir = tempfile::tempdir().expect("tempdir");
        let mut bytes = valid_png_bytes();
        let marker = bytes
            .windows(4)
            .position(|w| w == b"IDAT")
            .expect("el png trae IDAT");
        bytes[marker + 4] ^= 0x01;
        bytes[marker + 5] ^= 0x01;
        let path = write_temp(&dir, "roto.png", &bytes);
        assert_eq!(classify(&path), CorruptionKind::ChecksumMismatch);
    }

    #[test]
    fn text_named_png_is_undecodable() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = write_temp(&dir, "falso.png", b"esto no es un png");
        assert_eq!(classify(&path), CorruptionKind::Undecodable);
    }

    #[test]
    fn tiny_file_is_truncated() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = write_temp(&dir, "mini.png", b"\x89PNG");
        assert_eq!(classify(&path), CorruptionKind::Truncated);
    }

    #[test]
    fn missing_file_is_undecodable() {
        assert_eq!(
            classify(std::path::Path::new("Z:/no/existe.png")),
            CorruptionKind::Undecodable
        );
    }

    #[test]
    fn jpeg_magic_ok_with_header_is_healthy() {
        let dir = tempfile::tempdir().expect("tempdir");
        let img = image::RgbImage::from_pixel(4, 4, image::Rgb([200, 100, 50]));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Jpeg)
            .expect("codificar jpeg");
        let path = write_temp(&dir, "foto.jpg", &out.into_inner());
        assert_eq!(classify(&path), CorruptionKind::Healthy);
    }

    #[test]
    fn jpeg_with_wrong_magic_is_undecodable() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = write_temp(&dir, "falso.jpg", b"ni de lejos un jpeg....");
        assert_eq!(classify(&path), CorruptionKind::Undecodable);
    }

    #[test]
    fn svg_parses_or_not() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ok = write_temp(
            &dir,
            "dibujo.svg",
            br##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"></svg>"##,
        );
        assert_eq!(classify(&ok), CorruptionKind::Healthy);
        let bad = write_temp(&dir, "roto.svg", b"<svg <oops");
        assert_eq!(classify(&bad), CorruptionKind::Undecodable);
    }

    #[test]
    fn videos_and_designs_are_unsupported() {
        let dir = tempfile::tempdir().expect("tempdir");
        let vid = write_temp(&dir, "clip.mp4", b"\x00\x00\x00 ftyp");
        assert_eq!(classify(&vid), CorruptionKind::Unsupported);
        let design = write_temp(&dir, "doc.canvas", b"{}");
        assert_eq!(classify(&design), CorruptionKind::Unsupported);
    }
}
