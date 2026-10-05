//! Reparación exacta de PNG con checksum roto (el caso `51.png`): un IDAT
//! estructuralmente completo pero con 1–2 bits o una ráfaga corta dañados.
//! La técnica es algebraica, no heurística: el CRC32 es lineal, así que el
//! síndrome `calculado ^ guardado` determina el patrón de error; cada
//! candidato que cuadra el CRC se verifica inflando el stream zlib con
//! comprobación de Adler32 y longitud exacta. Solo hay dos resultados:
//! bytes idénticos al original, o error. La app nunca inventa píxeles.
//!
//! Nota: el decodificador PNG de `image` NO verifica el Adler del stream
//! (un candidato con CRC de chunk correcto pero Adler roto decodifica
//! "bien"), así que la verificación la hace este módulo con `miniz_oxide`.
//!
//! Límites honestos: un solo chunk dañado y que sea IDAT, sin entrelazado;
//! el chunk dañado ocupa ≤ 32 KiB (los codificadores habituales emiten
//! IDAT de ~8 KiB); inflado esperado ≤ 512 MiB; presupuesto total de 20 s.
//! Todo lo demás es `Unrepairable` (0 bytes, truncados, daño multi-chunk)
//! o `BudgetExceeded`.

use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

use thiserror::Error;

use crate::verify::{crc_end, crc_start, crc_update};

/// El archivo entero debe caber en RAM para clonarlo al devolverlo.
const MAX_FILE_BYTES: u64 = 128 * 1024 * 1024;
/// Tope del chunk dañado: la tabla de efectos de bit cuesta O(n²).
const MAX_BAD_CHUNK_BYTES: usize = 32 * 1024;
/// Tope del inflado esperado (filtros incluidos): evita pedir gigabytes.
const MAX_INFLATED_BYTES: usize = 512 * 1024 * 1024;
/// Presupuesto total de CPU de un intento de reparación.
const TIME_BUDGET: Duration = Duration::from_secs(20);

/// Por qué no se pudo reparar. Mensajes en inglés: acaban en la UI.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum RepairError {
    #[error("this file cannot be repaired automatically (only single-chunk PNG checksum damage is repairable)")]
    Unrepairable,
    #[error("repair budget exceeded (file too large or fix not found in time)")]
    BudgetExceeded,
}

/// Un chunk PNG localizado en los bytes del archivo.
#[derive(Clone, Copy)]
struct Chunk {
    typ: [u8; 4],
    data_off: usize,
    data_len: usize,
}

/// Lo que el walk extrae una sola vez: el chunk dañado, el stream IDAT
/// concatenado con su desplazamiento, y el tamaño inflado esperado.
struct Parsed {
    bad: Chunk,
    bad_idat_off: usize,
    idat: Vec<u8>,
    inflated_len: usize,
}

/// Bytes por fila (más su byte de filtro) según IHDR. `None` ante
/// combinaciones que no se verifican (profundidades raras, entrelazado).
fn row_bytes(bit_depth: u8, color_type: u8, width: u32) -> Option<u64> {
    let w = u64::from(width);
    let samples = match (color_type, bit_depth) {
        (0, 1) | (3, 1) => w.div_ceil(8),
        (0, 2) | (3, 2) => w.div_ceil(4),
        (0, 4) | (3, 4) => w.div_ceil(2),
        (0, 8) | (3, 8) => w,
        (0, 16) => w * 2,
        (2, 8) => w * 3,
        (2, 16) => w * 6,
        (4, 8) => w * 2,
        (4, 16) => w * 4,
        (6, 8) => w * 4,
        (6, 16) => w * 8,
        _ => return None,
    };
    Some(samples + 1)
}

/// Recorre los chunks sobre los bytes ya leídos. Falla con `Unrepairable`
/// ante cualquier cosa que no sea "estructura completa con EXACTAMENTE un
/// IDAT con CRC malo": truncados, vacíos efectivos, PNG sanos (nada que
/// arreglar), daño multi-chunk o fuera de IDAT, entrelazados e inflados
/// gigantes.
fn walk_single_bad_idat(raw: &[u8]) -> Result<Parsed, RepairError> {
    if raw.len() < 8 || raw[..8] != [137, 80, 78, 71, 13, 10, 26, 10] {
        return Err(RepairError::Unrepairable);
    }
    let mut pos = 8usize;
    let mut bad: Option<Chunk> = None;
    let mut bad_count = 0u32;
    let mut idat: Vec<u8> = Vec::new();
    let mut bad_idat_off = 0usize;
    let mut inflated_len = None;
    let mut first = true;
    let mut seen_iend = false;
    while pos + 8 <= raw.len() {
        let data_len =
            u32::from_be_bytes([raw[pos], raw[pos + 1], raw[pos + 2], raw[pos + 3]]) as usize;
        let typ = [raw[pos + 4], raw[pos + 5], raw[pos + 6], raw[pos + 7]];
        let data_off = pos + 8;
        let crc_off = data_off.saturating_add(data_len);
        if crc_off.saturating_add(4) > raw.len() {
            return Err(RepairError::Unrepairable);
        }
        if first && typ != *b"IHDR" {
            return Err(RepairError::Unrepairable);
        }
        first = false;
        if typ == *b"IHDR" {
            if data_len < 13 {
                return Err(RepairError::Unrepairable);
            }
            let width = u32::from_be_bytes([
                raw[data_off],
                raw[data_off + 1],
                raw[data_off + 2],
                raw[data_off + 3],
            ]);
            let height = u32::from_be_bytes([
                raw[data_off + 4],
                raw[data_off + 5],
                raw[data_off + 6],
                raw[data_off + 7],
            ]);
            if width == 0 || height == 0 || raw[data_off + 12] != 0 {
                return Err(RepairError::Unrepairable);
            }
            let row = row_bytes(raw[data_off + 8], raw[data_off + 9], width)
                .ok_or(RepairError::Unrepairable)?;
            let total = row
                .checked_mul(u64::from(height))
                .ok_or(RepairError::Unrepairable)?;
            if total > MAX_INFLATED_BYTES as u64 {
                return Err(RepairError::BudgetExceeded);
            }
            inflated_len = Some(total as usize);
        }
        let mut crc = crc_start(&typ);
        crc = crc_update(crc, &raw[data_off..data_off + data_len]);
        let stored = u32::from_be_bytes([
            raw[crc_off],
            raw[crc_off + 1],
            raw[crc_off + 2],
            raw[crc_off + 3],
        ]);
        if typ == *b"IDAT" {
            if crc_end(crc) != stored {
                bad_count += 1;
                bad_idat_off = idat.len();
                bad = Some(Chunk {
                    typ,
                    data_off,
                    data_len,
                });
            }
            idat.extend_from_slice(&raw[data_off..data_off + data_len]);
        } else if crc_end(crc) != stored {
            return Err(RepairError::Unrepairable);
        }
        pos = crc_off + 4;
        if typ == *b"IEND" {
            seen_iend = true;
            break;
        }
    }
    let (Some(chunk), Some(inflated_len)) = (bad, inflated_len) else {
        return Err(RepairError::Unrepairable);
    };
    if !seen_iend || bad_count != 1 {
        return Err(RepairError::Unrepairable);
    }
    Ok(Parsed {
        bad: chunk,
        bad_idat_off,
        idat,
        inflated_len,
    })
}

/// Efecto de cada bit del chunk sobre el CRC final: `CRC(dañado ^ e) ^
/// CRC(dañado)`. Es lo que permite resolver el error con álgebra en vez de
/// fuerza bruta. Caro (O(n²) hashes): solo con chunks acotados.
fn bit_effects(raw: &[u8], chunk: &Chunk, deadline: Instant) -> Result<Vec<u32>, RepairError> {
    let base = {
        let mut crc = crc_start(&chunk.typ);
        crc = crc_update(crc, &raw[chunk.data_off..chunk.data_off + chunk.data_len]);
        crc_end(crc)
    };
    let nbits = chunk.data_len * 8;
    let mut effects = Vec::with_capacity(nbits);
    let mut trial = raw[chunk.data_off..chunk.data_off + chunk.data_len].to_vec();
    for bit in 0..nbits {
        if bit % 2048 == 0 && Instant::now() > deadline {
            return Err(RepairError::BudgetExceeded);
        }
        let i = bit / 8;
        trial[i] ^= 1 << (bit % 8);
        let mut crc = crc_start(&chunk.typ);
        crc = crc_update(crc, &trial);
        effects.push(crc_end(crc) ^ base);
        trial[i] ^= 1 << (bit % 8);
    }
    Ok(effects)
}

/// Resuelve `columnas · x = rhs` sobre GF(2) (`columnas[c]` = efecto del
/// bit `c`). Devuelve máscaras de solución, como mucho `cap`; vacío si es
/// inconsistente o demasiado degenerado para enumerar.
fn solve_cols(cols: &[u32], rhs: u32, cap: usize) -> Vec<u32> {
    let n = cols.len();
    if n == 0 || n > 32 {
        return Vec::new();
    }
    let mut mat: Vec<u32> = (0..32)
        .map(|r| {
            cols.iter()
                .enumerate()
                .fold(0u32, |m, (c, col)| m | (((col >> r) & 1) << c))
        })
        .collect();
    let mut rhs_bits = rhs;
    let mut piv_col: Vec<Option<usize>> = vec![None; 32];
    let mut row = 0usize;
    for c in 0..n {
        let Some(pr) = mat
            .iter()
            .enumerate()
            .skip(row)
            .find(|(_, m)| ((*m >> c) & 1) == 1)
            .map(|(r, _)| r)
        else {
            continue;
        };
        mat.swap(row, pr);
        if ((rhs_bits >> row) & 1) != ((rhs_bits >> pr) & 1) {
            rhs_bits ^= (1 << row) | (1 << pr);
        }
        let pivot_row = mat[row];
        let pivot_rhs = (rhs_bits >> row) & 1;
        for (r, m) in mat.iter_mut().enumerate() {
            if r != row && ((*m >> c) & 1) == 1 {
                *m ^= pivot_row;
                if pivot_rhs == 1 {
                    rhs_bits ^= 1 << r;
                }
            }
        }
        piv_col[row] = Some(c);
        row += 1;
    }
    let inconsistent = mat
        .iter()
        .enumerate()
        .skip(row)
        .any(|(r, m)| *m == 0 && ((rhs_bits >> r) & 1) == 1);
    if inconsistent {
        return Vec::new();
    }
    let mut pivoted = vec![false; n];
    for pc in piv_col.iter().take(row) {
        pivoted[pc.expect("pivote registrado")] = true;
    }
    let free: Vec<usize> = (0..n).filter(|c| !pivoted[*c]).collect();
    if free.len() > 4 {
        return Vec::new();
    }
    let mut particular = 0u32;
    for (r, pc) in piv_col.iter().enumerate().take(row) {
        if (rhs_bits >> r) & 1 == 1 {
            particular |= 1 << pc.expect("pivote registrado");
        }
    }
    let mut basis = Vec::new();
    for f in &free {
        let mut v = 1u32 << f;
        for (r, m) in mat.iter().enumerate().take(row) {
            if (*m >> f) & 1 == 1 {
                v |= 1 << piv_col[r].expect("pivote registrado");
            }
        }
        basis.push(v);
    }
    let mut out = Vec::new();
    for mask in 0..(1usize << basis.len()) {
        let mut v = particular;
        for (j, b) in basis.iter().enumerate() {
            if (mask >> j) & 1 == 1 {
                v ^= b;
            }
        }
        out.push(v);
        if out.len() >= cap {
            break;
        }
    }
    out
}

/// ¿Este stream zlib infla a EXACTAMENTE los bytes esperados? `miniz_oxide`
/// verifica el Adler32 del wrapper: pasar implica el payload original, no
/// un apaño con CRC de chunk correcto pero píxeles inventados.
fn inflate_ok(idat: &[u8], expected_len: usize) -> bool {
    match miniz_oxide::inflate::decompress_to_vec_zlib(idat) {
        Ok(raw) => raw.len() == expected_len,
        Err(_) => false,
    }
}

/// Voltea los bits de `mask` (relativos a `base_bit`) en `buf` desde
/// `base_off`. XOR: llamar dos veces revierte.
fn flip_bits(buf: &mut [u8], base_off: usize, base_bit: usize, mask: u32, nvars: usize) {
    for c in 0..nvars {
        if (mask >> c) & 1 == 1 {
            let bit = base_bit + c;
            buf[base_off + bit / 8] ^= 1 << (bit % 8);
        }
    }
}

/// Intenta la reparación exacta de un PNG con checksum roto. Devuelve los
/// bytes del archivo reparado (el llamador decide dónde escribirlos, nunca
/// sobre el original). Fases de barato a caro, parando en la primera que
/// verifique: 1 bit → 1 byte → 2 bits dispersos → ráfagas de 2–3 bytes.
pub fn repair_png(path: &Path) -> Result<Vec<u8>, RepairError> {
    let raw = std::fs::read(path).map_err(|_| RepairError::Unrepairable)?;
    if raw.len() as u64 > MAX_FILE_BYTES {
        return Err(RepairError::BudgetExceeded);
    }
    let deadline = Instant::now() + TIME_BUDGET;
    let parsed = walk_single_bad_idat(&raw)?;
    if parsed.bad.data_len > MAX_BAD_CHUNK_BYTES {
        return Err(RepairError::BudgetExceeded);
    }
    let data = &raw[parsed.bad.data_off..parsed.bad.data_off + parsed.bad.data_len];
    let mut crc = crc_start(&parsed.bad.typ);
    crc = crc_update(crc, data);
    let crc_off = parsed.bad.data_off + parsed.bad.data_len;
    let stored = u32::from_be_bytes([
        raw[crc_off],
        raw[crc_off + 1],
        raw[crc_off + 2],
        raw[crc_off + 3],
    ]);
    let syndrome = crc_end(crc) ^ stored;
    let effects = bit_effects(&raw, &parsed.bad, deadline)?;
    let nbytes = parsed.bad.data_len;

    // Cada candidato se prueba sobre el stream IDAT (barato de clonar) y
    // solo el ganador se vuelca al archivo completo. Un parche es
    // (bit base, máscara, nº de bits): voltear dos veces revierte.
    type Patch = (usize, u32, usize);
    let mut work = parsed.idat.clone();
    let mut winner: Option<Vec<Patch>> = None;

    // Fase 1 bit: búsqueda directa en el mapa efecto → bit.
    let mut by_effect: HashMap<u32, usize> = HashMap::with_capacity(effects.len() * 2);
    for (bit, effect) in effects.iter().enumerate() {
        by_effect.entry(*effect).or_insert(bit);
    }
    if let Some(&bit) = by_effect.get(&syndrome) {
        flip_bits(&mut work, parsed.bad_idat_off, bit, 0b1, 1);
        if inflate_ok(&work, parsed.inflated_len) {
            winner = Some(vec![(bit, 0b1, 1)]);
        }
        flip_bits(&mut work, parsed.bad_idat_off, bit, 0b1, 1);
    }
    // Fase 1 byte: por cada posición, el valor que cuadra el CRC.
    if winner.is_none() {
        'bytes: for i in 0..nbytes {
            if i % 512 == 0 && Instant::now() > deadline {
                return Err(RepairError::BudgetExceeded);
            }
            let cols = effects[i * 8..(i + 1) * 8].to_vec();
            for mask in solve_cols(&cols, syndrome, 2) {
                if mask == 0 || mask > 0xFF {
                    continue;
                }
                flip_bits(&mut work, parsed.bad_idat_off, i * 8, mask, 8);
                if inflate_ok(&work, parsed.inflated_len) {
                    winner = Some(vec![(i * 8, mask, 8)]);
                    break 'bytes;
                }
                flip_bits(&mut work, parsed.bad_idat_off, i * 8, mask, 8);
            }
        }
    }
    // Fase 2 bits dispersos: para cada bit, el compañero que cierra el síndrome.
    if winner.is_none() {
        'pairs: for (i, effect) in effects.iter().enumerate() {
            if i % 4096 == 0 && Instant::now() > deadline {
                return Err(RepairError::BudgetExceeded);
            }
            let Some(&j) = by_effect.get(&(syndrome ^ effect)) else {
                continue;
            };
            if j <= i {
                continue;
            }
            flip_bits(&mut work, parsed.bad_idat_off, i, 0b1, 1);
            flip_bits(&mut work, parsed.bad_idat_off, j, 0b1, 1);
            if inflate_ok(&work, parsed.inflated_len) {
                winner = Some(vec![(i, 0b1, 1), (j, 0b1, 1)]);
                break 'pairs;
            }
            flip_bits(&mut work, parsed.bad_idat_off, i, 0b1, 1);
            flip_bits(&mut work, parsed.bad_idat_off, j, 0b1, 1);
        }
    }
    // Fase ráfagas de 2–3 bytes contiguos.
    if winner.is_none() {
        'bursts: for k in [2usize, 3] {
            if Instant::now() > deadline {
                return Err(RepairError::BudgetExceeded);
            }
            for i in 0..=(nbytes.saturating_sub(k)) {
                if i % 512 == 0 && Instant::now() > deadline {
                    return Err(RepairError::BudgetExceeded);
                }
                let cols = effects[i * 8..(i + k) * 8].to_vec();
                for mask in solve_cols(&cols, syndrome, 4) {
                    if mask == 0 {
                        continue;
                    }
                    flip_bits(&mut work, parsed.bad_idat_off, i * 8, mask, k * 8);
                    if inflate_ok(&work, parsed.inflated_len) {
                        winner = Some(vec![(i * 8, mask, k * 8)]);
                        break 'bursts;
                    }
                    flip_bits(&mut work, parsed.bad_idat_off, i * 8, mask, k * 8);
                }
            }
        }
    }
    // Vuelca el ganador al archivo completo (los bytes del IDAT dañado).
    match winner {
        None => Err(RepairError::Unrepairable),
        Some(patches) => {
            let mut fixed = raw.clone();
            for (base_bit, mask, nvars) in patches {
                flip_bits(&mut fixed, parsed.bad.data_off, base_bit, mask, nvars);
            }
            Ok(fixed)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;

    /// PNG 64×64 con patrón no trivial (ruido determinista) en memoria.
    fn noisy_png_bytes() -> Vec<u8> {
        let img = image::RgbaImage::from_fn(64, 64, |x, y| {
            let v = x.wrapping_mul(173) ^ y.wrapping_mul(283) ^ x.wrapping_mul(y);
            let v = v as u8;
            image::Rgba([v, v.wrapping_mul(3), v.wrapping_mul(7), 255])
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

    /// Desplaza al inicio de los datos del primer IDAT.
    fn first_idat_data_off(bytes: &[u8]) -> usize {
        let marker = bytes
            .windows(4)
            .position(|w| w == b"IDAT")
            .expect("el png trae IDAT");
        marker + 4
    }

    #[test]
    fn repairs_two_flipped_idat_bits_byte_identical() {
        // El caso 51.png: estructura completa, 2 bits mal → bytes originales.
        let dir = tempfile::tempdir().expect("tempdir");
        let original = noisy_png_bytes();
        let mut broken = original.clone();
        let off = first_idat_data_off(&original);
        broken[off] ^= 0x01;
        broken[off + 3] ^= 0x04;
        let path = write_temp(&dir, "roto.png", &broken);
        assert_eq!(
            crate::classify(&path),
            crate::CorruptionKind::ChecksumMismatch
        );
        let repaired = repair_png(&path).expect("reparar");
        assert_eq!(repaired, original);
    }

    #[test]
    fn repairs_a_whole_flipped_idat_byte() {
        let dir = tempfile::tempdir().expect("tempdir");
        let original = noisy_png_bytes();
        let mut broken = original.clone();
        let off = first_idat_data_off(&original);
        broken[off + 10] ^= 0xA5;
        let path = write_temp(&dir, "roto.png", &broken);
        let repaired = repair_png(&path).expect("reparar");
        assert_eq!(repaired, original);
    }

    #[test]
    fn repairs_a_three_bit_burst_across_two_bytes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let original = noisy_png_bytes();
        let mut broken = original.clone();
        let off = first_idat_data_off(&original);
        broken[off + 20] ^= 0x03;
        broken[off + 21] ^= 0x01;
        let path = write_temp(&dir, "roto.png", &broken);
        let repaired = repair_png(&path).expect("reparar");
        assert_eq!(repaired, original);
    }

    #[test]
    fn healthy_truncated_and_empty_are_unrepairable() {
        let dir = tempfile::tempdir().expect("tempdir");
        let original = noisy_png_bytes();
        let ok = write_temp(&dir, "sano.png", &original);
        assert_eq!(repair_png(&ok), Err(RepairError::Unrepairable));
        let cut = write_temp(&dir, "corto.png", &original[..original.len() * 6 / 10]);
        assert_eq!(repair_png(&cut), Err(RepairError::Unrepairable));
        let empty = write_temp(&dir, "vacio.png", b"");
        assert_eq!(repair_png(&empty), Err(RepairError::Unrepairable));
    }

    #[test]
    fn oversized_bad_chunk_exceeds_budget() {
        // IDAT gigante con CRC malo injertado: ni se intenta.
        let dir = tempfile::tempdir().expect("tempdir");
        let original = noisy_png_bytes();
        let iend_at = original.len() - 12;
        let mut spliced = original[..iend_at].to_vec();
        let big_len = (MAX_BAD_CHUNK_BYTES + 8192) as u32;
        spliced.extend_from_slice(&big_len.to_be_bytes());
        spliced.extend_from_slice(b"IDAT");
        spliced.extend(std::iter::repeat_n(0u8, big_len as usize));
        spliced.extend_from_slice(&0x1234_5678u32.to_be_bytes());
        spliced.extend_from_slice(&original[iend_at..]);
        let path = write_temp(&dir, "grande.png", &spliced);
        assert_eq!(repair_png(&path), Err(RepairError::BudgetExceeded));
    }

    #[test]
    fn wrong_adler_is_rejected() {
        // Propiedad de seguridad de la que depende todo el reparador: un
        // stream con Adler falso no pasa, aunque el deflate sea válido.
        let raw = vec![7u8; 5000];
        let deflate = miniz_oxide::deflate::compress_to_vec(&raw, 6);
        let mut stream = vec![0x78, 0x9C];
        stream.extend_from_slice(&deflate);
        stream.extend_from_slice(&0xDEAD_BEEFu32.to_be_bytes());
        assert!(miniz_oxide::inflate::decompress_to_vec_zlib(&stream).is_err());
    }
}
