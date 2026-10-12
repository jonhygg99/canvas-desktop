//! Papelera propia del proyecto (deshacer un borrado sin depender de la
//! papelera del sistema operativo).

use std::path::{Path, PathBuf};

use crate::IoError;

use super::paths::{ensure_sidecar_dir, free_name_in, numbered_name, sidecar_dir, MAX_NAME_SUFFIX};

/// Papelera propia del proyecto — deshacer un borrado ("Delete" del editor)
/// mueve el archivo aquí en vez de a la del sistema operativo: un
/// `std::fs::rename` no depende de ninguna API de plataforma (a diferencia
/// de restaurar de la papelera real, que en el crate `trash` solo funciona
/// en Windows/Linux, no en macOS). Vive dentro de `.canvas/`, que ya se
/// esconde entera — no hace falta ocultarla aparte.
pub fn trash_dir(folder: &Path) -> PathBuf {
    sidecar_dir(folder).join("trash")
}

/// Mueve `path` a la papelera del proyecto (creándola si hace falta) y
/// devuelve dónde quedó — deshacible con `restore_from_local_trash`.
/// El nombre se desambigua si ya había otro igual (borrar, restaurar y
/// volver a crear el mismo nombre; el `rename` de Windows SOBRESCRIBIRÍA el
/// destino), así que la ruta devuelta es la que hay que conservar —o buscar
/// con `find_trashed_entry`— para restaurar: derivarla de `path` apuntaría a
/// la copia vieja.
pub fn move_to_local_trash(path: &Path) -> Result<PathBuf, IoError> {
    let folder = path.parent().unwrap_or_else(|| Path::new(""));
    ensure_sidecar_dir(folder)?;
    let dir = trash_dir(folder);
    if let Err(source) = std::fs::create_dir(&dir) {
        if source.kind() != std::io::ErrorKind::AlreadyExists {
            return Err(IoError::Write {
                path: dir,
                message: format!("creando la papelera del proyecto: {source}"),
            });
        }
    }
    let staged = free_name_in(&dir, path);
    std::fs::rename(path, &staged).map_err(|source| IoError::Write {
        path: path.to_path_buf(),
        message: format!("moviendo a la papelera del proyecto: {source}"),
    })?;
    Ok(staged)
}

/// Entrada de la papelera del proyecto correspondiente a `original` (la más
/// antigua con ese nombre, desambiguada o no). `None` si no hay ninguna.
/// Necesario desde que `move_to_local_trash` desambigua: quien solo conoce
/// la ruta original ya no puede derivar la ruta papelera. Usa la MISMA regla
/// de nombres que `free_name_in` (`numbered_name`), para encontrar siempre
/// lo que aquella escribió.
///
/// Lee la carpeta UNA vez y busca en memoria: si no hay nada, el coste deja
/// de depender del tope de desambiguado (10 000 `is_file()` seguidos en una
/// carpeta de red se notan en un Ctrl+Z).
pub fn find_trashed_entry(original: &Path) -> Option<PathBuf> {
    let folder = original.parent().unwrap_or_else(|| Path::new(""));
    let dir = trash_dir(folder);
    let names: std::collections::HashSet<String> = std::fs::read_dir(&dir)
        .ok()?
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    for n in 1..=MAX_NAME_SUFFIX {
        let name = numbered_name(original, n);
        if names.contains(&name) {
            return Some(dir.join(name));
        }
    }
    None
}

/// Deshace `move_to_local_trash`: mueve `staged` de vuelta a `original`.
/// Rechaza si `original` ya existe (alguien creó otro archivo con ese mismo
/// nombre mientras tanto) en vez de sobrescribirlo en silencio — mismo
/// criterio que `rename_with_sidecar` en `canvas-app`.
pub fn restore_from_local_trash(staged: &Path, original: &Path) -> Result<(), IoError> {
    if original.exists() {
        return Err(IoError::Write {
            path: original.to_path_buf(),
            message: "a file already exists at the original location".to_owned(),
        });
    }
    std::fs::rename(staged, original).map_err(|source| IoError::Write {
        path: staged.to_path_buf(),
        message: format!("restaurando desde la papelera del proyecto: {source}"),
    })
}

/// Limpieza explícita de la papelera. La app no la ejecuta al navegar:
/// otra ventana puede conservar pasos de deshacer que aún necesitan estos archivos.
pub fn purge_local_trash(folder: &Path) {
    let dir = trash_dir(folder);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if let Err(e) = std::fs::remove_file(&path) {
            tracing::warn!("no se pudo purgar {} de la papelera: {e}", path.display());
        }
    }
}
