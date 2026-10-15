//! Tests del borrado del Download: a la papelera del SO, tolerando lo que
//! ya no exista. Sin red ni yt-dlp.

use super::{
    delete_to_trash, folder_is_download_empty, prune_emptied_download_folders, trashable_path,
};

#[test]
fn emptying_the_download_root_prunes_the_folder_itself() {
    let dir = tempfile::tempdir().expect("temp");
    let root = dir.path().join("staging");
    std::fs::create_dir(&root).expect("staging");
    let a = root.join("clip-a.mp4");
    std::fs::write(&a, [0u8; 4]).expect("a");
    // Basura ajena al binario descargado y otra carpeta descartada: el
    // staging se elimina entero, lo demás queda como está.
    std::fs::write(root.join("sweep.tmp"), [0u8; 2]).expect("junk");
    let away = dir.path().join("otra-carpeta");
    std::fs::create_dir(&away).expect("away");
    let stray = away.join("stray-clip.mp4");
    std::fs::write(&stray, [0u8; 4]).expect("stray");

    // Flujo real: la papelera sale PRIMERO (sin ella el clip seguiría en
    // la carpeta y la poda se negaría, como en producción).
    let (removed, errors) = delete_to_trash(&[a]);
    assert!(errors.is_empty());
    let folders = prune_emptied_download_folders(&removed, &root);
    assert_eq!(folders, vec![root.clone()]);
    assert!(!root.exists(), "la carpeta del Download se elimina");
    assert!(stray.is_file(), "solo se poda el staging dado");
}

#[test]
fn clip_left_behind_or_ongoing_download_keeps_the_folder() {
    let dir = tempfile::tempdir().expect("temp");
    let root = dir.path().join("staging");
    std::fs::create_dir(&root).expect("staging");
    let a = root.join("clip-a.mp4");
    let b = root.join("clip-b.mp4");
    std::fs::write(&a, [0u8; 4]).expect("a");
    std::fs::write(&b, [0u8; 4]).expect("b");
    // Queda otro clip: nada que podar.
    assert!(prune_emptied_download_folders(&[a], &root).is_empty());
    assert!(b.is_file());
    std::fs::remove_file(&b).expect("rm");
    std::fs::write(root.join("resume.mp4.part"), [0u8; 2]).expect("part");
    // Sin clips pero con parcial de reanudación: la poda se niega.
    assert!(prune_emptied_download_folders(&[b], &root).is_empty());
    assert!(root.is_dir());
}

#[test]
fn folders_away_from_the_download_root_are_never_pruned() {
    let dir = tempfile::tempdir().expect("temp");
    let away = dir.path().join("otra-carpeta");
    std::fs::create_dir(&away).expect("away");
    let stray = away.join("clip.mp4");
    std::fs::write(&stray, [0u8; 4]).expect("stray");
    assert!(prune_emptied_download_folders(&[stray], &dir.path().join("nope")).is_empty());
    assert!(away.is_dir());
}

#[test]
fn clip_on_display_blocks_the_prune_regardless_of_case() {
    // `Video.MP4` (mayúsculas, Windows no distingue) está EN la lista del
    // sidebar: la carpeta no puede declararse vacía y destruirlo — lista
    // y poda comparten la tabla `api::is_download_name`.
    let dir = tempfile::tempdir().expect("temp");
    let root = dir.path().join("staging");
    std::fs::create_dir(&root).expect("staging");
    let a = root.join("clip-a.mp4");
    let shown = root.join("Video.MP4");
    std::fs::write(&a, [0u8; 4]).expect("a");
    std::fs::write(&shown, [0u8; 4]).expect("mp4");
    let (removed, errors) = delete_to_trash(&[a]);
    assert!(errors.is_empty());
    assert!(prune_emptied_download_folders(&removed, &root).is_empty());
    assert!(root.is_dir());
    assert!(shown.is_file());
}

#[test]
fn folder_is_download_empty_checks_clips_and_partials() {
    let dir = tempfile::tempdir().expect("temp");
    let root = dir.path().join("staging");
    std::fs::create_dir(&root).expect("staging");
    std::fs::write(root.join("clip.mp4"), [0u8; 2]).expect("clip");
    assert!(!folder_is_download_empty(&root));
    std::fs::remove_file(root.join("clip.mp4")).expect("rm");
    assert!(folder_is_download_empty(&root));
    std::fs::write(root.join("resume.mp4.part"), [0u8; 2]).expect("part");
    assert!(!folder_is_download_empty(&root), "un .part impide la poda");
    std::fs::remove_file(root.join("resume.mp4.part")).expect("rm");
    std::fs::write(root.join("resume.mp4.ytdl"), [0u8; 2]).expect("ytdl");
    assert!(!folder_is_download_empty(&root), "un .ytdl impide la poda");
    std::fs::remove_file(root.join("resume.mp4.ytdl")).expect("rm");
    // Intermedios del merge de un «mute» (la tabla compartida los conoce).
    std::fs::write(root.join("mute.webm"), [0u8; 2]).expect("webm");
    assert!(!folder_is_download_empty(&root), "un .webm impide la poda");
    std::fs::remove_file(root.join("mute.webm")).expect("rm");
    std::fs::write(root.join("CLIP.M4A"), [0u8; 2]).expect("m4a");
    assert!(!folder_is_download_empty(&root), "un .M4A impide la poda");
}

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
