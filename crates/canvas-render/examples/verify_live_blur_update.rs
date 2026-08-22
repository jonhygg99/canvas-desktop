//! Verificación headless puntual del bug reportado: mover el slider de blur
//! (o color) de una capa que YA tenía efectos activos no cambiaba nada en
//! pantalla, porque `BlurEngine` reescribía la textura horneada pero nadie
//! avisaba a vello de que su copia en el atlas de imágenes había quedado
//! obsoleta (`Renderer::mark_override_image_dirty`). Este caso solo se
//! dispara en el camino de "re-horneado" (`entry.last != Some(..)` con una
//! entrada YA existente en caché) — por eso ambos horneados de abajo usan el
//! MISMO `CanvasRenderer`/`FxScope`, sin `forget_scope` entre medias.
//!
//! Uso: cargo run -p canvas-render --example verify_live_blur_update

use anyhow::{anyhow, Result};
use canvas_core::{Document, ImageContent, LayerContent, Transform};
use canvas_render::{image_data_from_rgba, CanvasRenderer, FxScope, ImageMap};
use vello::util::RenderContext;

/// Página con un borde duro rojo|azul en x=32, para que el blur "sangre"
/// rojo hacia el lado azul de forma proporcional al radio.
fn doc_with_hard_edge(w: u32, h: u32) -> (Document, ImageMap, canvas_core::LayerId) {
    let mut doc = Document::new(f64::from(w), f64::from(h));
    let id = doc
        .add_layer(
            "edge",
            Transform::new(0.0, 0.0, f64::from(w), f64::from(h)),
            LayerContent::Image(ImageContent {
                source_path: None,
                natural_width: w,
                natural_height: h,
                crop: None,
            }),
        )
        .unwrap();
    let mut pixels = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            if x < w / 2 {
                pixels.extend_from_slice(&[255, 0, 0, 255]);
            } else {
                pixels.extend_from_slice(&[0, 0, 255, 255]);
            }
            let _ = y;
        }
    }
    let mut images = ImageMap::new();
    images.insert(id, image_data_from_rgba(pixels, w, h));
    (doc, images, id)
}

#[allow(clippy::too_many_arguments)]
fn bake_at(
    renderer: &mut CanvasRenderer,
    device: &vello::wgpu::Device,
    queue: &vello::wgpu::Queue,
    scope: FxScope,
    doc: &Document,
    images: &ImageMap,
    sample_x: u32,
    sample_y: u32,
) -> (u8, u8, u8) {
    let (rgba, w, _h) = renderer
        .bake_page(device, queue, scope, doc, images, 1.0)
        .unwrap();
    let idx = ((sample_y * w + sample_x) * 4) as usize;
    (rgba[idx], rgba[idx + 1], rgba[idx + 2])
}

fn main() -> Result<()> {
    let mut ctx = RenderContext::new();
    let device_id = pollster::block_on(ctx.device(None))
        .ok_or_else(|| anyhow!("no hay adaptador wgpu disponible"))?;
    let handle = &ctx.devices[device_id];
    let (device, queue) = (&handle.device, &handle.queue);

    let (mut doc, images, layer_id) = doc_with_hard_edge(64, 64);
    let scope = FxScope::default();
    let mut renderer = CanvasRenderer::new(device)?;

    // Punto de muestra: 8px al lado azul del borde (x=32).
    let (sx, sy) = (40, 32);

    doc.layer_mut(layer_id).unwrap().effects.blur_radius = 50.0;
    let heavy = bake_at(&mut renderer, device, queue, scope, &doc, &images, sx, sy);

    // MISMO renderer/scope, SIN forget_scope: esto es justo el camino de
    // "re-horneado" que el bug dejaba pegado al primer valor.
    doc.layer_mut(layer_id).unwrap().effects.blur_radius = 3.0;
    let light = bake_at(&mut renderer, device, queue, scope, &doc, &images, sx, sy);

    println!("radio 50 en (x={sx},y={sy}): {heavy:?} (mucho rojo sangrando)");
    println!("radio 3  en (x={sx},y={sy}): {light:?} (casi azul puro)");

    // Con radio 50 el rojo sangra notablemente a 8px del borde; con radio 3,
    // casi nada. Si el segundo horneado se hubiera quedado pegado al
    // primero, `light` sería igual (o casi) a `heavy`.
    let updated = light.0 as i16 <= heavy.0 as i16 - 40;
    println!("el segundo horneado reflejó el nuevo radio: {updated}");
    if !updated {
        anyhow::bail!(
            "el horneado con radio 3 no cambió respecto al de radio 50 \
             (canal rojo: {} vs {}) — la actualización en vivo sigue rota",
            light.0,
            heavy.0
        );
    }
    println!(
        "OK: cambiar el radio de blur en una capa ya cacheada se refleja en el siguiente horneado."
    );
    Ok(())
}
