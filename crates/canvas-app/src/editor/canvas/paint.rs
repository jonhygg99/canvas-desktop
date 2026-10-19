//! Pintado: construir la escena vello con todas las ranuras visibles, mandarla
//! a la superficie offscreen, mostrar esa textura en egui, y superponer encima
//! el chrome de los lienzos, la rejilla, las reglas y los manejadores.
//!
//! Movido tal cual desde `canvas_ui`, en el mismo orden.

use canvas_core::Document;
use canvas_render::{CanvasRenderer, FxScope, ImageMap};
use eframe::egui;
use eframe::egui_wgpu::RenderState;
use vello::kurbo::Affine;

use crate::deck::{free_ram_bytes, total_physical_ram_bytes, Deck, SlotContent};
use crate::surface::CanvasSurface;

use super::super::overlay::{draw_grid, draw_rulers, draw_selection_overlay};
use super::super::properties_panel::size_popup_ui;
use super::super::slot_chrome::{
    draw_add_zone, draw_header_tooltips, draw_rename_overlay, draw_slot_chrome,
};
use super::super::EditorState;
use super::CanvasAction;

/// Préstamos de render que `sync_and_append` necesita, agrupados para
/// reducir la firma de 7 a 5 parámetros (bajo el umbral de clippy).
struct RenderRefs<'a> {
    renderer: &'a mut CanvasRenderer,
    rs: &'a RenderState,
}

/// Geometría del frame actual que `paint` necesita, agrupada para no
/// arrastrar 6 parámetros sueltos.
pub(super) struct PaintGeometry<'a> {
    pub rect: egui::Rect,
    pub slot_rect: egui::Rect,
    pub visible: &'a [usize],
    pub page_dims: (f64, f64),
    pub base_view: Affine,
    pub surface: &'a mut CanvasSurface,
}

pub(super) fn paint(
    state: &mut EditorState,
    deck: &mut Deck,
    ui: &egui::Ui,
    geo: &mut PaintGeometry<'_>,
    rs: &RenderState,
    renderer: &mut CanvasRenderer,
    action: &mut Option<CanvasAction>,
) {
    // Cronómetro de la sonda `CANVAS_DEBUG_PERF` (ver `perf_probe`): dos
    // lecturas de reloj monótono por frame, sin efecto si está apagada.
    let timer = crate::perf_probe::Timer::start();
    let mut layers_synced = 0usize;
    // Reutiliza la `Scene` del surface entre frames (evita una allocación
    // por frame — el buffer interno de vello puede crecer a varios KB).
    // Se separa el préstamo mutable de `scene_mut` del inmutable de
    // `render`: se llena la escena en un bloque y luego se renderiza.
    let surface = &mut *geo.surface;
    {
        let scene = surface.scene_mut();
        // Ancla de atlas (ver `canvas_render::draw_atlas_anchor`): sin esto,
        // un frame donde NINGUNA ranura visible tenga imágenes (cámara entre
        // páginas, solo lienzos vectoriales a la vista) deja la escena sin
        // patches y vello recrea el atlas GPU vacío, blanqueando todas las
        // fotos. El `append_document` de cada ranura también la dibuja; aquí
        // se cubre además el caso de cero ranuras visibles.
        canvas_render::draw_atlas_anchor(scene);
        for &idx in geo.visible {
            let Some(slot) = deck.slots.get(idx) else {
                continue;
            };
            // El scope es único a nivel de proceso (no el `slot.id`, que
            // cada ventana reinicia en 1): el `CanvasRenderer` es compartido
            // y dos ventanas con los mismos scopes se pisarían la caché de
            // efectos cada frame.
            let scope = FxScope(slot.scope);
            let view = geo.base_view * Affine::translate(slot.rect.origin());
            let mut rref = RenderRefs { renderer, rs };
            if idx == deck.active {
                layers_synced += layer_count(&state.doc);
                sync_and_append(scene, &mut rref, &state.doc, &state.images, scope, view);
            } else if let SlotContent::Ready(doc) = &slot.content {
                layers_synced += layer_count(&doc.doc);
                sync_and_append(scene, &mut rref, &doc.doc, &doc.images, scope, view);
            }
        }
    }
    let encode_ms = timer.split();
    // Presupuesto GPU del documento activo (Task 6 del plan de memoria):
    // tras sincronizar las ranuras visibles, si la caché de efectos supera
    // el presupuesto (1/16 de RAM, reducido por RAM libre) se expulsan los
    // scopes menos usados que no sean el activo. Los visibles se
    // re-sincronizan cada frame (su `last_used` está fresco), así que solo
    // se expulsan inactivos — el atlas no se llena con lo que se ve, y un
    // scope expulsado reaparece al volver a sincronizarse. Inofensivo por
    // frame: `evict_fx_to_budget` vuelve sin tocar nada bajo presupuesto.
    if let Some(active_slot) = deck.slots.get(deck.active) {
        // El presupuesto se mide como mucho una vez por segundo: `free_ram_bytes`
        // es una syscall y esto corre en cada frame (ver
        // `surface::cached_render_budget`). El gate de guardado sigue midiendo
        // en vivo, en el momento del bake.
        let budget = crate::surface::cached_render_budget(ui.ctx(), "fx_ram_budget", || {
            canvas_render::resolve_fx_budget(total_physical_ram_bytes(), free_ram_bytes())
        });
        renderer.evict_fx_to_budget(budget, FxScope(active_slot.scope));
    }

    // Cortacircuitos GPU (ver `CanvasSurface`): con la GPU muerta no se le
    // envía más trabajo — se enseña la última textura buena y la barra de
    // estado avisa. Sin esto, un device enfermo recibe envíos cada frame
    // hasta que egui-wgpu aborta en su staging belt (crash del 2026-10-14).
    // La sonda periódica reintenta un render real cada ~5 s: si la GPU se
    // recuperó (hipo transitorio del driver), se reanuda solo.
    if surface.gpu_dead() {
        if surface.gpu_probe_due() {
            match surface.render(rs, renderer, surface.scene_ref()) {
                Ok(()) => surface.record_probe(true),
                Err(e) => {
                    tracing::debug!("sonda GPU tras el corte sigue fallando: {e}");
                    surface.record_probe(false);
                }
            }
        }
    } else {
        match surface.render(rs, renderer, surface.scene_ref()) {
            Ok(()) => surface.record_render(true),
            Err(e) => {
                tracing::error!("fallo renderizando el lienzo: {e}");
                surface.record_render(false);
            }
        }
    }

    let render_ms = timer.split() - encode_ms;
    crate::perf_probe::report(crate::perf_probe::FrameStats {
        total_ms: timer.split(),
        encode_ms,
        render_ms,
        visible_slots: geo.visible.len(),
        layers_synced,
        display_rebuilds: renderer.display_rebuilds(),
    });

    ui.painter().image(
        surface.egui_id(),
        geo.rect,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );

    for &idx in geo.visible {
        draw_slot_chrome(state, deck, idx, ui, geo.rect);
    }
    if !state.isolate {
        draw_add_zone(state, deck, ui, geo.rect);
    }
    draw_header_tooltips(deck, ui, geo.rect, &state.viewport, geo.visible);

    if state.show_grid {
        draw_grid(state, ui, geo.slot_rect, geo.rect, geo.page_dims);
    }
    draw_selection_overlay(state, ui, geo.slot_rect, geo.rect);
    super::super::inline_text::show(state, ui, geo.slot_rect, geo.rect);
    if let Some(id) = super::super::context_toolbar::show(state, ui, geo.slot_rect, geo.rect) {
        *action = Some(CanvasAction::ReplaceFromLocal(id));
    }
    if state.show_rulers {
        draw_rulers(state, ui, geo.slot_rect, geo.rect);
    }

    // Renombrado en curso desde una cabecera (si lo hay): `egui::Area` de
    // primer plano, PASO DE UI SEPARADO del `response` gigante de arriba —
    // ver la doc de `draw_rename_overlay`.
    if let Some(a) = draw_rename_overlay(state, deck, ui, geo.rect) {
        *action = Some(a);
    }
    size_popup_ui(state, ui.ctx());
}

/// Capas de un documento (solo para el desglose de la sonda de rendimiento:
/// `page()` no clona y `layers.len()` es O(1)).
fn layer_count(doc: &Document) -> usize {
    doc.page().map_or(0, |page| page.layers.len())
}

/// Sincroniza los efectos GPU de un documento y lo añade a `scene` con su
/// propia transformación de vista. Un `fn` normal, no un cierre: dentro del
/// bucle de `canvas_ui` se llama con `renderer`/`scene` prestados de forma
/// disjunta en cada iteración, y un cierre que capturase ambos a la vez
/// complicaría el préstamo sin necesidad.
///
/// Antes hacía `page.layers.iter().map(...).collect::<Vec<_>>()` para luego
/// iterar sobre ese `Vec` — una allocación por frame por slot visible que
/// no aportaba nada: se itera directamente sobre `page.layers` y se clona
/// solo el `Effects` (que es `Copy`) cuando hace falta.
fn sync_and_append(
    scene: &mut vello::Scene,
    rref: &mut RenderRefs<'_>,
    doc: &Document,
    images: &ImageMap,
    scope: FxScope,
    view: Affine,
) {
    if let Ok(page) = doc.page() {
        for layer in &page.layers {
            if let Some(source) = images.get(&layer.id) {
                rref.renderer.sync_layer_effects(
                    &rref.rs.device,
                    &rref.rs.queue,
                    scope,
                    layer.id,
                    source,
                    &layer.effects,
                );
            }
        }
    }
    let blurred = rref.renderer.blur_overrides(scope);
    canvas_render::append_document(scene, doc, images, &blurred, view, true);
}
