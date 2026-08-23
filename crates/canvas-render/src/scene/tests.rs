//! Tests de `append_document`. No necesitan GPU: `vello::Scene` es solo un
//! buffer de codificación en CPU, así que se puede construir la escena y
//! comprobar sus contadores sin abrir ningún adaptador.

use std::sync::mpsc;
use std::time::Duration;

use canvas_core::{
    Document, ImageContent, LayerContent, LayerId, ShapeContent, ShapeKind, SvgContent, Transform,
};
use vello::kurbo::Affine;
use vello::Scene;

use super::{append_document, image_data_from_rgba, ImageMap};

fn image_layer(doc: &mut Document, name: &str) -> LayerId {
    doc.add_layer(
        name,
        Transform::new(0.0, 0.0, 100.0, 100.0),
        LayerContent::Image(ImageContent {
            source_path: None,
            natural_width: 10,
            natural_height: 10,
            crop: None,
        }),
    )
    .expect("el documento nuevo siempre tiene página")
}

fn svg_layer(doc: &mut Document, name: &str) -> LayerId {
    doc.add_layer(
        name,
        Transform::new(0.0, 0.0, 100.0, 100.0),
        LayerContent::Svg(SvgContent {
            source: "<svg/>".to_owned(),
            natural_width: 10,
            natural_height: 10,
        }),
    )
    .expect("el documento nuevo siempre tiene página")
}

fn shape_layer(doc: &mut Document, name: &str) -> LayerId {
    doc.add_layer(
        name,
        Transform::new(10.0, 10.0, 50.0, 50.0),
        LayerContent::Shape(ShapeContent {
            kind: ShapeKind::Rect,
            fill: [255, 0, 0, 255],
            stroke: [0, 0, 0, 0],
            stroke_width: 0.0,
            corner_radius: 0.0,
        }),
    )
    .expect("el documento nuevo siempre tiene página")
}

/// Construye la escena en un hilo aparte con límite de tiempo. Si vuelve el
/// bucle infinito, el test FALLA en vez de colgar la suite entera.
fn render_with_timeout(doc: Document, images: ImageMap) -> Scene {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut scene = Scene::new();
        append_document(
            &mut scene,
            &doc,
            &images,
            &ImageMap::new(),
            Affine::IDENTITY,
            false,
        );
        let _ = tx.send(scene);
    });
    rx.recv_timeout(Duration::from_secs(10))
        .expect("append_document no terminó: el bucle de capas no avanza")
}

#[test]
fn an_image_layer_whose_texture_has_not_loaded_does_not_hang_the_loop() {
    // Regresión: los brazos `Image`/`Svg` hacían `continue` cuando la textura
    // no estaba todavía en el `ImageMap` (carga asíncrona en vuelo). Ese
    // `continue` saltaba al `while` exterior sin pasar por el `i += 1` del
    // final del cuerpo, así que el índice no avanzaba nunca.
    let mut doc = Document::new(800.0, 600.0);
    image_layer(&mut doc, "todavía cargando");
    render_with_timeout(doc, ImageMap::new());
}

#[test]
fn an_svg_layer_whose_texture_has_not_loaded_does_not_hang_the_loop() {
    let mut doc = Document::new(800.0, 600.0);
    svg_layer(&mut doc, "todavía cargando");
    render_with_timeout(doc, ImageMap::new());
}

#[test]
fn a_zero_sized_bitmap_does_not_hang_the_loop() {
    // El otro `continue` del mismo brazo: la textura existe pero mide 0x0.
    let mut doc = Document::new(800.0, 600.0);
    let id = image_layer(&mut doc, "mapa de bits vacío");
    let mut images = ImageMap::new();
    images.insert(id, image_data_from_rgba(Vec::new(), 0, 0));
    render_with_timeout(doc, images);
}

#[test]
fn a_missing_texture_does_not_leave_an_opacity_layer_open() {
    // La capa de opacidad se empuja ANTES de mirar la textura, así que
    // saltarse el final del cuerpo también se saltaba su `pop_layer`.
    let mut doc = Document::new(800.0, 600.0);
    let id = image_layer(&mut doc, "translúcida y sin cargar");
    doc.layer_mut(id).expect("recién insertada").opacity = 0.5;

    let scene = render_with_timeout(doc, ImageMap::new());
    assert_eq!(
        scene.encoding().n_open_clips,
        0,
        "quedó una capa sin cerrar: push_layer/pop_layer desbalanceados"
    );
}

#[test]
fn a_missing_texture_does_not_stop_the_layers_above_it_from_painting() {
    // Lo que de verdad ve el usuario: una imagen a medio cargar no debe
    // borrar el resto del lienzo.
    let mut with_gap = Document::new(800.0, 600.0);
    image_layer(&mut with_gap, "sin cargar");
    shape_layer(&mut with_gap, "encima");
    let painted = render_with_timeout(with_gap, ImageMap::new());

    let mut only_gap = Document::new(800.0, 600.0);
    image_layer(&mut only_gap, "sin cargar");
    let baseline = render_with_timeout(only_gap, ImageMap::new());

    assert!(
        painted.encoding().n_paths > baseline.encoding().n_paths,
        "la forma de encima de la capa sin cargar no llegó a pintarse"
    );
}

#[test]
fn a_loaded_image_still_paints() {
    // Contraprueba del camino feliz: sin esto, un `drawable_image` que
    // devolviera siempre `None` pasaría todos los tests de arriba.
    let mut doc = Document::new(800.0, 600.0);
    let id = image_layer(&mut doc, "cargada");
    let mut images = ImageMap::new();
    images.insert(id, image_data_from_rgba(vec![255; 4 * 10 * 10], 10, 10));
    let painted = render_with_timeout(doc, images);

    // La referencia es un documento SIN capas, no uno con la imagen sin
    // cargar: así este test no depende de que el camino de la textura
    // ausente funcione.
    let baseline = render_with_timeout(Document::new(800.0, 600.0), ImageMap::new());

    assert!(
        painted.encoding().n_paths > baseline.encoding().n_paths,
        "la imagen cargada no llegó a pintarse"
    );
}
