//! Tests de `resolve_canvas_sidecar` (mapeo de `.canvas` a su imagen
//! hermana) y de `bake_came_out_blank` (la protección contra horneados en
//! blanco). Hermano de `persistence.rs` (convención `*_tests.rs`).

use canvas_core::{Document, ImageContent, LayerContent, Transform};

use super::{bake_came_out_blank, resolve_canvas_sidecar};

/// Documento de 100×100 con una capa de imagen visible 50×50.
fn doc_with_visible_image() -> Document {
    let mut doc = Document::new(100.0, 100.0);
    doc.add_layer(
        "img",
        Transform::new(0.0, 0.0, 50.0, 50.0),
        LayerContent::Image(ImageContent {
            source_path: None,
            natural_width: 4,
            natural_height: 2,
            crop: None,
        }),
    )
    .expect("añadir capa");
    doc
}

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

// ——— bake_came_out_blank: nunca escribir un horneado en blanco ———

#[test]
fn uniform_bake_with_visible_image_layers_is_blank() {
    // El caso 14.png: capas de imagen visibles, horneado blanco uniforme.
    let doc = doc_with_visible_image();
    let rgba = vec![255u8; 100 * 100 * 4];
    assert!(bake_came_out_blank(&doc, &rgba));
}

#[test]
fn uniform_transparent_bake_is_also_blank() {
    let doc = doc_with_visible_image();
    let rgba = vec![0u8; 100 * 100 * 4];
    assert!(bake_came_out_blank(&doc, &rgba));
}

#[test]
fn varied_bake_with_image_layers_is_not_blank() {
    // Un bake real con contenido: el primer píxel difiere del resto.
    let doc = doc_with_visible_image();
    let mut rgba = vec![255u8; 100 * 100 * 4];
    rgba[0..4].copy_from_slice(&[10, 20, 30, 255]);
    assert!(!bake_came_out_blank(&doc, &rgba));
}

#[test]
fn uniform_bake_without_image_layers_is_allowed() {
    // Diseño vectorial monocromo legítimo: no debe bloquearse.
    let doc = Document::new(100.0, 100.0);
    let rgba = vec![255u8; 100 * 100 * 4];
    assert!(!bake_came_out_blank(&doc, &rgba));
}

#[test]
fn uniform_bake_with_only_hidden_image_layers_is_allowed() {
    // La capa de imagen está oculta: el blanco uniforme puede ser legítimo.
    let mut doc = doc_with_visible_image();
    doc.layer_mut(doc.page().expect("página").layers.first().expect("capa").id)
        .expect("capa")
        .visible = false;
    let rgba = vec![255u8; 100 * 100 * 4];
    assert!(!bake_came_out_blank(&doc, &rgba));
}
