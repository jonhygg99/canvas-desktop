//! Verificación headless del hot path del lienzo (fase de rendimiento del
//! plan «lienzo extremadamente lento»): mide, con el renderer real sobre un
//! device wgpu headless, cuánto cuesta por frame
//!
//! 1. sincronizar los efectos de una capa grande SIN efectos (foto de 12 MP),
//! 2. codificar la escena de ese documento (lo que cuesta cada lienzo visible
//!    por frame, la pregunta de fondo de la tarea de los vecinos),
//! 3. la copia reducida que el bug materializaba en CADA frame y tiraba
//!    (`imageops::thumbnail` sobre el RGBA completo).
//!
//! El contrato que se comprueba: la copia reducida se materializa UNA vez
//! (`display_rebuilds == 1`), los frames en reposo no copian píxeles y su
//! coste es despreciable frente a codificar la escena.
//!
//! Uso: cargo run -p canvas-render --example fx_hotpath_probe
//!
//! Números de referencia (portátil Windows, foto sintética de 4032x3024):
//!
//! ```text
//! antes: cada frame y capa pagaba 103 ms de copia reducida (y la tiraba)
//! después: primer frame 111,26 ms (paga la reducción, 1 vez)
//!          frame en reposo 0,000 ms | codificar la escena 0,001 ms
//!          copias reducidas materializadas: 1
//! ```
//!
//! En la app real (`CANVAS_DEBUG_PERF=1`, PNG de 3000x2000): `frame=1
//! total_ms=82,15 encode_ms=75,86 render_ms=6,29 display_rebuilds=1`.

use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use canvas_core::{Document, ImageContent, LayerContent, Transform};
use canvas_render::{image_data_from_rgba, CanvasRenderer, FxScope, ImageMap};
use vello::kurbo::Affine;
use vello::util::RenderContext;

/// Foto típica de móvil: supera `MAX_FX_DIM` (2048), así que necesita copia
/// reducida. ~49 MB en RGBA.
const PHOTO: (u32, u32) = (4032, 3024);
/// Frames simulados para promediar.
const FRAMES: usize = 60;
/// Cota de reposo por frame: sincronizar sin cambios no puede costar más que
/// esto (medido en cientos de microsegundos; el bug costaba decenas de ms).
const IDLE_BUDGET: Duration = Duration::from_millis(2);

fn ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

/// Patrón determinista (no importa su contenido, solo el tamaño real).
fn photo_rgba(w: u32, h: u32) -> Vec<u8> {
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            rgba.extend_from_slice(&[(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8, 255]);
        }
    }
    rgba
}

fn make_doc(w: u32, h: u32, rgba: Vec<u8>) -> Result<(Document, ImageMap)> {
    let mut doc = Document::new(f64::from(w), f64::from(h));
    let id = doc.add_layer(
        "foto",
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
    Ok((doc, images))
}

/// Lo que hace `paint` por cada capa visible: sincronizar sus efectos GPU.
fn sync_effects(
    renderer: &mut CanvasRenderer,
    device: &vello::wgpu::Device,
    queue: &vello::wgpu::Queue,
    doc: &Document,
    images: &ImageMap,
) -> Result<()> {
    for layer in &doc.page()?.layers {
        if let Some(source) = images.get(&layer.id) {
            renderer.sync_layer_effects(
                device,
                queue,
                FxScope::default(),
                layer.id,
                source,
                &layer.effects,
            );
        }
    }
    Ok(())
}

fn main() -> Result<()> {
    let (w, h) = PHOTO;
    let rgba = photo_rgba(w, h);
    let (doc, images) = make_doc(w, h, rgba.clone())?;

    let mut ctx = RenderContext::new();
    let device_id = pollster::block_on(ctx.device(None))
        .ok_or_else(|| anyhow!("no hay adaptador wgpu disponible"))?;
    let handle = &ctx.devices[device_id];
    let (device, queue) = (&handle.device, &handle.queue);
    let mut renderer = CanvasRenderer::new(device)?;

    // 1) Primer frame: aquí SÍ toca materializar la copia reducida de la foto.
    let first = Instant::now();
    sync_effects(&mut renderer, device, queue, &doc, &images)?;
    let first_frame = first.elapsed();

    // 2) Frames en reposo: el mismo documento, sin editar. Antes del fix cada
    //    uno copiaba y reducía los ~49 MB de la foto y tiraba el resultado.
    let idle = Instant::now();
    for _ in 0..FRAMES {
        sync_effects(&mut renderer, device, queue, &doc, &images)?;
    }
    let idle_frame = idle.elapsed() / FRAMES as u32;

    // 3) Codificar la escena (el coste de un lienzo visible por frame).
    let blurred = renderer.blur_overrides(FxScope::default());
    let mut scene = vello::Scene::new();
    let view = Affine::scale(0.25);
    let encode = Instant::now();
    for _ in 0..FRAMES {
        scene.reset();
        canvas_render::append_document(&mut scene, &doc, &images, &blurred, view, true);
    }
    let encode_frame = encode.elapsed() / FRAMES as u32;

    // 4) La copia reducida (lo que el bug hacía por frame y descartaba).
    let capping = Instant::now();
    let photo = image::RgbaImage::from_raw(w, h, rgba)
        .ok_or_else(|| anyhow!("buffer RGBA inesperado para la foto sintética"))?;
    let scaled = image::imageops::thumbnail(&photo, 2048, 1536);
    let capping_cost = capping.elapsed();

    println!(
        "foto {w}×{h} ({:.1} MB RGBA) + {} capa(s)\n\
         primer frame (con copia reducida): {:.2} ms\n\
         frame en reposo (60 frames):      {:.3} ms/frame\n\
         codificar escena (60 frames):     {:.3} ms/frame\n\
         copia reducida suelta:            {:.2} ms ({scaled_width}×{scaled_height})\n\
         copias reducidas materializadas:  {} (debe ser 1)",
        rgba_len_mb(w, h),
        doc.page()?.layers.len(),
        ms(first_frame),
        ms(idle_frame),
        ms(encode_frame),
        ms(capping_cost),
        renderer.display_rebuilds(),
        scaled_width = scaled.width(),
        scaled_height = scaled.height(),
    );

    anyhow::ensure!(
        renderer.display_rebuilds() == 1,
        "la copia reducida se materializó {} veces: el hot path vuelve a copiar \
         píxeles en cada frame (regresión del lienzo lento)",
        renderer.display_rebuilds()
    );
    anyhow::ensure!(
        idle_frame < IDLE_BUDGET,
        "un frame en reposo tarda {:.3} ms en sincronizar (tope {:.0} ms): \
         sigue habiendo trabajo de píxeles por frame",
        ms(idle_frame),
        ms(IDLE_BUDGET)
    );
    println!("\nOK: en reposo no se copian píxeles; la copia reducida se paga una sola vez.");
    Ok(())
}

fn rgba_len_mb(w: u32, h: u32) -> f64 {
    f64::from(w) * f64::from(h) * 4.0 / (1024.0 * 1024.0)
}
