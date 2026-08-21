//! Verificación headless puntual del bug reportado: al guardar dos "fotos"
//! (documentos) distintas seguidas con el mismo `CanvasRenderer` compartido,
//! el fondo desenfocado horneado de la segunda no debe ser el de la primera.
//! Reproduce el escenario real: cada `Document` nuevo empieza su `LayerId`
//! en 1, y ambos usan blur_radius=50 (igual que el fondo automático).
//!
//! Uso: cargo run -p canvas-render --example verify_scope_fix

use anyhow::{anyhow, Result};
use canvas_core::{Document, ImageContent, LayerContent, Transform};
use canvas_render::{image_data_from_rgba, CanvasRenderer, FxScope, ImageMap};
use vello::util::RenderContext;

fn doc_with_color(w: u32, h: u32, rgb: [u8; 3]) -> (Document, ImageMap) {
    let mut doc = Document::new(f64::from(w), f64::from(h));
    let id = doc
        .add_layer(
            "bg",
            Transform::new(0.0, 0.0, f64::from(w), f64::from(h)),
            LayerContent::Image(ImageContent {
                source_path: None,
                natural_width: w,
                natural_height: h,
                crop: None,
            }),
        )
        .unwrap();
    doc.layer_mut(id).unwrap().effects.blur_radius = 50.0;
    let mut pixels = Vec::with_capacity((w * h * 4) as usize);
    for _ in 0..(w * h) {
        pixels.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
    }
    let mut images = ImageMap::new();
    images.insert(id, image_data_from_rgba(pixels, w, h));
    (doc, images)
}

fn bake(
    renderer: &mut CanvasRenderer,
    device: &vello::wgpu::Device,
    queue: &vello::wgpu::Queue,
    scope: FxScope,
    doc: &Document,
    images: &ImageMap,
    forget_first: bool,
) -> (u8, u8, u8) {
    if forget_first {
        renderer.forget_scope(scope);
    }
    let (rgba, _, _) = renderer.bake_page(device, queue, scope, doc, images, 1.0).unwrap();
    (rgba[0], rgba[1], rgba[2])
}

fn main() -> Result<()> {
    let mut ctx = RenderContext::new();
    let device_id = pollster::block_on(ctx.device(None))
        .ok_or_else(|| anyhow!("no hay adaptador wgpu disponible"))?;
    let handle = &ctx.devices[device_id];
    let (device, queue) = (&handle.device, &handle.queue);

    let (doc_a, images_a) = doc_with_color(64, 64, [255, 0, 0]); // "foto A": rojo
    let (doc_b, images_b) = doc_with_color(64, 64, [0, 0, 255]); // "foto B": azul

    // Caso SIN el fix (bug reproducido): mismo scope, sin vaciar la caché entre horneados.
    let mut buggy = CanvasRenderer::new(device)?;
    let a1 = bake(&mut buggy, device, queue, FxScope::default(), &doc_a, &images_a, false);
    let b1 = bake(&mut buggy, device, queue, FxScope::default(), &doc_b, &images_b, false);
    println!("SIN fix -> foto A: {a1:?}, foto B: {b1:?} (B debería ser azul, no rojo)");

    // Caso CON el fix: forget_scope antes de cada horneado (lo que ahora hace start_save/start_export).
    let mut fixed = CanvasRenderer::new(device)?;
    let a2 = bake(&mut fixed, device, queue, FxScope::default(), &doc_a, &images_a, true);
    let b2 = bake(&mut fixed, device, queue, FxScope::default(), &doc_b, &images_b, true);
    println!("CON fix -> foto A: {a2:?}, foto B: {b2:?}");

    let bug_reproduced = b1.2 < 100; // sin fix, B sale rojo (canal azul bajo) en vez de azul
    let fix_works = b2.2 > 150 && b2.0 < 100; // con fix, B sale azul de verdad

    println!("bug reproducido sin el fix: {bug_reproduced}");
    println!("fix corrige el resultado: {fix_works}");

    if !bug_reproduced || !fix_works {
        anyhow::bail!("la verificación no dio el resultado esperado");
    }
    println!("OK: el fix evita que el fondo desenfocado de una foto contamine a la siguiente.");
    Ok(())
}
