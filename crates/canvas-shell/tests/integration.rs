//! Tests de integración del shell: normalización de rutas, detección de
//! archivos ocultos, eventos, y el trait `ShellIntegration` de cada plataforma.
//!
//! Estos tests corren en la plataforma nativa (no se mockea el SO), pero
//! usan archivos temporales para no tocar el sistema real de asociaciones.

use canvas_shell::{is_hidden, open_paths_from_args, ShellEvent, ShellIntegration};

use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// open_paths_from_args
// ---------------------------------------------------------------------------

#[test]
fn filters_flags_and_keeps_existing_files() {
    let dir = std::env::temp_dir();
    let img = dir.join("canvas_shell_test_img.png");
    std::fs::write(&img, b"x").unwrap();

    let args = vec![
        "canvas-desktop".to_owned(),
        "--register-shell".to_owned(),
        "-v".to_owned(),
        img.to_string_lossy().into_owned(),
        "Z:/no/existe.png".to_owned(),
    ];
    let paths = open_paths_from_args(args);
    assert_eq!(paths, vec![img.clone()]);

    let _ = std::fs::remove_file(&img);
}

#[test]
fn skips_argv0_even_if_it_exists() {
    let exe = std::env::current_exe().unwrap();
    let paths = open_paths_from_args(vec![exe.to_string_lossy().into_owned()]);
    assert!(paths.is_empty(), "argv[0] nunca se interpreta como ruta");
}

#[test]
fn multiple_existing_files_are_returned_in_order() {
    let dir = std::env::temp_dir();
    let a = dir.join("canvas_shell_a.png");
    let b = dir.join("canvas_shell_b.png");
    std::fs::write(&a, b"x").unwrap();
    std::fs::write(&b, b"x").unwrap();

    let args = vec![
        "canvas-desktop".to_owned(),
        a.to_string_lossy().into_owned(),
        b.to_string_lossy().into_owned(),
    ];
    let paths = open_paths_from_args(args);
    assert_eq!(paths.len(), 2);
    assert_eq!(paths[0], a);
    assert_eq!(paths[1], b);

    let _ = std::fs::remove_file(&a);
    let _ = std::fs::remove_file(&b);
}

#[test]
fn paths_with_spaces_are_preserved() {
    let dir = std::env::temp_dir();
    let name = "canvas shell with spaces.png";
    let path = dir.join(name);
    std::fs::write(&path, b"x").unwrap();

    let args = vec![
        "canvas-desktop".to_owned(),
        path.to_string_lossy().into_owned(),
    ];
    let paths = open_paths_from_args(args);
    assert_eq!(paths.len(), 1);
    assert_eq!(paths[0], path);

    let _ = std::fs::remove_file(&path);
}

#[test]
fn empty_args_produce_empty_paths() {
    let paths = open_paths_from_args(Vec::<String>::new());
    assert!(paths.is_empty());
}

#[test]
fn only_argv0_produces_empty_paths() {
    let paths = open_paths_from_args(vec!["canvas-desktop".to_owned()]);
    assert!(paths.is_empty());
}

#[test]
fn relative_path_that_exists_is_returned_as_is() {
    // Crea un archivo relativo en el cwd actual. El test es best-effort:
    // si el cwd no es escribible, se ignora.
    let name = "canvas_shell_relative_test.tmp";
    let abs = std::env::current_dir().unwrap().join(name);
    if std::fs::write(&abs, b"x").is_err() {
        return; // cwd no escribible, saltar
    }

    let args = vec!["canvas-desktop".to_owned(), name.to_owned()];
    let paths = open_paths_from_args(args);
    assert_eq!(paths.len(), 1);
    // `open_paths_from_args` no canoniza: devuelve la ruta tal como vino.
    assert_eq!(paths[0], PathBuf::from(name));

    let _ = std::fs::remove_file(&abs);
}

#[test]
fn folder_path_is_returned_if_it_exists() {
    let dir = std::env::temp_dir();
    let args = vec![
        "canvas-desktop".to_owned(),
        dir.to_string_lossy().into_owned(),
    ];
    let paths = open_paths_from_args(args);
    assert_eq!(paths, vec![dir]);
}

// ---------------------------------------------------------------------------
// is_hidden
// ---------------------------------------------------------------------------

#[test]
fn regular_file_is_not_hidden() {
    let dir = std::env::temp_dir();
    let path = dir.join("canvas_shell_not_hidden.tmp");
    std::fs::write(&path, b"x").unwrap();
    assert!(!is_hidden(&path));
    let _ = std::fs::remove_file(&path);
}

#[cfg(unix)]
#[test]
fn dotfile_is_hidden_on_unix() {
    let dir = std::env::temp_dir();
    let path = dir.join(".canvas_shell_hidden_test");
    std::fs::write(&path, b"x").unwrap();
    assert!(
        is_hidden(&path),
        "un archivo que empieza por . debe ser oculto en Unix"
    );
    let _ = std::fs::remove_file(&path);
}

#[cfg(windows)]
#[test]
fn hidden_attribute_is_detected_on_windows() {
    let dir = std::env::temp_dir();
    let path = dir.join("canvas_shell_hidden_attr_test.tmp");
    std::fs::write(&path, b"x").unwrap();

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

#[test]
fn nonexistent_path_is_not_hidden() {
    assert!(!is_hidden(Path::new("/Z/no/existe/.archivo")));
}

// ---------------------------------------------------------------------------
// ShellEvent
// ---------------------------------------------------------------------------

#[test]
fn shell_event_open_path_carries_the_path() {
    let event = ShellEvent::OpenPath(PathBuf::from("foto.png"));
    match &event {
        ShellEvent::OpenPath(p) => assert_eq!(p, Path::new("foto.png")),
    }
}

#[test]
fn shell_event_clone_and_eq() {
    let a = ShellEvent::OpenPath(PathBuf::from("a.png"));
    let b = a.clone();
    assert_eq!(a, b);
}

// ---------------------------------------------------------------------------
// ShellIntegration (platform() sin efectos secundarios)
// ---------------------------------------------------------------------------

/// `unregister` debe ser idempotente: no fallar aunque nunca se haya
/// registrado nada. Esto cubre el camino de "desinstalar sin haber
/// instalado", que es lo que pasa si el usuario ejecuta
/// `--unregister-shell` antes de `--register-shell`.
#[test]
fn unregister_is_idempotent_without_prior_registration() {
    let shell = canvas_shell::platform();
    // No debe panicar ni dejar estado colgando.
    let result = shell.unregister_file_associations();
    // En Windows/macOS/Linux, unregister de algo no registrado es Ok(())
    // (borra claves/archivos que no existen).
    assert!(
        result.is_ok(),
        "unregister sin registro previo debe ser Ok, no error: {result:?}"
    );
}

/// `update_jump_list` con lista vacía: en Linux/macOS es no-op (Ok);
/// en Windows la Jump List rechaza categorías vacías (parámetro inválido),
/// lo cual es esperado y no un bug.
#[test]
fn empty_jump_list_behavior() {
    let shell = canvas_shell::platform();
    let result = shell.update_jump_list(&[]);
    #[cfg(windows)]
    {
        // Windows: COM devuelve E_INVALIDARG con una categoría vacía.
        // No es un bug, es una restricción del API.
        assert!(result.is_err(), "Windows rechaza categorías vacías");
    }
    #[cfg(not(windows))]
    {
        assert!(
            result.is_ok(),
            "update_jump_list con lista vacía debe ser Ok en Unix: {result:?}"
        );
    }
}
