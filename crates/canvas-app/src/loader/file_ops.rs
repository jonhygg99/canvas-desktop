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

/// Deja el sidecar de `src` junto al archivo renombrado `dst`: asegura la
/// carpeta `.canvas` de `folder` (un sidecar hermano legacy necesita que
/// exista) y renombra dentro de ella. Error tipado: el llamador revierte el
/// rename de la imagen con este error, así que tiene que sobrevivir al viaje
/// hasta el banner sin pasar por `String`.
fn move_sidecar_next_to(folder: &Path, src: &Path, dst: &Path) -> Result<(), IoError> {
    canvas_io::ensure_sidecar_dir(folder)?;
    let dst_sidecar = canvas_io::sidecar_path(dst);
    std::fs::rename(src, &dst_sidecar).map_err(|source| IoError::Write {
        path: dst_sidecar,
        message: format!("moviendo el sidecar de capas editables: {source}"),
    })
}

/// Cambia el nombre base de `path` a `new_stem`, conservando su extensión
/// original. Si el destino ya existe se rechaza en vez de sobrescribir:
/// `std::fs::rename` en Windows usa `MOVEFILE_REPLACE_EXISTING`, así que sin
/// este chequeo previo un nombre repetido perdería el archivo que ya
/// hubiera ahí en silencio.
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
            // Las capas editables tienen que acabar junto al archivo nuevo.
            // Cualquier fallo en el camino — crear `.canvas` o renombrar el
            // sidecar — deja la imagen ya renombrada sin ellas, así que los
            // dos pasos comparten el mismo deshacer.
            if let Err(e) = move_sidecar_next_to(&folder, &src_sidecar, &dst) {
                // Rollback: devolver la imagen a su nombre original. Si
                // tampoco se puede, informar al usuario en vez de callar.
                let rollback = std::fs::rename(&dst, path);
                return Err(match rollback {
                    Ok(()) => IoError::Message {
                        message: format!(
                            "\"{}\" was renamed but its editable layers could not be moved: {e}. The file keeps its original name.",
                            path.display()
                        ),
                    },
                    Err(back) => IoError::Message {
                        message: format!(
                            "\"{}\" is now \"{}\" but its editable layers are still in the old sidecar \"{}\" and could not be moved ({e}), and the rename could not be undone ({back}).",
                            path.display(),
                            dst.display(),
                            src_sidecar.display()
                        ),
                    },
                });
            }
        }
    }
    Ok(dst)
}

/// Resultado de una operación sobre la pareja imagen + sidecar: lo que pasó
/// con el archivo principal y, si la parte editable falló SIN poder deshacer
/// la principal, el texto que hay que enseñar al usuario.
///
/// Existe porque esas operaciones son a medias por naturaleza: la imagen ya
/// se ha movido cuando el sidecar falla, así que no se puede devolver error
/// sin mentir (la operación principal SÍ ocurrió) ni `Ok` sin más (el usuario
/// creería que se llevó también las capas editables). Antes el fallo del
/// sidecar solo se registraba en el log.
#[derive(Debug)]
pub(super) struct PairOutcome<T = ()> {
    pub result: Result<T, IoError>,
    pub warning: Option<String>,
}

impl PairOutcome {
    /// Operación principal completada, sin nada que avisar.
    fn ok() -> Self {
        Self {
            result: Ok(()),
            warning: None,
        }
    }

    /// Operación principal completada, con un aviso sobre el sidecar.
    fn warned(message: String) -> Self {
        Self {
            result: Ok(()),
            warning: Some(message),
        }
    }
}

/// Envía `path` (y su sidecar, si es una imagen que tiene uno) a la
/// Papelera de reciclaje del sistema: recuperable si el usuario se
/// equivoca, a diferencia de un borrado permanente. Usado por el borrado
/// desde la GALERÍA (`GalleryOp::Delete`) — ese no tiene deshacer, así que
/// sí le compensa depender de la papelera real del sistema.
pub(super) fn trash_with_sidecar(path: &Path) -> PairOutcome {
    if let Err(e) = trash::delete(path) {
        return PairOutcome {
            result: Err(IoError::Message {
                message: e.to_string(),
            }),
            warning: None,
        };
    }
    let Some(sidecar) = sidecar_of(path) else {
        return PairOutcome::ok();
    };
    match trash::delete(&sidecar) {
        Ok(()) => PairOutcome::ok(),
        Err(e) => PairOutcome::warned(format!(
            "\"{}\" went to the Recycle Bin, but its editable layers could not be removed from the design folder ({e}). They are still there if you restore the file.",
            path.display()
        )),
    }
}

/// Mueve `path` (y su sidecar, si es una imagen que tiene uno) a la
/// papelera PROPIA del proyecto (`canvas_io::move_to_local_trash`), no la
/// del sistema — este borrado sí tiene deshacer (botón «Delete» del
/// editor/cabecera de un lienzo), y restaurar con un `rename` no depende de
/// ninguna API de plataforma (a diferencia de `trash::os_limited`, que en
/// macOS no existe).
pub(super) fn trash_locally_with_sidecar(path: &Path) -> PairOutcome<crate::editor::DeleteRecord> {
    let staged = match canvas_io::move_to_local_trash(path) {
        Ok(staged) => staged,
        Err(e) => {
            return PairOutcome {
                result: Err(e),
                warning: None,
            }
        }
    };
    let mut record = crate::editor::DeleteRecord {
        path: path.to_owned(),
        staged,
        sidecar: None,
    };
    let mut warning = None;
    if let Some(sidecar) = sidecar_of(path) {
        match canvas_io::move_to_local_trash(&sidecar) {
            Ok(staged) => record.sidecar = Some((sidecar, staged)),
            Err(e) => warning = Some(format!(
                "\"{}\" went to the project trash, but its editable layers could not be moved with it ({e}). Undo will bring back the file only.", path.display()
            )),
        }
    }
    PairOutcome {
        result: Ok(record),
        warning,
    }
}

/// Restaura la entrada exacta registrada por el borrado, sin buscar por nombre.
pub(super) fn restore_one(original: &Path, staged: &Path) -> PairOutcome {
    PairOutcome {
        result: canvas_io::restore_from_local_trash(staged, original),
        warning: None,
    }
}

/// Sidecar de `path` si es una imagen que lo tiene.
fn sidecar_of(path: &Path) -> Option<PathBuf> {
    canvas_io::is_image_file(path)
        .then(|| canvas_io::find_sidecar(path))
        .flatten()
}

#[cfg(test)]
#[path = "file_ops_tests.rs"]
mod file_ops_tests;
