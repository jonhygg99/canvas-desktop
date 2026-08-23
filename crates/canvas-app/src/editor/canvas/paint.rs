//! Pintado: construir la escena vello con todas las ranuras visibles, mandarla
//! a la superficie offscreen, mostrar esa textura en egui, y superponer encima
//! el chrome de los lienzos, la rejilla, las reglas y los manejadores.
//!
//! Movido tal cual desde `canvas_ui`, en el mismo orden.

use canvas_core::{Document, LayerId};
use canvas_render::{CanvasRenderer, FxScope, ImageMap};
use eframe::egui;
use eframe::egui_wgpu::RenderState;
use vello::kurbo::Affine;

use crate::deck::{Deck, SlotContent};
use crate::surface::CanvasSurface;

use super::super::overlay::{draw_grid, draw_rulers, draw_selection_overlay};
use super::super::properties_panel::size_popup_ui;
use super::super::slot_chrome::{
    draw_add_zone, draw_header_tooltips, draw_rename_overlay, draw_slot_chrome,
};
use super::super::EditorState;
use super::CanvasAction;

#[allow(clippy::too_many_arguments)]
pub(super) fn paint(
    state: &mut EditorState,
    deck: &mut Deck,
    ui: &egui::Ui,
    rect: egui::Rect,
    slot_rect: egui::Rect,
    visible: &[usize],
    page_dims: (f64, f64),
    base_view: Affine,
    surface: &CanvasSurface,
    rs: &RenderState,
    renderer: &mut CanvasRenderer,
    action: &mut Option<CanvasAction>,
) {
    let mut scene = vello::Scene::new();
    for &idx in visible {
        let Some(slot) = deck.slots.get(idx) else {
            continue;
        };
        let scope = FxScope(slot.id);
        let view = base_view * Affine::translate(slot.rect.origin());
        if idx == deck.active {
            sync_and_append(
                &mut scene,
                renderer,
                rs,
                &state.doc,
                &state.images,
                scope,
                view,
            );
        } else if let SlotContent::Ready(doc) = &slot.content {
            sync_and_append(&mut scene, renderer, rs, &doc.doc, &doc.images, scope, view);
        }
    }
    if let Err(e) = surface.render(rs, renderer, &scene) {
        tracing::error!("fallo renderizando el lienzo: {e}");
    }

    ui.painter().image(
        surface.egui_id(),
        rect,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );

    for &idx in visible {
        draw_slot_chrome(state, deck, idx, ui, rect);
    }
    if !state.isolate {
        draw_add_zone(state, deck, ui, rect);
    }
    draw_header_tooltips(deck, ui, rect, &state.viewport, visible);

    if state.show_grid {
        draw_grid(state, ui, slot_rect, rect, page_dims);
    }
    draw_selection_overlay(state, ui, slot_rect, rect);
    if state.show_rulers {
        draw_rulers(state, ui, slot_rect, rect);
    }

    // Renombrado en curso desde una cabecera (si lo hay): `egui::Area` de
    // primer plano, PASO DE UI SEPARADO del `response` gigante de arriba —
    // ver la doc de `draw_rename_overlay`.
    if let Some(a) = draw_rename_overlay(state, deck, ui, rect) {
        *action = Some(a);
    }
    size_popup_ui(state, ui.ctx());
}

/// Sincroniza los efectos GPU de un documento y lo añade a `scene` con su
/// propia transformación de vista. Un `fn` normal, no un cierre: dentro del
/// bucle de `canvas_ui` se llama con `renderer`/`scene` prestados de forma
/// disjunta en cada iteración, y un cierre que capturase ambos a la vez
/// complicaría el préstamo sin necesidad.
#[allow(clippy::too_many_arguments)]
fn sync_and_append(
    scene: &mut vello::Scene,
    renderer: &mut CanvasRenderer,
    rs: &RenderState,
    doc: &Document,
    images: &ImageMap,
    scope: FxScope,
    view: Affine,
) {
    if let Ok(page) = doc.page() {
        let fx_targets: Vec<(LayerId, canvas_core::Effects)> =
            page.layers.iter().map(|l| (l.id, l.effects)).collect();
        for (id, effects) in fx_targets {
            if let Some(source) = images.get(&id) {
                renderer.sync_layer_effects(&rs.device, &rs.queue, scope, id, source, &effects);
            }
        }
    }
    let blurred = renderer.blur_overrides(scope);
    canvas_render::append_document(scene, doc, images, &blurred, view, true);
}
