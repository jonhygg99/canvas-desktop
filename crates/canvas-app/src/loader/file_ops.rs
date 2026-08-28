//! Operaciones de disco con conciencia de sidecar: copiar/duplicar con
//! su `.canvas`, renombrar sin sobrescribir, papelera del sistema y
//! papelera propia del proyecto (con restauración). Puros: sin hilos, sin
//! canal — los orquesta `gallery_ops.rs`.

use std::path::{Path, PathBuf};

use canvas_io::IoError;

/// Nombre de archivo sin su última extensión, y esa extensión (ambos vacíos
/// si `path` no los tiene).
fn split_name(path: &Path) -> (String, String) {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_default();
    (stem, ext)
}

/// Copia `src` (y su sidecar, si es una imagen que tiene uno) a una ruta
/// libre en `folder`. Con `force_copy_suffix`, el nombre siempre lleva
/// « copy» (duplicar en el sitio); si no, se intenta primero el mismo
/// nombre y solo se numera si colisiona (pegar en otra carpeta). Revierte
/// la primera copia si la del sidecar falla, para no dejar un duplicado a
/// medias.
pub(super) fn duplicate_into(
    src: &Path,
    folder: &Path,
    force_copy_suffix: bool,
) -> Result<PathBuf, IoError> {
    let (stem, ext) = split_name(src);
    let base = if force_copy_suffix {
        format!("{stem} copy")
    } else {
        stem
    };
    let dst = canvas_io::reserve_unique_path(folder, &base, &ext)?;
    if let Err(source) = std::fs::copy(src, &dst) {
        let _ = std::fs::remove_file(&dst);
        return Err(IoError::Io {
            path: dst.clone(),
            source,
        });
    }
    if canvas_io::is_image_file(src) {
        if let Some(src_sidecar) = canvas_io::find_sidecar(src) {
            // El destino de sidecar cae en la carpeta oculta de `folder`
            // (que puede no ser la de `src` — copiar entre carpetas): hay
            // que asegurarla antes de copiar, no solo antes de reservar `dst`.
            if let Err(e) = canvas_io::ensure_sidecar_dir(folder) {
                let _ = std::fs::remove_file(&dst);
                return Err(e);
            }
            let dst_sidecar = canvas_io::sidecar_path(&dst);
            if let Err(source) = std::fs::copy(&src_sidecar, &dst_sidecar) {
                let _ = std::fs::remove_file(&dst);
                return Err(IoError::Io {
                    path: dst.clone(),
                    source,
                });
            }
        }
    }
    Ok(dst)
}

/// Cambia el nombre base de `path` a `new_stem`, conservando su extensión
/// original. Si el destino ya existe se rechaza en vez de sobrescribir:
/// `std::fs::rename` en Windows usa `MOVEFILE_REPLACE_EXISTING`, así que sin
/// este chequeo previo un nombre repetido perdería el archivo que ya
/// hubiera ahí en silencio. Si `path` es una imagen con sidecar, lo
/// renombra también (mejor esfuerzo: un fallo ahí no deshace el renombrado
/// principal, solo se registra).
pub(super) fn rename_with_sidecar(path: &Path, new_stem: &str) -> Result<PathBuf, IoError> {
    let folder = path.parent().map(PathBuf::from).unwrap_or_default();
    let (_, ext) = split_name(path);
    let new_name = if ext.is_empty() {
        new_stem.to_owned()
    } else {
        format!("{new_stem}.{ext}")
    };
    let dst = folder.join(&new_name);
    if dst.exists() {
        return Err(IoError::Message {
            message: format!("\"{new_name}\" already exists"),
        });
    }
    std::fs::rename(path, &dst).map_err(|source| IoError::Io {
        path: path.to_owned(),
        source,
    })?;
    if canvas_io::is_image_file(path) {
        if let Some(src_sidecar) = canvas_io::find_sidecar(path) {
            if let Err(e) = canvas_io::ensure_sidecar_dir(&folder) {
                tracing::warn!("no se pudo preparar la carpeta de sidecars: {e}");
            } else {
                let dst_sidecar = canvas_io::sidecar_path(&dst);
                if let Err(e) = std::fs::rename(&src_sidecar, &dst_sidecar) {
                    tracing::warn!("no se pudo renombrar el sidecar: {e}");
                }
            }
        }
    }
    Ok(dst)
}

/// Envía `path` (y su sidecar, si es una imagen que tiene uno) a la
/// Papelera de reciclaje del sistema: recuperable si el usuario se
/// equivoca, a diferencia de un borrado permanente. Usado por el borrado
/// desde la GALERÍA (`GalleryOp::Delete`) — ese no tiene deshacer, así que
/// sí le compensa depender de la papelera real del sistema.
pub(super) fn trash_with_sidecar(path: &Path) -> Result<(), IoError> {
    trash::delete(path).map_err(|e| IoError::Message {
        message: e.to_string(),
    })?;
    if canvas_io::is_image_file(path) {
        if let Some(sidecar) = canvas_io::find_sidecar(path) {
            if let Err(e) = trash::delete(&sidecar) {
                tracing::warn!("no se pudo borrar el sidecar: {e}");
            }
        }
    }
    Ok(())
}

/// Mueve `path` (y su sidecar, si es una imagen que tiene uno) a la
/// papelera PROPIA del proyecto (`canvas_io::move_to_local_trash`), no la
/// del sistema — este borrado sí tiene deshacer (botón «Delete» del
/// editor/cabecera de un lienzo), y restaurar con un `rename` no depende de
/// ninguna API de plataforma (a diferencia de `trash::os_limited`, que en
/// macOS no existe).
pub(super) fn trash_locally_with_sidecar(path: &Path) -> Result<(), IoError> {
    canvas_io::move_to_local_trash(path)?;
    if canvas_io::is_image_file(path) {
        if let Some(sidecar) = canvas_io::find_sidecar(path) {
            if let Err(e) = canvas_io::move_to_local_trash(&sidecar) {
                tracing::warn!("no se pudo mover el sidecar a la papelera: {e}");
            }
        }
    }
    Ok(())
}

pub(super) fn restore_one(original: &Path) -> Result<(), IoError> {
    let staged = canvas_io::local_trash_path(original);
    canvas_io::restore_from_local_trash(&staged, original)
}
