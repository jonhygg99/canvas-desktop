//! Verificación headless del flujo de guardado completo — la misma cadena
//! que ejecuta Ctrl+S en la app: documento editado → `bake_page` (GPU) →
//! `canvas_io::save_rgba` (escritura atómica sobre un archivo existente).
//!
//! Uso: cargo run -p canvas-render --example save_roundtrip -- <entrada> <destino>

use anyhow::{anyhow, Context, Result};
use canvas_core::{Document, ImageContent, LayerContent, Transform};
use canvas_render::{image_data_from_rgba, CanvasRenderer, ImageMap};
use vello::util::RenderContext;

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let input = args.next().context("falta la ruta de entrada")?;
    let target = args.next().context("falta la ruta destino")?;

    let img = image::open(&input)?.to_rgba8();
    let (w, h) = img.dimensions();

    // Documento con una edición real: imagen encogida al 50% y centrada,
    // con un desenfoque suave. Es lo que debe verse en el archivo guardado.
    let mut doc = Document::new(f64::from(w), f64::from(h));
    let id = doc.add_layer(
        "img",
        Transform::new(
            f64::from(w) * 0.25,
            f64::from(h) * 0.25,
            f64::from(w) * 0.5,
            f64::from(h) * 0.5,
        ),
        LayerContent::Image(ImageContent {
            source_path: None,
            natural_width: w,
            natural_height: h,
        }),
    )?;
    doc.layer_mut(id)?.effects.blur_radius = 6.0;
    doc.page_mut()?.background = Some([255, 255, 255, 255]);
    let mut images = ImageMap::new();
    images.insert(id, image_data_from_rgba(img.into_raw(), w, h));

    let mut ctx = RenderContext::new();
    let device_id = pollster::block_on(ctx.device(None))
        .ok_or_else(|| anyhow!("no hay adaptador wgpu disponible"))?;
    let handle = &ctx.devices[device_id];

    let mut renderer = CanvasRenderer::new(&handle.device)?;
    let (rgba, bw, bh) = renderer.bake_page(&handle.device, &handle.queue, &doc, &images, 1.0)?;

    // Guardado atómico sobre el destino (que ya existe: es una sustitución).
    let before = std::fs::metadata(&target).map(|m| m.len()).unwrap_or(0);
    canvas_io::save_rgba(std::path::Path::new(&target), rgba, bw, bh)?;
    let after = std::fs::metadata(&target)?.len();

    // Reabre y comprueba.
    let saved = image::open(&target)?.to_rgba8();
    println!(
        "guardado OK: {target}\n  dims: {}x{} (esperadas {bw}x{bh})\n  bytes: {before} -> {after}",
        saved.width(),
        saved.height(),
    );
    anyhow::ensure!(saved.dimensions() == (bw, bh), "dimensiones inesperadas");
    // La esquina debe ser el fondo blanco (la imagen quedó al 50% centrada).
    let corner = saved.get_pixel(2, 2).0;
    anyhow::ensure!(
        corner == [255, 255, 255, 255],
        "la esquina debería ser fondo blanco, fue {corner:?}"
    );
    println!("  esquina = fondo blanco OK; edición visible en el archivo");
    Ok(())
}
