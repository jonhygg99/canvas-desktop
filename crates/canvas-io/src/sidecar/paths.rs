//! Rutas del sidecar de una imagen y de la carpeta oculta que las contiene.

use std::path::{Path, PathBuf};

use crate::IoError;

/// Nombre de la carpeta oculta donde viven los sidecar de una carpeta de
/// imágenes.
pub const SIDECAR_DIR: &str = ".canvas";

/// Carpeta de sidecars de `folder` (no crea nada: solo compone la ruta).
pub fn sidecar_dir(folder: &Path) -> PathBuf {
    folder.join(SIDECAR_DIR)
}

/// Ruta ACTUAL del sidecar de una imagen — dentro de `.canvas/`, no como
/// hermano directo. Es la única ruta que usan los llamadores que van a
/// ESCRIBIR (`write_sidecar`, y quien vaya a copiar/renombrar un sidecar ya
/// migrado); para LEER, usar `find_sidecar`, que también contempla el
/// hermano legacy.
pub fn sidecar_path(image_path: &Path) -> PathBuf {
    let folder = image_path.parent().unwrap_or_else(|| Path::new(""));
    sidecar_dir(folder).join(sidecar_file_name(image_path))
}

fn sidecar_file_name(image_path: &Path) -> String {
    let mut name = image_path
        .file_name()
        .map(|n| n.to_owned())
        .unwrap_or_default();
    name.push(".");
    name.push(crate::CANVAS_EXTENSION);
    name.to_string_lossy().into_owned()
}

/// Sidecar existente de `image_path`, si lo hay: primero la ubicación actual
/// (`.canvas/foto.png.canvas`), luego el hermano legacy
/// (`foto.png.canvas`) para no perder de vista lo que una versión anterior ya
/// había escrito. `None` si ninguno de los dos existe.
pub fn find_sidecar(image_path: &Path) -> Option<PathBuf> {
    let current = sidecar_path(image_path);
    if current.is_file() {
        return Some(current);
    }
    let legacy = super::legacy_sidecar_path(image_path);
    legacy.is_file().then_some(legacy)
}

/// Nombre desambiguado número `n` de `original`: `nombre 2.ext`,
/// `nombre 3.ext`… (con `n == 1` devuelve el nombre tal cual). Dueña ÚNICA
/// de la regla de nombres: la usan `free_name_in` (elegir un destino libre)
/// y `find_trashed_entry` (buscar la copia trasteada), que deben generar
/// siempre los mismos nombres.
pub fn numbered_name(original: &Path, n: u32) -> String {
    let name = original
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".to_owned());
    if n <= 1 {
        return name;
    }
    let stem = original
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or(name);
    let ext = original
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    format!("{stem} {n}{ext}")
}

/// Tope del desambiguado: con 10 000 copias del mismo nombre en una carpeta
/// de proyecto ya hay un problema mayor que el nombre.
pub const MAX_NAME_SUFFIX: u32 = 10_000;

/// Nombre libre dentro de `dir` para `original`: el mismo nombre si está
/// libre, y si no `nombre 2.ext`, `nombre 3.ext`… (los `rename` de Windows
/// SOBRESCRIBEN el destino, así que «mover» sin desambiguar destruiría lo
/// que ya hubiera ahí). Compartido por la cuarentena y la papelera del
/// proyecto: ambas apartan archivos con nombres que el usuario puede volver
/// a crear.
pub fn free_name_in(dir: &Path, original: &Path) -> PathBuf {
    for n in 1..=MAX_NAME_SUFFIX {
        let candidate = dir.join(numbered_name(original, n));
        if !candidate.exists() {
            return candidate;
        }
    }
    dir.join(numbered_name(original, MAX_NAME_SUFFIX))
}

/// Crea `<folder>/.canvas` si no existe y, en Windows, la marca oculta con
/// `FILE_ATTRIBUTE_HIDDEN` — el prefijo `.` del nombre no oculta nada ahí, a
/// diferencia de Unix. Solo marca el atributo justo cuando ACABA de crear la
/// carpeta: si el usuario la hizo visible a mano después, esta función no se
/// lo revierte en cada guardado.
pub fn ensure_sidecar_dir(folder: &Path) -> Result<PathBuf, IoError> {
    let dir = sidecar_dir(folder);
    match std::fs::create_dir(&dir) {
        Ok(()) => {
            hide_dir(&dir);
            Ok(dir)
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(dir),
        Err(source) => Err(IoError::Write {
            path: dir,
            message: format!("creando la carpeta de diseños: {source}"),
        }),
    }
}

/// Mejor esfuerzo: no ocultar la carpeta nunca es motivo para fallar el
/// guardado, solo un fastidio visual.
#[cfg(windows)]
fn hide_dir(dir: &Path) {
    use windows::core::HSTRING;
    use windows::Win32::Storage::FileSystem::{SetFileAttributesW, FILE_ATTRIBUTE_HIDDEN};
    let result =
        unsafe { SetFileAttributesW(&HSTRING::from(dir.as_os_str()), FILE_ATTRIBUTE_HIDDEN) };
    if let Err(e) = result {
        tracing::warn!("no se pudo ocultar {}: {e}", dir.display());
    }
}

#[cfg(not(windows))]
fn hide_dir(_dir: &Path) {
    // El prefijo `.` del nombre ya la oculta por convención Unix.
}
