//! Tests del borrado del Download: a la papelera del SO, tolerando lo que
//! ya no exista. Sin red ni yt-dlp.

use super::{delete_to_trash, trashable_path};

#[test]
fn trashable_path_strips_verbatim_prefix() {
    use std::path::PathBuf;
    assert_eq!(
        trashable_path(&PathBuf::from(r"\\?\D:\v\clip.mp4")),
        PathBuf::from(r"D:\v\clip.mp4")
    );
    assert_eq!(
        trashable_path(&PathBuf::from(r"\\?\UNC\srv\share\clip.mp4")),
        PathBuf::from(r"\\srv\share\clip.mp4")
    );
    assert_eq!(
        trashable_path(&PathBuf::from(r"D:\v\clip.mp4")),
        PathBuf::from(r"D:\v\clip.mp4")
    );
}

#[test]
fn delete_moves_files_to_trash() {
    let dir = tempfile::tempdir().expect("temp");
    let a = dir.path().join("clip-a.mp4");
    let b = dir.path().join("clip-b.mp4");
    std::fs::write(&a, [0u8; 4]).expect("a");
    std::fs::write(&b, [0u8; 4]).expect("b");
    let (removed, errors) = delete_to_trash(&[a.clone(), b.clone()]);
    assert!(errors.is_empty());
    assert_eq!(removed, vec![a, b]);
}

#[test]
fn delete_missing_reports_without_panicking() {
    let missing = std::env::temp_dir().join("canvas-no-existe.mp4");
    let (removed, errors) = delete_to_trash(std::slice::from_ref(&missing));
    assert!(removed.is_empty());
    assert_eq!(errors.len(), 1);
}
