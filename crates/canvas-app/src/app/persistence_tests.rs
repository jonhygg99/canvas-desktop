//! Tests de `resolve_canvas_sidecar` (mapeo de `.canvas` a su imagen
//! hermana). Hermano de `persistence.rs` (convención `*_tests.rs`).

use super::resolve_canvas_sidecar;

#[test]
fn resolves_a_sidecar_inside_the_dot_canvas_folder_to_its_image() {
    let dir = tempfile::tempdir().expect("tempdir");
    let image = dir.path().join("foto.png");
    std::fs::write(&image, b"x").unwrap();
    let sidecar_dir = dir.path().join(canvas_io::SIDECAR_DIR);
    std::fs::create_dir_all(&sidecar_dir).unwrap();
    let sidecar = sidecar_dir.join("foto.png.canvas");
    std::fs::write(&sidecar, b"{}").unwrap();

    assert_eq!(resolve_canvas_sidecar(sidecar), image);
}

#[test]
fn resolves_a_legacy_sibling_sidecar_to_its_image() {
    let dir = tempfile::tempdir().expect("tempdir");
    let image = dir.path().join("foto.png");
    std::fs::write(&image, b"x").unwrap();
    let sidecar = dir.path().join("foto.png.canvas");
    std::fs::write(&sidecar, b"{}").unwrap();

    assert_eq!(resolve_canvas_sidecar(sidecar), image);
}

#[test]
fn a_standalone_design_is_returned_as_is() {
    let dir = tempfile::tempdir().expect("tempdir");
    let design = dir.path().join("Untitled.canvas");
    std::fs::write(&design, b"{}").unwrap();

    assert_eq!(resolve_canvas_sidecar(design.clone()), design);
}
