//! Fuentes duraderas para documentos editables: `.download` pertenece al
//! proyecto y nunca al staging que limpia Download. La copia usa streaming,
//! sin cargar el MP4 en RAM; el nombre por contenido evita duplicar recortes.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::{CanvasPayload, IoError};
use canvas_core::{Document, LayerContent};

#[cfg(test)]
#[path = "video_sources_tests.rs"]
mod tests;

pub type VideoSourceLinks = Vec<(PathBuf, PathBuf)>;

pub fn preserve_video_sources(
    payload: &mut CanvasPayload,
    target: &Path,
) -> Result<VideoSourceLinks, IoError> {
    let folder = target
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let folder = std::fs::canonicalize(folder).map_err(|source| IoError::Io {
        path: folder.to_owned(),
        source,
    })?;
    let mut links: VideoSourceLinks = Vec::new();
    for page in &mut payload.document.pages {
        for layer in &mut page.layers {
            let LayerContent::Video(video) = &mut layer.content else {
                continue;
            };
            let Some(src) = video.source_path.as_ref() else {
                continue;
            };
            let dst = if let Some((_, dst)) = links.iter().find(|(from, _)| from == src) {
                dst.clone()
            } else {
                let dst = copy_source(src, &folder)?;
                links.push((src.clone(), dst.clone()));
                dst
            };
            video.source_path = Some(dst);
        }
    }
    Ok(links)
}

fn copy_source(src: &Path, folder: &Path) -> Result<PathBuf, IoError> {
    let dir = folder.join(".download");
    std::fs::create_dir_all(&dir).map_err(|source| IoError::Io {
        path: dir.clone(),
        source,
    })?;
    // Una fuente ya guardada en este proyecto es inmutable para Canvas;
    // no copiar ni hashear el MP4 entero en cada Ctrl+S.
    if let Ok(existing) = std::fs::canonicalize(src) {
        if existing.parent() == Some(dir.as_path()) {
            return Ok(existing);
        }
    }
    let mut input = std::fs::File::open(src).map_err(|source| IoError::Open {
        path: src.to_owned(),
        source,
    })?;
    let mut temp = tempfile::Builder::new()
        .prefix(".source-")
        .tempfile_in(&dir)
        .map_err(|source| IoError::Io {
            path: dir.clone(),
            source,
        })?;
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    let mut buf = [0u8; 64 * 1024];
    loop {
        let count = input.read(&mut buf).map_err(|source| IoError::Io {
            path: src.to_owned(),
            source,
        })?;
        if count == 0 {
            break;
        }
        for byte in &buf[..count] {
            hash = (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3);
        }
        temp.write_all(&buf[..count])
            .map_err(|source| IoError::Io {
                path: dir.clone(),
                source,
            })?;
    }
    let ext = src.extension().and_then(|e| e.to_str()).unwrap_or("mp4");
    let dst = dir.join(format!("{hash:016x}.{ext}"));
    // No reemplazar una fuente existente: incluso una colisión de hash se
    // comprueba byte por byte antes de reutilizarla.
    temp.as_file().sync_all().map_err(|source| IoError::Io {
        path: dst.clone(),
        source,
    })?;
    match temp.persist_noclobber(&dst) {
        Ok(file) => file.sync_all().map_err(|source| IoError::Io {
            path: dst.clone(),
            source,
        })?,
        Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {
            if !same_bytes(error.file.path(), &dst)? {
                return Err(IoError::Write {
                    path: dst,
                    message: "video source name collision; existing source was preserved".into(),
                });
            }
        }
        Err(error) => {
            return Err(IoError::Io {
                path: dst,
                source: error.error,
            })
        }
    }
    Ok(dst)
}

fn same_bytes(a: &Path, b: &Path) -> Result<bool, IoError> {
    let compare = || -> std::io::Result<bool> {
        let mut a = std::io::BufReader::new(std::fs::File::open(a)?);
        let mut b = std::io::BufReader::new(std::fs::File::open(b)?);
        if a.get_ref().metadata()?.len() != b.get_ref().metadata()?.len() {
            return Ok(false);
        }
        let mut left = [0u8; 64 * 1024];
        let mut right = [0u8; 64 * 1024];
        loop {
            let n = a.read(&mut left)?;
            b.read_exact(&mut right[..n])?;
            if left[..n] != right[..n] {
                return Ok(false);
            }
            if n == 0 {
                return Ok(true);
            }
        }
    };
    compare().map_err(|source| IoError::Io {
        path: b.to_owned(),
        source,
    })
}

/// Solo relativiza fuentes del proyecto, no rutas arbitrarias recibidas.
pub(crate) fn portable_document(doc: &Document, target: &Path) -> Document {
    let mut doc = doc.clone();
    let mut folder = target.parent().unwrap_or_else(|| Path::new("."));
    if folder.file_name().is_some_and(|name| name == ".canvas") {
        folder = folder.parent().unwrap_or_else(|| Path::new("."));
    }
    if let Ok(folder) = std::fs::canonicalize(folder) {
        for page in &mut doc.pages {
            for layer in &mut page.layers {
                if let LayerContent::Video(video) = &mut layer.content {
                    if let Some(relative) = video
                        .source_path
                        .as_deref()
                        .and_then(|src| src.strip_prefix(&folder).ok())
                    {
                        if relative.starts_with(".download") {
                            video.source_path = Some(relative.to_owned());
                        }
                    }
                }
            }
        }
    }
    doc
}

pub(crate) fn resolve_sources(doc: &mut Document, target: &Path) {
    let folder = target.parent().unwrap_or_else(|| Path::new("."));
    for page in &mut doc.pages {
        for layer in &mut page.layers {
            if let LayerContent::Video(video) = &mut layer.content {
                if let Some(src) = &video.source_path {
                    if src.is_relative() && src.starts_with(".download") {
                        video.source_path = Some(folder.join(src));
                    }
                }
            }
        }
    }
}
