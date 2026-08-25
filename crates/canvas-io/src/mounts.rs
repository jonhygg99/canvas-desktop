//! Lecturas de directorio resilientes para montajes FUSE/File Provider.
//!
//! `~/Library/CloudStorage` (Google Drive, Dropbox, iCloud, OneDrive…) es un
//! montaje virtual: `opendir` puede fallar con `EPERM`/`EIO` TRANSITORIOS
//! mientras el proveedor hidrata contenido solo-en-línea (típico en la
//! sección «Otros ordenadores» de Google Drive) o mientras el demonio de
//! sincronización está ocupado. Sin reintentos, eso se ve como una galería
//! «vacía» o un error seco aunque la carpeta exista y sea accesible.

use std::path::Path;
use std::time::Duration;

/// Esperas entre reintentos de `read_dir` (ms). Cortas a propósito: si el
/// proveedor hidrata la entrada, responde en decenas de ms; si el fallo es
/// real (permisos, sin red), no queremos colgar la UI más de ~0.6 s.
const RETRY_DELAYS_MS: [u64; 2] = [150, 450];

/// ¿Este error de `read_dir` merece reintento? Los montajes FUSE fallan con
/// errores crudos que `ErrorKind` no cubre (EIO, EBUSY), así que se mira el
/// número de error además del `kind`.
fn is_transient_mount_error(err: &std::io::Error) -> bool {
    match err.kind() {
        std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::Interrupted => true,
        // EIO (5) y EBUSY (16) en macOS/Linux; en Windows los crudos de FUSE
        // también suelen caer aquí.
        _ => matches!(err.raw_os_error(), Some(5) | Some(16)),
    }
}

/// `read_dir` con hasta dos reintentos cortos sobre errores transitorios de
/// montajes en la nube. Un fallo REAL (ruta inexistente, sin permisos del
/// usuario) devuelve el error inmediatamente: no hay nada que esperar.
pub fn read_dir_resilient(folder: &Path) -> std::io::Result<std::fs::ReadDir> {
    let mut attempt = 0usize;
    loop {
        match std::fs::read_dir(folder) {
            Ok(entries) => return Ok(entries),
            Err(err) => {
                if attempt >= RETRY_DELAYS_MS.len() || !is_transient_mount_error(&err) {
                    return Err(err);
                }
                std::thread::sleep(Duration::from_millis(RETRY_DELAYS_MS[attempt]));
                attempt += 1;
            }
        }
    }
}

/// ¿La ruta vive dentro de un montaje de almacenamiento en la nube de macOS
/// (`~/Library/CloudStorage/<Proveedor>/…`)?
pub fn is_cloud_storage_path(folder: &Path) -> bool {
    folder.components().any(|c| c.as_os_str() == "CloudStorage")
}

/// Pista accionable para montajes de nube: el fallo casi nunca es de la app,
/// sino del estado del proveedor (contenido solo-en-línea, app de
/// sincronización parada, o permiso de macOS sin conceder).
pub const CLOUD_STORAGE_HINT: &str = "This folder is inside a cloud-storage mount \
     (Google Drive, Dropbox, iCloud, OneDrive…). Open it once in Finder, make it \
     \u{201c}available offline\u{201d} in the sync app, and make sure that app is \
     running. If it still fails, grant Full Disk Access to the terminal/app that \
     launches Canvas Desktop in System Settings \u{203a} Privacy & Security.";

/// Mensaje de error completo para la UI: ruta + causa, con la pista de nube
/// cuando la carpeta vive en un montaje de CloudStorage.
pub fn describe_read_dir_error(folder: &Path, error: &std::io::Error) -> String {
    let base = format!("{}: {error}", folder.display());
    if is_cloud_storage_path(folder) {
        format!("{base}\n{CLOUD_STORAGE_HINT}")
    } else {
        base
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn detects_cloud_storage_mounts() {
        let drive = PathBuf::from(
            "/Users/x/Library/CloudStorage/GoogleDrive-a@b.com/Otros ordenadores/My Computer",
        );
        assert!(is_cloud_storage_path(&drive));
        assert!(!is_cloud_storage_path(&PathBuf::from("/Users/x/Material")));
    }

    #[test]
    fn describes_read_dir_errors_with_a_cloud_hint_when_it_applies() {
        let drive =
            PathBuf::from("/Users/x/Library/CloudStorage/GoogleDrive-a@b.com/Otros ordenadores");
        let error = std::io::Error::from_raw_os_error(1);
        let message = describe_read_dir_error(&drive, &error);
        // El texto del error crudo varía por SO (Unix: «Operation not
        // permitted»; Windows: otro mensaje para el código 1): lo estable es
        // el «os error N».
        assert!(message.contains("os error 1"));
        assert!(message.contains("cloud-storage mount"));

        let local = PathBuf::from("/Users/x/Material");
        let message = describe_read_dir_error(&local, &error);
        assert!(message.contains("os error 1"));
        assert!(!message.contains("cloud-storage mount"));
    }

    #[test]
    fn resilient_read_dir_fails_fast_on_a_missing_folder() {
        // Ruta inexistente: ENOENT no es transitorio, no debe reintentar.
        let missing = std::env::temp_dir().join("canvas-desktop-no-such-dir-42");
        let result = read_dir_resilient(&missing);
        assert!(result.is_err());
    }

    #[test]
    fn resilient_read_dir_lists_a_normal_folder() {
        let dir = std::env::temp_dir();
        let entries = read_dir_resilient(&dir).expect("temp dir is readable");
        assert!(entries.count() > 0);
    }
}
