//! Papelera propia del proyecto (deshacer un borrado sin depender de la
//! papelera del sistema operativo).

use std::path::{Path, PathBuf};

use crate::IoError;

use super::paths::{ensure_sidecar_dir, sidecar_dir};

/// Papelera propia del proyecto — deshacer un borrado ("Delete" del editor)
/// mueve el archivo aquí en vez de a la del sistema operativo: un
/// `std::fs::rename` no depende de ninguna API de plataforma (a diferencia
/// de restaurar de la papelera real, que en el crate `trash` solo funciona
/// en Windows/Linux, no en macOS). Vive dentro de `.canvas/`, que ya se
/// esconde entera — no hace falta ocultarla aparte.
pub fn trash_dir(folder: &Path) -> PathBuf {
    sidecar_dir(folder).join("trash")
}

/// Ruta que tendría `original` si estuviera en la papelera del proyecto:
/// mismo nombre de archivo, sin desambiguar — un archivo no puede estar
/// borrado dos veces a la vez en la misma carpeta (la segunda vez ya lo
/// habría movido o purgado la primera), así que no hace falta un sufijo.
pub fn local_trash_path(original: &Path) -> PathBuf {
    let folder = original.parent().unwrap_or_else(|| Path::new(""));
    let name = original.file_name().unwrap_or_default();
    trash_dir(folder).join(name)
}

/// Mueve `path` a la papelera del proyecto (creándola si hace falta) y
/// devuelve dónde quedó — deshacible con `restore_from_local_trash`.
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
    let staged = local_trash_path(path);
    std::fs::rename(path, &staged).map_err(|source| IoError::Write {
        path: path.to_path_buf(),
        message: format!("moviendo a la papelera del proyecto: {source}"),
    })?;
    Ok(staged)
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

/// Borra de verdad (mejor esfuerzo, sin fallar si `trash/` no existe o algún
/// archivo ya no está) todo lo que quedó en la papelera del proyecto sin
/// deshacer — se llama al salir de la carpeta (galería/proyecto) y al
/// volver a entrar en ella, para que un `Ctrl+Z` que nunca llegó, o los
/// restos de una sesión que se cerró en falso, no se queden para siempre.
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
