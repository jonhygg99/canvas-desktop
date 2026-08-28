//! Benchmark CPU (sin GPU) de la construcción de escena de vello.
//!
//! `append_document` codifica el frame en un `vello::Scene` por frame y por
//! ventana, así que su tiempo es parte del presupuesto de repintado junto
//! con el bake GPU y la composición. Este bench mide ese coste con un
//! documento mixto (imágenes con textura, formas, texto y un grupo) y sirve
//! para detectar regresiones antes de que lleguen a los 60 fps del usuario.
//!
//! ```sh
//! cargo bench -p canvas-render --bench scene_build
//! ```

use std::hint::black_box;
use std::time::Duration;

use canvas_core::{Document, ImageContent, LayerContent, ShapeContent, ShapeKind, Transform};
use canvas_render::{append_document, image_data_from_rgba, ImageMap};
use criterion::{criterion_group, criterion_main, Criterion};
use vello::kurbo::Affine;
use vello::Scene;

/// Documento representativo: 2 imágenes con textura, 4 formas, 1 grupo con
/// 3 capas dentro. Tamaños modestos a propósito: mide el coste del
/// recorrido/codificación, no de la decodificación de píxeles.
fn sample_doc() -> (Document, ImageMap) {
    let mut doc = Document::new(1920.0, 1080.0);
    let mut images = ImageMap::new();
    let rgba = vec![128u8; 16 * 16 * 4];
    for i in 0..2 {
        let id = doc
            .add_layer(
                format!("img{i}"),
                Transform::new(40.0 * i as f64, 40.0, 320.0, 180.0),
                LayerContent::Image(ImageContent {
                    source_path: None,
                    natural_width: 16,
                    natural_height: 16,
                    crop: None,
                }),
            )
            .expect("el documento nuevo siempre tiene página");
        images.insert(id, image_data_from_rgba(rgba.clone(), 16, 16));
    }
    for i in 0..4 {
        doc.add_layer(
            format!("shape{i}"),
            Transform::new(10.0 * i as f64, 500.0, 200.0, 120.0),
            LayerContent::Shape(ShapeContent {
                kind: ShapeKind::Rect,
                fill: [200, 60, 60, 255],
                stroke: [0, 0, 0, 255],
                stroke_width: 2.0,
                corner_radius: 8.0,
            }),
        )
        .expect("el documento nuevo siempre tiene página");
    }
    let group = doc.allocate_layer_id();
    let page = doc.page_mut().expect("hay página");
    page.insert_child(
        canvas_core::Layer::group(group, "group"),
        None,
        page.layers.len(),
    );
    for i in 0..3 {
        let leaf = doc
            .add_layer(
                format!("leaf{i}"),
                Transform::new(20.0 * i as f64, 700.0, 100.0, 80.0),
                LayerContent::Shape(ShapeContent {
                    kind: ShapeKind::Rect,
                    fill: [60, 200, 60, 255],
                    stroke: [0, 0, 0, 0],
                    stroke_width: 0.0,
                    corner_radius: 0.0,
                }),
            )
            .expect("el documento nuevo siempre tiene página");
        doc.page_mut()
            .expect("hay página")
            .move_subtree(leaf, Some(group), i)
            .expect("mover dentro del grupo");
    }
    (doc, images)
}

fn scene_build(c: &mut Criterion) {
    let (doc, images) = sample_doc();
    let mut group = c.benchmark_group("scene");
    group.measurement_time(Duration::from_secs(5));
    group.bench_function("append_document/mixed_document", |b| {
        b.iter(|| {
            let mut scene = Scene::new();
            append_document(
                black_box(&mut scene),
                black_box(&doc),
                black_box(&images),
                &ImageMap::new(),
                Affine::IDENTITY,
                false,
            );
            scene
        })
    });
    group.finish();
}

criterion_group!(benches, scene_build);
criterion_main!(benches);
