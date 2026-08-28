//! Contenedor binario del `.canvas` (v5).
//!
//! Hasta v4 el `.canvas` era un único JSON con los píxeles de cada capa
//! embebidos como PNG en base64: un 33 % más de bytes y un coste de
//! serialización/parseo considerable en documentos con fotos grandes. Desde
//! v5 el archivo es un contenedor:
//!
//! ```text
//! "CANVAS5" (7 bytes)  ·  u64 LE json_len  ·  [JSON de cabecera]
//! (u32 LE blob_len · blob PNG)*                  ← sección binaria
//! ```
//!
//! El JSON de cabecera lleva documento, hash, miniatura (pequeña, sigue en
//! base64) y, por capa, el ÍNDICE de su blob. Los probes (versión, página,
//! miniatura) solo necesitan la cabecera, así que siguen siendo baratos.
//! Los archivos v1–v4 (JSON puro, sin mágica) se siguen leyendo sin
//! migración.

use crate::IoError;

/// Mágica del contenedor. Un `.canvas` que NO empieza así es un JSON puro
/// de v1–v4 y se lee por el camino legacy.
pub(super) const CONTAINER_MAGIC: &[u8; 7] = b"CANVAS5";

/// Tamaño del encabezado fijo: mágica (7) + json_len (8).
const HEADER_LEN: usize = CONTAINER_MAGIC.len() + std::mem::size_of::<u64>();

fn corrupt(path: &std::path::Path, message: &str) -> IoError {
    IoError::Decode {
        path: path.to_owned(),
        source: image::ImageError::IoError(std::io::Error::other(format!(
            "corrupt sidecar: {message}"
        ))),
    }
}

/// `true` si `bytes` es un contenedor v5 (empieza por la mágica).
pub(super) fn is_container(bytes: &[u8]) -> bool {
    bytes.starts_with(CONTAINER_MAGIC)
}

/// Empaqueta la cabecera JSON y los blobs en el contenedor v5.
pub(super) fn encode_container(json: &[u8], blobs: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::with_capacity(
        HEADER_LEN + json.len() + blobs.iter().map(|b| b.len() + 4).sum::<usize>(),
    );
    out.extend_from_slice(CONTAINER_MAGIC);
    out.extend_from_slice(&(json.len() as u64).to_le_bytes());
    out.extend_from_slice(json);
    for blob in blobs {
        out.extend_from_slice(&(blob.len() as u32).to_le_bytes());
        out.extend_from_slice(blob);
    }
    out
}

/// Parte un contenedor v5 en (cabecera JSON, blobs, en orden de escritura).
/// Los índices de `images[].blob` del JSON apuntan a este Vec.
pub(super) fn split_container<'a>(
    bytes: &'a [u8],
    path: &std::path::Path,
) -> Result<(&'a [u8], Vec<&'a [u8]>), IoError> {
    let rest = bytes
        .get(CONTAINER_MAGIC.len()..)
        .ok_or_else(|| corrupt(path, "truncated header"))?;
    let (len_bytes, rest) = rest
        .split_at_checked(std::mem::size_of::<u64>())
        .ok_or_else(|| corrupt(path, "truncated json length"))?;
    let json_len = u64::from_le_bytes(len_bytes.try_into().expect("8 bytes")) as usize;
    let (json, rest) = rest
        .split_at_checked(json_len)
        .ok_or_else(|| corrupt(path, "json length exceeds the file"))?;

    let mut blobs = Vec::new();
    let mut pos = 0;
    while pos < rest.len() {
        let Some(len_bytes) = rest.get(pos..pos + 4) else {
            return Err(corrupt(path, "truncated blob length"));
        };
        let blob_len = u32::from_le_bytes(len_bytes.try_into().expect("4 bytes")) as usize;
        pos += 4;
        let Some(blob) = rest.get(pos..pos + blob_len) else {
            return Err(corrupt(path, "truncated blob"));
        };
        blobs.push(blob);
        pos += blob_len;
    }
    Ok((json, blobs))
}
