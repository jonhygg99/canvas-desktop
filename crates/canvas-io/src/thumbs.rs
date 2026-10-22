//! Miniaturas para la galería de carpetas, con caché en disco: la clave
//! incluye ruta, mtime y tamaño, así que se invalida sola si la imagen cambia.

use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use crate::{is_canvas_file, load_image, read_preview, IoError, LoadedImage};

/// Genera (o recupera de la caché) una miniatura RGBA cuyo lado mayor es
/// `max_dim`. Los fallos de caché nunca son fatales: se regenera.
pub fn thumbnail(
    path: &Path,
    max_dim: u32,
    cache_dir: Option<&Path>,
) -> Result<LoadedImage, IoError> {
    let cache_path = cache_dir.and_then(|dir| cache_key_path(dir, path, max_dim));
    if let Some(hit) = cache_path.as_deref().and_then(read_cache) {
        return Ok(hit);
    }

    // Un diseño lleva su miniatura embebida: este hilo (rayon) no tiene GPU
    // para hornear la página él mismo.
    let full = if is_canvas_file(path) {
        read_preview(path)?.ok_or_else(|| IoError::Decode {
            path: path.to_owned(),
            source: image::ImageError::IoError(std::io::Error::other(
                "this design has no preview yet",
            )),
        })?
    } else {
        load_image(path)?
    };
    let src = image::RgbaImage::from_raw(full.width, full.height, full.rgba).ok_or_else(|| {
        IoError::Decode {
            path: path.to_owned(),
            source: image::ImageError::Limits(image::error::LimitError::from_kind(
                image::error::LimitErrorKind::DimensionError,
            )),
        }
    })?;

    let (tw, th) = fit_within(full.width, full.height, max_dim);
    let thumb = image::imageops::thumbnail(&src, tw, th);

    if let Some(cp) = cache_path {
        // Mejor esfuerzo: una caché que no se puede escribir no es un error.
        if let Err(e) = thumb.save_with_format(&cp, image::ImageFormat::Png) {
            tracing::debug!("no se pudo cachear la miniatura de {}: {e}", path.display());
        }
    }

    Ok(LoadedImage {
        rgba: thumb.into_raw(),
        width: tw,
        height: th,
    })
}

/// Encaja `w × h` en un cuadrado de `max_dim` por el lado mayor sin deformar
/// y sin agrandar (un original más pequeño se queda como está). La comparten
/// las miniaturas de galería y las tarjetas de clip de `canvas-app`: las dos
/// piden fotogramas de forma arbitraria (9:16, 21:9…) con el mismo
/// presupuesto de memoria.
pub fn fit_within(w: u32, h: u32, max_dim: u32) -> (u32, u32) {
    let max = w.max(h).max(1);
    if max <= max_dim {
        return (w.max(1), h.max(1));
    }
    let scale = f64::from(max_dim) / f64::from(max);
    (
        ((f64::from(w) * scale).round() as u32).max(1),
        ((f64::from(h) * scale).round() as u32).max(1),
    )
}

fn cache_key_path(cache_dir: &Path, path: &Path, max_dim: u32) -> Option<PathBuf> {
    let meta = std::fs::metadata(path).ok()?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.to_string_lossy().hash(&mut hasher);
    meta.len().hash(&mut hasher);
    if let Ok(modified) = meta.modified() {
        modified.hash(&mut hasher);
    }
    max_dim.hash(&mut hasher);
    Some(cache_dir.join(format!("{:016x}.png", hasher.finish())))
}

fn read_cache(cache_path: &Path) -> Option<LoadedImage> {
    let img = image::open(cache_path).ok()?.to_rgba8();
    let (width, height) = img.dimensions();
    Some(LoadedImage {
        rgba: img.into_raw(),
        width,
        height,
    })
}

/// Tope de archivos de la caché de miniaturas en disco.
/// Tope de bytes (128 MiB de PNGs de 256 px).
pub const MAX_THUMB_FILES: usize = 1000;
pub const MAX_THUMB_BYTES: u64 = 128 * 1024 * 1024;

/// Poda la caché de miniaturas: la clave incluye ruta + mtime + tamaño,
/// así que cada versión de cada foto deja un PNG huérfano para siempre —
/// sin esto el directorio crece sin cota. Borra los más viejos por mtime
/// hasta quedar bajo `MAX_THUMB_FILES` Y `MAX_THUMB_BYTES`. Lo mejor
/// posible, sin fallar: la caché se regenera sola. La app lo llama una vez
/// al arrancar, en un hilo de fondo.
pub fn prune_thumbnail_cache(dir: &Path) {
    prune_thumbnail_cache_in(dir, MAX_THUMB_FILES, MAX_THUMB_BYTES);
}

fn prune_thumbnail_cache_in(dir: &Path, max_files: usize, max_bytes: u64) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<(std::time::SystemTime, u64, PathBuf)> = entries
        .flatten()
        .filter_map(|entry| {
            let meta = entry.metadata().ok()?;
            if !meta.is_file() {
                return None;
            }
            Some((meta.modified().ok()?, meta.len(), entry.path()))
        })
        .collect();
    // Más viejos primero: ante el empate de mtime, los más grandes antes
    // (liberan más por borrado).
    files.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
    let mut total: u64 = files.iter().map(|(_, len, _)| len).sum();
    let mut count = files.len();
    for (_, len, path) in files {
        if count <= max_files && total <= max_bytes {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            count -= 1;
            total = total.saturating_sub(len);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prune_evicts_oldest_first_under_both_caps() {
        let dir = tempfile::tempdir().expect("tempdir");
        for (name, size) in [("a.png", 8usize), ("b.png", 8), ("c.png", 8)] {
            std::fs::write(dir.path().join(name), vec![7u8; size]).expect("sembrar");
        }
        std::fs::create_dir(dir.path().join("sub")).expect("subdir");
        // Tope de bytes: 24 > 20, sobra un archivo (el empate de mtime no
        // importa: la aserción es de conteo y de total).
        prune_thumbnail_cache_in(dir.path(), 10, 20);
        let files: Vec<_> = std::fs::read_dir(dir.path())
            .expect("leer")
            .flatten()
            .filter(|e| e.metadata().is_ok_and(|m| m.is_file()))
            .collect();
        assert_eq!(files.len(), 2);
        let total: u64 = files
            .iter()
            .filter_map(|e| e.metadata().ok().map(|m| m.len()))
            .sum();
        assert!(total <= 20);
        // Tope de archivos: queda uno; el subdirectorio no cuenta ni se toca.
        prune_thumbnail_cache_in(dir.path(), 1, u64::MAX);
        let files = std::fs::read_dir(dir.path()).expect("leer").count();
        assert_eq!(files, 2, "un PNG + el subdirectorio intacto");
        assert!(dir.path().join("sub").is_dir());
    }

    #[test]
    fn fits_within_max_dimension() {
        assert_eq!(fit_within(1600, 1000, 256), (256, 160));
        assert_eq!(fit_within(1000, 1600, 256), (160, 256));
        assert_eq!(fit_within(100, 50, 256), (100, 50), "no agranda");
    }

    #[test]
    fn generates_and_caches_thumbnail() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cache = dir.path().join("cache");
        std::fs::create_dir_all(&cache).expect("crear caché");
        let img_path = dir.path().join("grande.png");
        image::RgbaImage::from_pixel(800, 400, image::Rgba([10, 200, 30, 255]))
            .save(&img_path)
            .expect("guardar");

        let t = thumbnail(&img_path, 256, Some(&cache)).expect("miniatura");
        assert_eq!((t.width, t.height), (256, 128));
        assert_eq!(t.rgba[0..4], [10, 200, 30, 255]);

        // Segunda llamada: sale de la caché (hay exactamente un archivo).
        let cached_files = std::fs::read_dir(&cache).expect("leer").count();
        assert_eq!(cached_files, 1);
        let t2 = thumbnail(&img_path, 256, Some(&cache)).expect("miniatura cacheada");
        assert_eq!((t2.width, t2.height), (256, 128));
    }

    #[test]
    fn thumbnail_reads_the_embedded_preview() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cache = dir.path().join("cache");
        std::fs::create_dir_all(&cache).expect("crear caché");
        let path = dir.path().join("Untitled.canvas");

        let (w, h) = (8u32, 4u32);
        let mut preview_rgba = Vec::with_capacity((w * h * 4) as usize);
        for _ in 0..(w * h) {
            preview_rgba.extend_from_slice(&[10, 200, 30, 255]);
        }
        let payload = crate::CanvasPayload {
            document: canvas_core::Document::new(800.0, 400.0),
            images: Vec::new(),
            background_layer: None,
            preview: Some(LoadedImage {
                rgba: preview_rgba.clone(),
                width: w,
                height: h,
            }),
        };
        crate::write_design(&path, &payload).expect("escribir diseño");

        // Este hilo (rayon) no tiene GPU: debe pintar la miniatura embebida.
        let t = thumbnail(&path, 256, Some(&cache)).expect("miniatura");
        assert_eq!((t.width, t.height), (w, h));
        assert_eq!(t.rgba[0..4], [10, 200, 30, 255]);

        let cached_files = std::fs::read_dir(&cache).expect("leer").count();
        assert_eq!(cached_files, 1);
        let t2 = thumbnail(&path, 256, Some(&cache)).expect("miniatura cacheada");
        assert_eq!((t2.width, t2.height), (w, h));
    }
}
