//! Integración con el sistema operativo. Todo el código específico de
//! plataforma vive aquí, detrás de `#[cfg(target_os = ...)]`.
//!
//! Normaliza las distintas vías de apertura (argv en frío, segunda instancia,
//! `openURLs` de macOS) en un único evento interno [`ShellEvent::OpenPath`].

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
}
