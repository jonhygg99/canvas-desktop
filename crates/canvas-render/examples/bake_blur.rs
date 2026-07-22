//! Verificación headless del pipeline de render y del desenfoque GPU:
//! carga una imagen, la mete en un documento con blur y hornea el resultado
//! a un PNG, sin abrir ninguna ventana.
//!
//! Uso: cargo run -p canvas-render --example bake_blur -- <entrada> <salida> [radio]

use anyhow::{anyhow, Context, Result};
use canvas_core::{Document, ImageContent, LayerContent, Transform};
use canvas_render::{image_data_from_rgba, CanvasRenderer, ImageMap};
use vello::util::RenderContext;

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let input = args.next().context("falta la ruta de entrada")?;
    let output = args.next().context("falta la ruta de salida")?;
    let radius: f32 = args.next().map(|r| r.parse()).transpose()?.unwrap_or(20.0);

    // Carga la imagen a RGBA8.
    let img = image::open(&input)?.to_rgba8();
    let (w, h) = img.dimensions();
    println!("entrada: {input} ({w}x{h}), radio de blur: {radius}");

    // Documento con la imagen a página completa y blur aplicado.
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
    doc.layer_mut(id)?.effects.blur_radius = radius;
    let mut images = ImageMap::new();
    images.insert(id, image_data_from_rgba(img.into_raw(), w, h));

    // Device wgpu headless.
    let mut ctx = RenderContext::new();
    let device_id = pollster::block_on(ctx.device(None))
        .ok_or_else(|| anyhow!("no hay adaptador wgpu disponible"))?;
    let handle = &ctx.devices[device_id];
    let (device, queue) = (&handle.device, &handle.queue);

    let mut renderer = CanvasRenderer::new(device)?;
    let (rgba, bw, bh) = renderer.bake_page(device, queue, &doc, &images, 1.0)?;

    image::RgbaImage::from_raw(bw, bh, rgba)
        .context("buffer horneado con tamaño inesperado")?
        .save(&output)?;
    println!("salida: {output} ({bw}x{bh})");
    Ok(())
}
