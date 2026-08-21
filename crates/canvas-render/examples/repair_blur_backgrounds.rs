//! Herramienta puntual de reparación: recorre una carpeta y, para cada
//! `.png` con sidecar (`.canvas/foto.png.canvas`), vuelve a hornear su
//! página con un `CanvasRenderer` nuevo (sin caché compartida entre
//! archivos) y la reemplaza atómicamente. Corrige el bug donde el fondo
//! desenfocado horneado en el PNG se tomaba de otra foto guardada antes en
//! la misma sesión (ver el fix en `canvas-app::start_save` /
//! `renderer.forget_scope`) — este script arregla los archivos que ya se
//! habían guardado ANTES de ese fix, sin necesidad de reabrir cada uno en
//! la app.
//!
//! Solo toca capas ya existentes (el documento y sus imágenes por capa NO
//! cambian, únicamente el raster final); reescribe el sidecar con el mismo
//! documento pero el hash actualizado a los bytes nuevos, para que la app no
//! lo marque luego como "cambiado por fuera".
//!
//! Se salta cualquier archivo sin sidecar o cuyo hash no coincida ya
//! (cambiado por fuera desde el último guardado): no hay nada seguro que
//! rehornear en esos casos.
//!
//! Uso: cargo run -p canvas-render --example repair_blur_backgrounds -- <carpeta>

use anyhow::{anyhow, Context, Result};
use canvas_io::{CanvasPayload, LayerPixels};
use canvas_render::{image_data_from_rgba, CanvasRenderer, FxScope, ImageMap};
use std::path::Path;
use vello::util::RenderContext;

fn main() -> Result<()> {
    let folder = std::env::args()
        .nth(1)
        .context("falta la ruta de la carpeta")?;
    let folder = Path::new(&folder);

    let mut ctx = RenderContext::new();
    let device_id = pollster::block_on(ctx.device(None))
        .ok_or_else(|| anyhow!("no hay adaptador wgpu disponible"))?;
    let handle = &ctx.devices[device_id];
    let (device, queue) = (&handle.device, &handle.queue);

    let mut pngs: Vec<_> = std::fs::read_dir(folder)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("png"))
        })
        .collect();
    pngs.sort();

    println!("{} PNG encontrados en {}", pngs.len(), folder.display());

    let mut fixed = 0;
    let mut skipped = 0;
    for path in &pngs {
        match repair_one(device, queue, path) {
            Ok(true) => {
                println!("  [OK]      {}", path.display());
                fixed += 1;
            }
            Ok(false) => {
                println!("  [omitido] {}", path.display());
                skipped += 1;
            }
            Err(e) => {
                println!("  [ERROR]   {}: {e}", path.display());
                skipped += 1;
            }
        }
    }
    println!("listo: {fixed} corregidos, {skipped} omitidos");
    Ok(())
}

fn repair_one(
    device: &vello::wgpu::Device,
    queue: &vello::wgpu::Queue,
    path: &Path,
) -> Result<bool> {
    let Some(restored) = canvas_io::read_sidecar(path)? else {
        return Ok(false); // sin sidecar: nada que rehornear con seguridad.
    };
    if !restored.hash_matches {
        return Ok(false); // cambió por fuera desde el último guardado.
    }

    let mut images = ImageMap::new();
    for (raw_id, img) in &restored.images {
        let id = canvas_core::LayerId::from_raw(*raw_id);
        images.insert(
            id,
            image_data_from_rgba(img.rgba.clone(), img.width, img.height),
        );
    }

    // Renderer NUEVO por archivo: cero estado compartido entre fotos, así
    // que no hay ninguna caché que pueda arrastrar la textura de otra.
    let mut renderer = CanvasRenderer::new(device)?;
    let (rgba, width, height) = renderer.bake_page(
        device,
        queue,
        FxScope::default(),
        &restored.document,
        &images,
        1.0,
    )?;

    let metadata = canvas_io::extract_metadata_from_file(path);
    let meta_opt = (!metadata.is_empty()).then_some(&metadata);
    let new_bytes = canvas_io::save_rgba(path, rgba, width, height, 100, meta_opt)?;

    let sidecar_images: Vec<LayerPixels> = restored
        .images
        .iter()
        .map(|(id, img)| (*id, img.rgba.clone(), img.width, img.height))
        .collect();
    canvas_io::write_sidecar(
        path,
        &new_bytes,
        &CanvasPayload {
            document: restored.document,
            images: sidecar_images,
            background_layer: restored.background_layer,
            preview: None,
        },
    )?;

    Ok(true)
}
