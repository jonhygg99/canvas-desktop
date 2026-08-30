//! Recuperación puntual de un `.canvas` contaminado durante una verificación.
//!
//! El flujo de trabajo: mientras se probaba el editor se pegaron 5 imágenes
//! de prueba sobre `1.png` (una foto real de una carpeta de galería) y se
//! guardó. El sidecar v5 quedó con las capas originales INTACTAS — la foto
//! nítida (layer id 1) y el `Blurred background` (layer id 2) — más las 5
//! capas `Pasted Image` (ids 3..=7) añadidas por la prueba. La única
//! contaminación es ese rango de capas y sus blobs.
//!
//! Este ejemplo lee el sidecar, elimina del DOCUMENTO cualquier capa cuyo id
//! caiga en el rango contaminado (`3`..=`7`), reconstruye el payload limpio
//! conservando las imágenes de las capas 1 y 2, y vuelve a hornear el PNG a
//! página completa con un `CanvasRenderer` real (desenfoque no destructivo
//! aplicado de verdad). Replica el diseño original: capa 1 = foto nítida
//! sobre la página, capa 2 = fondo desenfocado cubriéndola (blur 50).
//!
//! Uso: cargo run -p canvas-render --example recover_design -- <foto.png> <salida.png>

use anyhow::{anyhow, Context, Result};
use canvas_core::{
    contain_transform, cover_transform, Document, ImageContent, Layer, LayerContent, LayerId,
};
use canvas_io::{CanvasPayload, LayerPixels};
use canvas_render::{image_data_from_rgba, CanvasRenderer, FxScope, ImageMap};
use std::path::Path;
use vello::util::RenderContext;

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let src = args.next().context("falta la ruta del PNG")?;
    let out = args
        .next()
        .unwrap_or_else(|| format!("{}.recuperado.png", src.trim_end_matches(".png")));

    let Some(restored) = canvas_io::read_sidecar(Path::new(&src))? else {
        return Err(anyhow!("no hay sidecar para {}", src));
    };

    // El sidecar contaminado guardó la foto nítida en `images` (capa 1) pero
    // NO como capa en `page.layers` (solo quedaron el blur, capa 2, y las 5
    // capas rojas de la prueba, 3..=7). Para recuperar el diseño original
    // hay que reconstruir el DOCUMENTO: capa 1 = foto nítida (contain sobre
    // la página) + capa 2 = fondo desenfocado (cover, blur 50) DEBAJO.

    // Localiza las imágenes por id: 1 = foto nítida, 2 = blur.
    let at = |id: u64| {
        restored
            .images
            .iter()
            .find(|(i, _)| *i == id)
            .map(|(_, l)| l)
    };
    let photo = at(1).context("falta la imagen de la capa 1")?;
    let blurp = at(2).context("falta la imagen de la capa 2")?;

    let page_size = restored
        .document
        .page()
        .map(|p| (p.width, p.height))
        .unwrap_or((1920.0, 1080.0));
    let (pw, ph) = (page_size.0, page_size.1);

    // Imitación exacta de `set_blurred_background` + `add_image_layer` sobre
    // lienzo vacío: la foto como capa 1 en el centro (contain) y la MISMA
    // foto a página entera (cover) con blur 50 como capa 2 en el fondo de la
    // pila. `add_layer` asigna id 1 a la foto; el blur usa id 2 con from_raw.
    let mut doc = Document::new(pw, ph);
    if let Ok(page) = doc.page_mut() {
        page.background = Some([255, 255, 255, 255]);
    }
    doc.add_layer(
        "1.png",
        contain_transform(f64::from(photo.width), f64::from(photo.height), pw, ph),
        LayerContent::Image(ImageContent {
            source_path: None,
            natural_width: photo.width,
            natural_height: photo.height,
            crop: None,
        }),
    )
    .unwrap();
    let mut bg = Layer::new(
        LayerId::from_raw(2),
        "Blurred background",
        cover_transform(f64::from(blurp.width), f64::from(blurp.height), pw, ph),
        LayerContent::Image(ImageContent {
            source_path: None,
            natural_width: blurp.width,
            natural_height: blurp.height,
            crop: None,
        }),
    );
    bg.effects.blur_radius = 50.0;
    if let Ok(page) = doc.page_mut() {
        page.layers.insert(0, bg); // el blur queda DEBAJO de la foto nítida
    }

    let mut images = ImageMap::new();
    let mut sidecar_images: Vec<LayerPixels> = Vec::new();
    // Capa 1 (foto): id 1. Capa 2 (blur): id 2.
    images.insert(
        LayerId::from_raw(1),
        image_data_from_rgba(photo.rgba.clone(), photo.width, photo.height),
    );
    sidecar_images.push((1, photo.rgba.clone(), photo.width, photo.height));
    images.insert(
        LayerId::from_raw(2),
        image_data_from_rgba(blurp.rgba.clone(), blurp.width, blurp.height),
    );
    sidecar_images.push((2, blurp.rgba.clone(), blurp.width, blurp.height));

    let mut ctx = RenderContext::new();
    let device_id = pollster::block_on(ctx.device(None))
        .ok_or_else(|| anyhow!("no hay adaptador wgpu disponible"))?;
    let handle = &ctx.devices[device_id];
    let (device, queue) = (&handle.device, &handle.queue);

    // Renderer NUEVO: cero estado compartido, sin caché que arrastre basura.
    let mut renderer = CanvasRenderer::new(device)?;
    let (rgba, width, height) =
        renderer.bake_page(device, queue, FxScope::default(), &doc, &images, 1.0)?;

    let metadata = canvas_io::extract_metadata_from_file(Path::new(&src));
    let meta_opt = (!metadata.is_empty()).then_some(&metadata);
    let new_bytes = canvas_io::save_rgba(Path::new(&out), rgba, width, height, 100, meta_opt)?;

    // Escribe el sidecar limpio JUNTO a la salida (mismo documento y blobs
    // restantes, hash actualizado a los bytes nuevos).
    if let Some(parent) = Path::new(&out).parent() {
        let dir = parent.join(".canvas");
        std::fs::create_dir_all(&dir).ok();
        canvas_io::write_sidecar(
            Path::new(&out),
            &new_bytes,
            &CanvasPayload {
                document: doc,
                images: sidecar_images,
                background_layer: Some(2), // el blur, como en el sidecar real
                preview: None,
            },
        )?;
    }

    println!(
        "recuperado: {} ({}x{}) — capa foto (contain) + capa blur (cover, blur 50)",
        out, width, height
    );
    Ok(())
}
