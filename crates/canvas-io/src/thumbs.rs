//! Miniaturas para la galería de carpetas, con caché en disco: la clave
//! incluye ruta, mtime y tamaño, así que se invalida sola si la imagen cambia.

use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use crate::{load_image, IoError, LoadedImage};

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

    let full = load_image(path)?;
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

fn fit_within(w: u32, h: u32, max_dim: u32) -> (u32, u32) {
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
