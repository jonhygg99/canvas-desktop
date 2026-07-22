//! Verificación headless de los filtros de color GPU: hornea la misma imagen
//! sin ajustes y con escala de grises al 100 %, y comprueba que el resultado
//! neutro es idéntico al original y el gris es de verdad gris (R≈G≈B).
//!
//! Uso: cargo run -p canvas-render --example bake_filters

use anyhow::{anyhow, Result};
use canvas_core::{Document, ImageContent, LayerContent, Transform};
use canvas_render::{image_data_from_rgba, CanvasRenderer, ImageMap};
use vello::util::RenderContext;

fn make_doc(w: u32, h: u32, rgba: Vec<u8>) -> Result<(Document, ImageMap, canvas_core::LayerId)> {
    let mut doc = Document::new(f64::from(w), f64::from(h));
    let id = doc.add_layer(
        "img",
        Transform::new(0.0, 0.0, f64::from(w), f64::from(h)),
        LayerContent::Image(ImageContent {
            source_path: None,
            natural_width: w,
            natural_height: h,
            crop: None,
        }),
    )?;
    let mut images = ImageMap::new();
    images.insert(id, image_data_from_rgba(rgba, w, h));
    Ok((doc, images, id))
}

fn main() -> Result<()> {
    // Imagen sintética muy saturada (roja/azul) para que el gris se note.
    let (w, h) = (64u32, 64u32);
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            if (x / 8 + y / 8) % 2 == 0 {
                rgba.extend_from_slice(&[220, 30, 30, 255]);
            } else {
                rgba.extend_from_slice(&[30, 30, 220, 255]);
            }
        }
    }

    let mut ctx = RenderContext::new();
    let device_id = pollster::block_on(ctx.device(None))
        .ok_or_else(|| anyhow!("no hay adaptador wgpu disponible"))?;
    let handle = &ctx.devices[device_id];
    let (device, queue) = (&handle.device, &handle.queue);
    let mut renderer = CanvasRenderer::new(device)?;

    // 1) Sin ajustes: los píxeles salen tal cual entraron.
    let (doc, images, _) = make_doc(w, h, rgba.clone())?;
    let (neutral, ..) = renderer.bake_page(device, queue, &doc, &images, 1.0)?;
    anyhow::ensure!(
        neutral == rgba,
        "el horneado neutro debería ser idéntico al original"
    );
    println!("neutro OK: sin ajustes, la imagen no cambia");

    // 2) Escala de grises al 100 %: R, G y B casi iguales en todo píxel.
    let (mut doc, images, id) = make_doc(w, h, rgba)?;
    doc.layer_mut(id)?.effects.grayscale = 1.0;
    let (gray, ..) = renderer.bake_page(device, queue, &doc, &images, 1.0)?;
    let center = ((32 * w + 32) * 4) as usize;
    let px = &gray[center..center + 3];
    anyhow::ensure!(
        px[0].abs_diff(px[1]) <= 2 && px[1].abs_diff(px[2]) <= 2,
        "el gris debería tener R≈G≈B, fue {px:?}"
    );
    let context_px = gray
        .chunks(4)
        .take(64)
        .all(|c| c[0].abs_diff(c[1]) <= 2 && c[1].abs_diff(c[2]) <= 2);
    anyhow::ensure!(context_px, "toda la primera fila debería ser gris");
    println!("grayscale OK: R≈G≈B en el horneado ({px:?})");
    Ok(())
}
