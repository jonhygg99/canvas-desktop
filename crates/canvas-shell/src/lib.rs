//! Integración con el sistema operativo. Todo el código específico de
//! plataforma vive aquí, detrás de `#[cfg(target_os = ...)]`.
//!
//! Normaliza las distintas vías de apertura (argv en frío, segunda instancia,
//! `openURLs` de macOS) en un único evento interno [`ShellEvent::OpenPath`].

mod integration;
mod single_instance;

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "macos")]
mod macos;

#[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
mod linux;

pub use integration::{platform, ShellError, ShellIntegration};
pub use single_instance::{acquire_instance, InstanceListener, InstanceRole};

/// Identidad de la app ante la barra de tareas (AppUserModelID en Windows).
/// Llamar lo antes posible en `main`, antes de crear la ventana. No-op fuera
/// de Windows.
pub fn set_app_identity() {
    #[cfg(target_os = "windows")]
    windows::set_app_user_model_id();
}

use std::path::PathBuf;

/// Evento normalizado que el resto de la app consume sin saber de qué
/// plataforma ni de qué vía (argv, segunda instancia, openURLs) procede.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellEvent {
    OpenPath(PathBuf),
}

/// Extrae rutas abribles de los argumentos de línea de comandos (arranque en
/// frío). Filtra flags (todo lo que empiece por `-`, p. ej. lo que cuela cargo
/// en desarrollo) y descarta lo que no exista en disco.
pub fn open_paths_from_args<I>(args: I) -> Vec<PathBuf>
where
    I: IntoIterator<Item = String>,
{
    args.into_iter()
        .skip(1) // argv[0] es el ejecutable
        .filter(|a| !a.starts_with('-'))
        .map(PathBuf::from)
        .filter(|p| p.exists())
        .collect()
}

/// ¿El archivo está oculto según la convención de la plataforma? En Windows,
/// el atributo `FILE_ATTRIBUTE_HIDDEN`; en Unix, el prefijo `.` del nombre.
#[cfg(windows)]
pub fn is_hidden(path: &std::path::Path) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
    std::fs::metadata(path)
        .map(|m| m.file_attributes() & FILE_ATTRIBUTE_HIDDEN != 0)
        .unwrap_or(false)
}

#[cfg(not(windows))]
pub fn is_hidden(path: &std::path::Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with('.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_flags_and_missing_paths() {
        let existing = std::env::temp_dir();
        let args = vec![
            "canvas-desktop.exe".to_owned(),
            "--release".to_owned(),
            "-v".to_owned(),
            existing.to_string_lossy().into_owned(),
            "Z:/no/existe/imagen.png".to_owned(),
        ];
        let paths = open_paths_from_args(args);
        assert_eq!(paths, vec![existing]);
    }

    #[test]
    fn skips_argv0_even_if_it_exists() {
        let exe = std::env::temp_dir();
        let paths = open_paths_from_args(vec![exe.to_string_lossy().into_owned()]);
        assert!(paths.is_empty());
    }

    #[test]
    fn regular_file_is_not_hidden() {
        let dir = std::env::temp_dir();
        let path = dir.join("canvas-shell-visible-test.tmp");
        std::fs::write(&path, b"x").expect("escribir");
        assert!(!is_hidden(&path));
        let _ = std::fs::remove_file(&path);
    }

    #[cfg(windows)]
    #[test]
    fn hidden_attribute_is_detected_on_windows() {
        let dir = std::env::temp_dir();
        let path = dir.join("canvas-shell-hidden-test.tmp");
        std::fs::write(&path, b"x").expect("escribir");
        let status = std::process::Command::new("attrib")
            .arg("+h")
            .arg(&path)
            .status()
            .expect("attrib");
        assert!(status.success());
        assert!(is_hidden(&path));
        let _ = std::process::Command::new("attrib")
            .arg("-h")
            .arg(&path)
            .status();
        let _ = std::fs::remove_file(&path);
    }
}
