//! Tabla de la política de destino de una escritura de fondo
//! (`write_target.rs`): qué se escribe solo y qué espera a su lienzo.

use std::path::PathBuf;

use canvas_core::{Document, ImageContent, LayerContent, Transform};

use super::{background_target, BackgroundRequest, BackgroundWrite};

fn raster(path: &str) -> Document {
    let mut doc = Document::new(100.0, 100.0);
    doc.add_layer(
        "photo",
        Transform::new(0.0, 0.0, 50.0, 50.0),
        LayerContent::Image(ImageContent {
            source_path: Some(PathBuf::from(path)),
            natural_width: 4,
            natural_height: 2,
            crop: None,
        }),
    )
    .unwrap();
    doc.source_path = Some(PathBuf::from(path));
    doc
}

fn design(path: &str) -> Document {
    let mut doc = Document::new(100.0, 100.0);
    doc.source_path = Some(PathBuf::from(path));
    doc
}

/// Lo que la app puede escribir sin preguntar: un PNG que escribió ella.
fn app_written(doc: &Document) -> BackgroundRequest<'_> {
    BackgroundRequest {
        doc,
        is_placeholder: false,
        is_design: false,
        born_blank: false,
        external_change: false,
        app_written: true,
        overwrite_ok: false,
        skip_overwrite_warning: false,
    }
}

fn target(req: BackgroundRequest<'_>) -> Option<PathBuf> {
    match background_target(req) {
        BackgroundWrite::Auto(path) => Some(path),
        BackgroundWrite::Skip(_) => None,
    }
}

#[test]
fn an_app_written_raster_canvas_is_written_without_asking() {
    let doc = raster("bulk/01.png");
    let path = target(app_written(&doc)).expect("destino automático");
    assert_eq!(path, PathBuf::from("bulk/01.png"));
}

#[test]
fn an_unsaved_placeholder_or_a_never_saved_canvas_is_skipped() {
    let doc = raster("a.png");
    let mut req = app_written(&doc);
    req.is_placeholder = true;
    assert!(target(req).is_none());

    let mut bare = Document::new(10.0, 10.0);
    bare.source_path = None;
    assert!(target(app_written(&bare)).is_none());
}

#[test]
fn an_external_change_is_never_overwritten_behind_the_users_back() {
    let doc = raster("a.png");
    let mut req = app_written(&doc);
    req.external_change = true;
    assert!(target(req).is_none());
}

/// El original del usuario, abierto desde el disco, sigue pidiendo la misma
/// confirmación destructiva que el `Ctrl+S`: una ranura de fondo no puede
/// enseñarla, así que no se escribe sola.
#[test]
fn an_untouched_original_needs_the_overwrite_answer() {
    let doc = raster("foto.png");
    let mut req = app_written(&doc);
    req.app_written = false;
    assert!(target(req).is_none());

    let mut confirmed = app_written(&doc);
    confirmed.app_written = false;
    confirmed.overwrite_ok = true;
    assert!(target(confirmed).is_some());

    let mut dont_ask = app_written(&doc);
    dont_ask.app_written = false;
    dont_ask.skip_overwrite_warning = true;
    assert!(target(dont_ask).is_some());
}

#[test]
fn a_document_with_no_raster_layers_waits_for_its_own_dialog() {
    let doc = design("vector.png"); // sin capas de imagen
    assert!(canvas_io::is_image_file(std::path::Path::new("vector.png")));
    // `is_design` falso: es un raster que perdió su última foto.
    assert!(target(app_written(&doc)).is_none());
    // Un lienzo nacido en blanco no tiene foto que perder: se escribe.
    let mut born = app_written(&doc);
    born.born_blank = true;
    assert!(target(born).is_some());
}

#[test]
fn read_only_formats_are_left_to_their_canvas() {
    let doc = raster("drawing.svg");
    assert!(target(app_written(&doc)).is_none());
}

#[test]
fn a_design_needs_a_canvas_destination() {
    let doc = design("design.canvas");
    let mut req = app_written(&doc);
    req.is_design = true;
    assert_eq!(
        target(req).expect("diseño escribible"),
        PathBuf::from("design.canvas")
    );

    let wrong = design("photo.png");
    let mut req = app_written(&wrong);
    req.is_design = true;
    assert!(target(req).is_none());
}
