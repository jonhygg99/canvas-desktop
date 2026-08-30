//! Composición de capas de un documento en una escena vello.

use canvas_core::{Document, LayerContent};
use vello::kurbo::{Affine, Rect};
use vello::peniko::Fill;
use vello::Scene;

use super::raster::{checker_image, place_transform, ImageMap};
use super::shape::draw_shape;
use super::text::draw_text;
use super::{draw_atlas_anchor, drawable_image};

pub(super) fn append_document(
    scene: &mut Scene,
    doc: &Document,
    images: &ImageMap,
    blurred: &ImageMap,
    view: Affine,
    decorated: bool,
) {
    // El camino de pantalla conserva la omisión silenciosa (la carga
    // asíncrona de texturas es legítima); el contador es para el horneado,
    // que la rechaza como bake incompleto.
    append_document_counting(scene, doc, images, blurred, view, decorated, &mut 0);
}

/// Variante con contador de capas de imagen/SVG visibles sin píxel que
/// pintar (`drawable_image` → `None`: carga pendiente, mapa ausente o 0×0).
pub(super) fn append_document_counting(
    scene: &mut Scene,
    doc: &Document,
    images: &ImageMap,
    blurred: &ImageMap,
    view: Affine,
    decorated: bool,
    skipped: &mut usize,
) {
    draw_atlas_anchor(scene);
    let Ok(page) = doc.page() else { return };
    let page_rect = Rect::new(0.0, 0.0, page.width, page.height);
    match page.background {
        Some([r, g, b, a]) => scene.fill(
            Fill::NonZero,
            view,
            vello::peniko::Color::from_rgba8(r, g, b, a),
            None,
            &page_rect,
        ),
        None if !decorated => {}
        None => scene.fill(
            Fill::NonZero,
            view,
            &vello::peniko::ImageBrush {
                image: checker_image().clone(),
                sampler: vello::peniko::ImageSampler {
                    x_extend: vello::peniko::Extend::Repeat,
                    y_extend: vello::peniko::Extend::Repeat,
                    quality: vello::peniko::ImageQuality::Low,
                    alpha: 1.0,
                },
            },
            Some(Affine::scale(8.0)),
            &page_rect,
        ),
    }
    scene.push_layer(
        Fill::NonZero,
        vello::peniko::Mix::Normal,
        1.0,
        view,
        &page_rect,
    );
    let mut open = Vec::with_capacity(8);
    let mut i = 0usize;
    while i < page.layers.len() {
        close_finished_groups(scene, &mut open, i);
        let layer = &page.layers[i];
        let len = page.subtree_len(i);
        if !layer.visible {
            i += 1 + len;
            continue;
        }
        let alpha = layer.opacity.clamp(0.0, 1.0);
        let fade = alpha < 1.0;
        if fade {
            scene.push_layer(
                Fill::NonZero,
                vello::peniko::Mix::Normal,
                alpha,
                view,
                &page_rect,
            );
        }
        if matches!(layer.content, LayerContent::Group(_)) {
            open.push((i + len, fade));
            i += 1;
            continue;
        }
        draw_shadow(scene, layer, view);
        draw_content(scene, layer, images, blurred, view, skipped);
        if fade {
            scene.pop_layer();
        }
        i += 1;
    }
    while let Some((_, pushed)) = open.pop() {
        if pushed {
            scene.pop_layer();
        }
    }
    scene.pop_layer();
    if decorated {
        scene.stroke(
            &vello::kurbo::Stroke::new(1.0),
            view,
            vello::peniko::color::palette::css::BLACK.with_alpha(0.25),
            None,
            &page_rect,
        );
    }
}

fn close_finished_groups(scene: &mut Scene, open: &mut Vec<(usize, bool)>, index: usize) {
    while open.last().is_some_and(|&(end, _)| index > end) {
        let (_, pushed) = open.pop().expect("la condición garantiza un grupo");
        if pushed {
            scene.pop_layer();
        }
    }
}

fn draw_shadow(scene: &mut Scene, layer: &canvas_core::Layer, view: Affine) {
    let Some(shadow) = layer.effects.shadow else {
        return;
    };
    let t = layer.transform;
    let rect = Rect::new(
        t.x + shadow.offset_x,
        t.y + shadow.offset_y,
        t.x + t.width + shadow.offset_x,
        t.y + t.height + shadow.offset_y,
    );
    scene.draw_blurred_rounded_rect(
        view,
        rect,
        vello::peniko::Color::BLACK.with_alpha(shadow.opacity.clamp(0.0, 1.0)),
        0.0,
        f64::from(shadow.blur.max(0.0)),
    );
}

fn draw_content(
    scene: &mut Scene,
    layer: &canvas_core::Layer,
    images: &ImageMap,
    blurred: &ImageMap,
    view: Affine,
    skipped: &mut usize,
) {
    match &layer.content {
        LayerContent::Image(content) => {
            draw_image(scene, layer, content.crop, images, blurred, view, skipped)
        }
        LayerContent::Svg(_) => draw_svg(scene, layer, images, blurred, view, skipped),
        LayerContent::Text(text) => {
            let t = layer.transform;
            draw_text(scene, view * place_transform(&t), text, t.width);
        }
        LayerContent::Shape(shape) => draw_shape(scene, layer, shape, view),
        LayerContent::Group(_) => unreachable!("los grupos se gestionan en el recorrido"),
    }
}

fn draw_image(
    scene: &mut Scene,
    layer: &canvas_core::Layer,
    crop: Option<canvas_core::CropRect>,
    images: &ImageMap,
    blurred: &ImageMap,
    view: Affine,
    skipped: &mut usize,
) {
    let Some(image) = drawable_image(blurred, images, layer.id) else {
        *skipped += 1;
        return;
    };
    let t = layer.transform;
    let place = place_transform(&t);
    let crop = crop
        .map(canvas_core::CropRect::clamped)
        .unwrap_or_else(canvas_core::CropRect::full);
    let iw = f64::from(image.width);
    let ih = f64::from(image.height);
    let image_local =
        Affine::scale_non_uniform(t.width / (crop.width * iw), t.height / (crop.height * ih))
            * Affine::translate((-crop.x * iw, -crop.y * ih));
    let cropped = crop != canvas_core::CropRect::full();
    if cropped {
        scene.push_layer(
            Fill::NonZero,
            vello::peniko::Mix::Normal,
            1.0,
            view * place,
            &Rect::new(0.0, 0.0, t.width, t.height),
        );
    }
    scene.draw_image(image, view * place * image_local);
    if cropped {
        scene.pop_layer();
    }
}

fn draw_svg(
    scene: &mut Scene,
    layer: &canvas_core::Layer,
    images: &ImageMap,
    blurred: &ImageMap,
    view: Affine,
    skipped: &mut usize,
) {
    let Some(image) = drawable_image(blurred, images, layer.id) else {
        *skipped += 1;
        return;
    };
    let t = layer.transform;
    let image_local = Affine::scale_non_uniform(
        t.width / f64::from(image.width),
        t.height / f64::from(image.height),
    );
    scene.draw_image(image, view * place_transform(&t) * image_local);
}
