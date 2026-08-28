//! Tests del descubrimiento de archivos de galería y del backoff de
//! refresco de carpetas. Hermano de `gallery_ops.rs` (convención
//! `*_tests.rs` cableada con `#[path]`).

use super::discover_gallery_files;
use std::fs;
use tempfile::tempdir;

#[test]
fn discovers_images_designs_and_ignores_hidden_entries() {
    let dir = tempdir().expect("tempdir");
    fs::write(dir.path().join("photo.png"), b"not decoded here").unwrap();
    fs::write(dir.path().join("design.canvas"), b"standalone design").unwrap();
    fs::write(dir.path().join(".hidden.png"), b"hidden").unwrap();
    // En Windows un archivo «oculto» es el que lleva el atributo, no el
    // que empieza por punto (ver `canvas_shell::is_hidden`): marcar el
    // atributo para que el test signifique lo mismo en todas las
    // plataformas (mismo patrón que `canvas-shell/tests/integration.rs`).
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("attrib")
            .arg("+h")
            .arg(dir.path().join(".hidden.png"))
            .status();
    }
    fs::create_dir(dir.path().join("subfolder")).unwrap();
    fs::create_dir(dir.path().join(canvas_io::SIDECAR_DIR)).unwrap();

    let files = discover_gallery_files(dir.path()).unwrap();
    let names: Vec<_> = files
        .iter()
        .map(|(path, _)| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["design.canvas", "photo.png"]);
}

#[test]
fn folder_auto_refresh_backoff_doubles_and_caps() {
    use std::time::Duration;
    assert_eq!(super::folder_refresh_delay(0), Duration::from_secs(1));
    assert_eq!(super::folder_refresh_delay(1), Duration::from_secs(2));
    assert_eq!(super::folder_refresh_delay(2), Duration::from_secs(4));
    assert_eq!(super::folder_refresh_delay(9), Duration::from_secs(4));
    // Guarda en tiempo de compilación: la tabla de backoff de arriba
    // asume al menos estos intentos de refresco automático.
    const _: () = assert!(super::FOLDER_REFRESH_ATTEMPTS >= 2);
}

#[test]
fn reports_unreadable_or_missing_gallery_folder() {
    let dir = tempdir().expect("tempdir");
    let missing = dir.path().join("does-not-exist");
    let error = discover_gallery_files(&missing).unwrap_err();
    assert!(error.contains("does-not-exist"));
}
