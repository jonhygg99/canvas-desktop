//! Verificación del fix de `FxScope` a nivel de atlas de vello: reproduce el
//! escenario reportado — DOS VENTANAS de editor sobre la misma carpeta, un
//! único `CanvasRenderer` compartido — intercalando frames de ambas y
//! EDITANDO en las dos, y cuenta las subidas al atlas por frame.
//!
//! Antes del fix, el scope derivaba del `slot.id` (cada baraja/ventana
//! reinicia su contador en 1): las dos ventanas usaban los MISMOS scopes y
//! cada frame se pisaban la caché de efectos → re-horneado y re-subida al
//! atlas CADA frame (el «spam»). Con el fix (scope global único por ranura),
//! el estado estacionario es 0 subidas y una edición re-subida SOLO en la
//! ventana editada, una sola vez.
//!
//! Uso: cargo run -p canvas-render --example verify_two_windows_atlas

use anyhow::{anyhow, Result};
use canvas_core::{Document, Effects, ImageContent, LayerContent, Transform};
use canvas_render::{append_document, image_data_from_rgba, CanvasRenderer, FxScope, ImageMap};
use vello::kurbo::Affine;
use vello::util::RenderContext;

/// Frames «idle» (sin editar) que se intercalan por ventana.
const IDLE_FRAMES: usize = 5;

/// Documento de una ventana: una capa de imagen a página completa con blur.
/// Devuelve también el id de la capa, para poder editarla después.
fn doc_with_color(
    w: u32,
    h: u32,
    rgb: [u8; 3],
    blur: f32,
) -> (canvas_core::LayerId, Document, ImageMap) {
    let mut doc = Document::new(f64::from(w), f64::from(h));
    let id = doc
        .add_layer(
            "img",
            Transform::new(0.0, 0.0, f64::from(w), f64::from(h)),
            LayerContent::Image(ImageContent {
                source_path: None,
                natural_width: w,
                natural_height: h,
                crop: None,
            }),
        )
        .unwrap();
    doc.layer_mut(id).unwrap().effects = Effects {
        blur_radius: blur,
        ..Default::default()
    };
    let pixels: Vec<u8> = (0..w * h)
        .flat_map(|_| [rgb[0], rgb[1], rgb[2], 255])
        .collect();
    let mut images = ImageMap::new();
    images.insert(id, image_data_from_rgba(pixels, w, h));
    (id, doc, images)
}

/// Un «frame» de una ventana: el mismo camino que `paint.rs` — sincroniza
/// los efectos GPU de todas las capas y renderiza la escena a la textura.
fn window_frame(
    renderer: &mut CanvasRenderer,
    device: &vello::wgpu::Device,
    queue: &vello::wgpu::Queue,
    scope: FxScope,
    doc: &Document,
    images: &ImageMap,
    target: &vello::wgpu::TextureView,
) {
    if let Ok(page) = doc.page() {
        for layer in &page.layers {
            if let Some(source) = images.get(&layer.id) {
                renderer.sync_layer_effects(device, queue, scope, layer.id, source, &layer.effects);
            }
        }
    }
    let blurred = renderer.blur_overrides(scope);
    let mut scene = vello::Scene::new();
    append_document(&mut scene, doc, images, &blurred, Affine::IDENTITY, true);
    let _ = renderer.render_to_texture(device, queue, &scene, target, 128, 128);
}

fn main() -> Result<()> {
    let mut ctx = RenderContext::new();
    let device_id = pollster::block_on(ctx.device(None))
        .ok_or_else(|| anyhow!("no hay adaptador wgpu disponible"))?;
    let handle = &ctx.devices[device_id];
    let (device, queue) = (&handle.device, &handle.queue);

    // Ventana A y ventana B sobre la misma carpeta: mismo tamaño y misma
    // capa, contenido distinto (rojo/azul) — como editar ranuras distintas.
    let (id_a, doc_a, images_a) = doc_with_color(128, 128, [200, 30, 30], 12.0);
    let (id_b, doc_b, images_b) = doc_with_color(128, 128, [30, 30, 200], 12.0);
    let mut doc_a = doc_a;
    let mut doc_b = doc_b;

    let target_a = CanvasRenderer::create_target_texture(device, 128, 128);
    let target_b = CanvasRenderer::create_target_texture(device, 128, 128);
    let view_a = target_a.create_view(&Default::default());
    let view_b = target_b.create_view(&Default::default());

    // ── CON el fix: scopes distintos por ventana ─────────────────────────
    let scope_a = FxScope(9001);
    let scope_b = FxScope(9002);
    let mut fixed = CanvasRenderer::new(device)?;
    println!("== CON FIX (scopes {scope_a:?} y {scope_b:?}): frames intercalados de A y B ==");
    for frame in 1..=IDLE_FRAMES {
        fixed.reset_atlas_stats();
        window_frame(
            &mut fixed, device, queue, scope_a, &doc_a, &images_a, &view_a,
        );
        window_frame(
            &mut fixed, device, queue, scope_b, &doc_b, &images_b, &view_b,
        );
        let s = fixed.atlas_stats();
        println!(
            "  frame {frame}: registros_nuevos={} re_subidas={}",
            s.registrations, s.reuploads
        );
        if frame == 1 {
            assert_eq!(
                (s.registrations, s.reuploads),
                (2, 2),
                "frame 1: cada ventana registra y hornea su textura una vez"
            );
        } else {
            assert_eq!(
                (s.registrations, s.reuploads),
                (0, 0),
                "frame {frame}: nadie edita → 0 subidas al atlas"
            );
        }
    }

    // Editar en AMBAS ventanas (mismo frame): una re-subida por ventana
    // editada, y ninguna para la que no se tocó.
    fixed.reset_atlas_stats();
    doc_a.layer_mut(id_a).unwrap().effects.blur_radius = 30.0;
    doc_b.layer_mut(id_b).unwrap().effects.blur_radius = 24.0;
    window_frame(
        &mut fixed, device, queue, scope_a, &doc_a, &images_a, &view_a,
    );
    window_frame(
        &mut fixed, device, queue, scope_b, &doc_b, &images_b, &view_b,
    );
    let s = fixed.atlas_stats();
    println!(
        "  frame edición (A y B): registros_nuevos={} re_subidas={} (1 por ventana editada, no más)",
        s.registrations, s.reuploads
    );
    assert_eq!(
        s.reuploads, 2,
        "una edición = una re-subida por ventana editada"
    );
    assert_eq!(s.registrations, 0, "editar no registra texturas nuevas");

    // El frame siguiente, sin editar: 0/0. El spam del bug era CADA frame.
    fixed.reset_atlas_stats();
    window_frame(
        &mut fixed, device, queue, scope_a, &doc_a, &images_a, &view_a,
    );
    window_frame(
        &mut fixed, device, queue, scope_b, &doc_b, &images_b, &view_b,
    );
    let s = fixed.atlas_stats();
    println!(
        "  frame post-edición: registros_nuevos={} re_subidas={}",
        s.registrations, s.reuploads
    );
    assert_eq!(
        (s.registrations, s.reuploads),
        (0, 0),
        "tras editar, el siguiente frame no re-subió nada (la caché está al día)"
    );
    println!("OK con el fix: el spam de subidas al atlas cesó.\n");

    // ── SIN el fix: mismo scope para ambas ventanas (el bug) ─────────────
    let shared = FxScope(7); // antes: FxScope(slot.id) colisionaba entre ventanas
    let mut buggy = CanvasRenderer::new(device)?;
    println!("== SIN FIX (mismo scope {shared:?} para A y B — el bug) ==");
    for frame in 1..=IDLE_FRAMES {
        buggy.reset_atlas_stats();
        window_frame(
            &mut buggy, device, queue, shared, &doc_a, &images_a, &view_a,
        );
        window_frame(
            &mut buggy, device, queue, shared, &doc_b, &images_b, &view_b,
        );
        let s = buggy.atlas_stats();
        println!(
            "  frame {frame}: registros_nuevos={} re_subidas={}",
            s.registrations, s.reuploads
        );
        if frame > 1 {
            assert!(
                s.reuploads >= 2,
                "sin fix, ambas ventanas re-suben CADA frame (spam)"
            );
        }
    }
    println!(
        "\nConclusión: con scopes colisionando hay re-subidas al atlas en CADA frame \
         (spam); con el fix, solo en el frame de la edición y una vez por ventana."
    );
    Ok(())
}
