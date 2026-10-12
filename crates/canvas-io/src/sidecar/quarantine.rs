//! Cuarentena propia del proyecto: aparta archivos ilegibles (vacíos,
//! truncados, con checksum roto) sin borrarlos. Vive dentro de `.canvas/`,
//! así que los escaneos de galería/baraja ya la ignoran (filtran
//! `SIDECAR_DIR` entero). Como la papelera, se conserva al navegar.
//! Ambas reservan el destino antes de mover para conservar cada copia.

use std::path::{Path, PathBuf};

use crate::IoError;

use super::paths::ensure_sidecar_dir;

/// Carpeta de cuarentena dentro de `.canvas/`.
pub fn quarantine_dir(folder: &Path) -> PathBuf {
    super::paths::sidecar_dir(folder).join("quarantine")
}

/// Mueve `path` a la cuarentena del proyecto (creándola si hace falta) y
/// devuelve dónde quedó — deshacible con `restore_from_quarantine`.
pub fn move_to_quarantine(path: &Path) -> Result<PathBuf, IoError> {
    let folder = path.parent().unwrap_or_else(|| Path::new(""));
    ensure_sidecar_dir(folder)?;
    let dir = quarantine_dir(folder);
    if let Err(source) = std::fs::create_dir(&dir) {
        if source.kind() != std::io::ErrorKind::AlreadyExists {
            return Err(IoError::Write {
                path: dir,
                message: format!("creando la cuarentena del proyecto: {source}"),
            });
        }
    }
    super::paths::move_to_free_name(&dir, path)
}

/// Deshace `move_to_quarantine`: mueve `staged` de vuelta a `original`.
/// Rechaza si `original` ya existe (alguien creó otro archivo con ese
/// nombre mientras tanto) en vez de sobrescribirlo en silencio — mismo
/// criterio que `restore_from_local_trash`.
pub fn restore_from_quarantine(staged: &Path, original: &Path) -> Result<(), IoError> {
    if original.exists() {
        return Err(IoError::Write {
            path: original.to_path_buf(),
            message: "a file already exists at the original location".to_owned(),
        });
    }
    std::fs::rename(staged, original).map_err(|source| IoError::Write {
        path: staged.to_path_buf(),
        message: format!("restaurando desde la cuarentena: {source}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiny_png(path: &Path) {
        image::RgbaImage::from_pixel(2, 2, image::Rgba([1, 2, 3, 255]))
            .save(path)
            .expect("guardar png");
    }

    #[test]
    fn quarantines_and_restores_roundtrip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("roto.png");
        tiny_png(&file);
        let staged = move_to_quarantine(&file).expect("cuarentenar");
        assert!(!file.exists());
        assert!(staged.exists());
        assert_eq!(staged.parent().expect("padre"), quarantine_dir(dir.path()));
        restore_from_quarantine(&staged, &file).expect("restaurar");
        assert!(file.exists());
        assert!(!staged.exists());
    }

    #[test]
    fn second_quarantine_of_same_name_disambiguates() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("roto.png");
        tiny_png(&file);
        let first = move_to_quarantine(&file).expect("primera");
        tiny_png(&file);
        let second = move_to_quarantine(&file).expect("segunda");
        assert_ne!(first, second);
        assert_eq!(
            second.file_name().expect("nombre").to_string_lossy(),
            "roto 2.png"
        );
    }

    #[test]
    fn restore_refuses_to_overwrite() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("roto.png");
        tiny_png(&file);
        let staged = move_to_quarantine(&file).expect("cuarentenar");
        tiny_png(&file);
        let err = restore_from_quarantine(&staged, &file).unwrap_err();
        assert!(err.to_string().contains("already exists"));
    }

    #[test]
    fn purge_leaves_quarantine_alone() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("roto.png");
        tiny_png(&file);
        let staged = move_to_quarantine(&file).expect("cuarentenar");
        super::super::trash::purge_local_trash(dir.path());
        assert!(staged.exists(), "la purga no toca la cuarentena");
    }
}
