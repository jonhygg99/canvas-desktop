//! El lienzo activo: menu contextual, render (GPU vello + chrome de los
//! lienzos vecinos), hit-testing de la baraja/cabeceras, y el popup de
//! "Replace from URL". `canvas_ui` es el orquestador: llama a los submodulos
//! EN EL MISMO ORDEN en que estaban sus bloques dentro de la funcion original
//! - ese orden es significativo y no debe cambiarse al tocar este archivo.

use std::sync::mpsc::Sender;

use canvas_core::LayerId;
use canvas_render::CanvasRenderer;
use eframe::egui;
use eframe::egui_wgpu::RenderState;
use vello::kurbo::Affine;

use crate::deck::{Deck, DeckRect, SlotContent};
use crate::loader::{self, AppMsg};
use crate::surface::CanvasSurface;

use super::interaction::layer_interaction;
use super::properties_panel::size_popup_ui;
use super::viewport::screen_to_page;
use super::EditorState;

mod camera;
mod context_menu;
mod paint;
mod picking;
mod url_popup;

use camera::{apply_camera, Camera};
use context_menu::canvas_context_menu;
use paint::paint;
use picking::handle_press;
use url_popup::replace_url_popup_ui;

/// Acción pedida desde la cabecera de un lienzo (área central) que necesita
/// tocar disco (duplicar/borrar) o reconciliarse con el nombre real del
/// archivo (renombrar) — `canvas_ui` la arma pero no la ejecuta; se resuelve
/// en `main.rs`, mismo espíritu que `StripAction` desde la tira.
pub enum CanvasAction {
    Rename(u64, String),
    Duplicate(u64),
    Delete(u64),
    ReplaceFromLocal(LayerId),
    ReplaceFromUrl(LayerId, String),
    /// Elegido en el menú contextual (clic derecho) del propio lienzo —
    /// reutiliza el mismo `MenuAction` que ya resuelve la barra de menú
    /// nativa/de respaldo, sin duplicar esa lógica.
    Menu(crate::menus::MenuAction),
}

/// El lienzo: gestiona zoom/paneo, carga perezosa/descarte de la baraja, y
/// renderiza en una sola escena todos los lienzos visibles (el activo con
/// `state.doc`/`state.images`; el resto con su propio `SlotDoc`).
#[allow(clippy::too_many_arguments)]
pub fn canvas_ui(
    state: &mut EditorState,
    deck: &mut Deck,
    ui: &mut egui::Ui,
    rs: &RenderState,
    renderer: &mut CanvasRenderer,
    surface_slot: &mut Option<CanvasSurface>,
    tx: &Sender<AppMsg>,
    // Extensión de `settings.new_canvas_format` — qué crea la zona "+" al
    // final de la baraja cuando se pulsa directamente sobre el lienzo.
    new_canvas_ext: &str,
    sidecar_default: bool,
) -> Option<CanvasAction> {
    // Duplicar/borrar/renombrar tocan disco o el watcher: se arman aquí (en
    // la cabecera de un lienzo, ver más abajo) pero se resuelven en
    // `main.rs`, igual que `StripAction` desde la tira.
    let mut action: Option<CanvasAction> = None;

    let avail = ui.available_size();
    let (rect, response) = ui.allocate_exact_size(avail, egui::Sense::click_and_drag());

    // Menú contextual (clic derecho): antes no había ninguno en el área de
    // edición. Solo las acciones que de verdad se usan desde un clic
    // derecho — no una copia entera del menú Edit (eso ya está a un atajo
    // de teclado o al menú superior de distancia).
    response.context_menu(|ui| canvas_context_menu(ui, state, &mut action));

    if action.is_none() {
        action = replace_url_popup_ui(state, ui.ctx());
    }

    if rect.width() < 1.0 || rect.height() < 1.0 {
        return action;
    }

    let page_dims = match state.doc.page() {
        Ok(p) => (p.width, p.height),
        Err(_) => (1.0, 1.0),
    };

    // La ranura activa siempre conoce su tamaño real (es el documento que se
    // está editando; no hace falta esperar a `DeckProbed`) — mantenerla al
    // día aquí cubre tanto la primera carga como un cambio de tamaño de
    // página desde el panel, sin ningún caso especial.
    let mut sizes_changed = false;
    if let Some(slot) = deck.slots.get_mut(deck.active) {
        if slot.page != Some(page_dims) {
            slot.page = Some(page_dims);
            sizes_changed = true;
        }
    }
    // Defensa en profundidad: cualquier ranura YA cargada (no solo la
    // activa) conoce su tamaño real por su propio documento, sin depender
    // de que `DeckProbed` haya llegado ni de que el sondeo funcione para su
    // formato. Sin esto, un lienzo mayor que la estimación de `Slot::size()`
    // se pinta fuera de su `rect` de layout y se come el hueco con el
    // vecino. Acumulador local porque no se puede escribir
    // `deck.layout_dirty` mientras `&mut deck.slots` sigue prestado por el
    // bucle.
    for slot in &mut deck.slots {
        if let SlotContent::Ready(d) = &slot.content {
            if let Ok(p) = d.doc.page() {
                let real = (p.width, p.height);
                if slot.page != Some(real) {
                    slot.page = Some(real);
                    sizes_changed = true;
                }
            }
        }
    }
    deck.layout_dirty |= sizes_changed;
    if deck.layout_dirty {
        // Recolocar puede desplazar el origen del lienzo ACTIVO como efecto
        // secundario (el centrado en el eje transversal usa el máximo
        // ancho/alto de TODAS las ranuras: aprender el tamaño real de un
        // vecino cambia ese máximo). Compensar el pan para que el lienzo
        // activo se quede clavado en pantalla — el usuario no pidió mover
        // nada. No aplica en el primer frame: `needs_fit`/`AutoFit` van a
        // reescribir el pan entero de todos modos, y no-op con una sola
        // ranura (su origen activo siempre es `(0,0)`).
        let before = deck.active_origin();
        deck.relayout();
        if !state.viewport.needs_fit {
            let after = deck.active_origin();
            let (dx, dy) = (after.0 - before.0, after.1 - before.1);
            if dx != 0.0 || dy != 0.0 {
                state.viewport.pan -= egui::vec2(
                    (dx * state.viewport.zoom) as f32,
                    (dy * state.viewport.zoom) as f32,
                );
            }
        }
    }

    let Camera {
        panning,
        space_down,
    } = apply_camera(state, deck, ui, rect, &response);

    // Origen del lienzo activo en el espacio de baraja, en puntos de
    // pantalla. `slot_rect` es `rect` desplazado ese origen — pasado en vez
    // de `rect` a `layer_interaction` y a los cuatro ayudantes de
    // coordenadas (`page_to_screen` y compañía, más abajo), consigue que un
    // lienzo que no esté en el origen de la baraja se seleccione/arrastre/
    // redimensione igual que si fuera el único, sin tocar ni una línea de
    // esa lógica.
    let (origin_x, origin_y) = deck.active_origin();
    let slot_offset = egui::vec2(
        (origin_x * state.viewport.zoom) as f32,
        (origin_y * state.viewport.zoom) as f32,
    );
    let slot_rect = egui::Rect::from_min_size(rect.min + slot_offset, rect.size());

    // Qué ranuras están a la vista (con margen de precarga), y sella cuándo
    // se vieron por última vez (política de descarte LRU). Calculado AQUÍ
    // (antes de resolver la pulsación) porque el hit-test de la cabecera de
    // cada lienzo, más abajo, solo tiene sentido sobre ranuras visibles —
    // el resto del uso de `visible` (carga perezosa, descarte, escena,
    // `draw_slot_chrome`) sigue más adelante sin cambios, solo reutiliza
    // este mismo cálculo en vez de repetirlo.
    let (x0, y0) = screen_to_page(&state.viewport, rect, rect.min);
    let (x1, y1) = screen_to_page(&state.viewport, rect, rect.max);
    let view_deck_rect = Deck::dilate(DeckRect {
        x: x0,
        y: y0,
        w: x1 - x0,
        h: y1 - y0,
    });
    let visible = if state.isolate {
        vec![deck.active]
    } else {
        deck.visible_indices(view_deck_rect)
    };
    deck.mark_visible(&visible);

    handle_press(
        state,
        deck,
        ui,
        rect,
        &response,
        &visible,
        space_down,
        new_canvas_ext,
        rs,
        &mut action,
    );

    // Selección, arrastre y redimensionado (si no se está paneando, el
    // gesto en curso no pertenece a la baraja, y el diseño activo no está
    // bloqueado — `Slot::locked`, cabecera del lienzo).
    let active_locked = deck.slots.get(deck.active).is_some_and(|s| s.locked);
    if !panning && !space_down && !state.press_on_other_slot && !active_locked {
        layer_interaction(state, ui, &response, slot_rect);
    }

    // La marca dura el gesto entero (pulsar, arrastrar, soltar) y se limpia
    // cuando el botón ya no está pulsado — DESPUÉS de la guardia de arriba,
    // para que el frame en que se suelta (donde egui emite `clicked`/
    // `drag_stopped`) siga protegido.
    if !ui.input(|i| i.pointer.primary_down()) {
        state.press_on_other_slot = false;
    }

    // Render vello → textura del tamaño físico del lienzo.
    let ppp = ui.ctx().pixels_per_point();
    let width = (rect.width() * ppp).round().max(1.0) as u32;
    let height = (rect.height() * ppp).round().max(1.0) as u32;
    let surface = CanvasSurface::ensure(surface_slot, rs, width, height);

    // Transformación del espacio de BARAJA a píxeles físicos del lienzo, sin
    // desplazar por ninguna ranura en particular: cada lienzo visible añade
    // su propio origen antes de renderizarse (más abajo).
    let base_view = Affine::translate((
        f64::from(state.viewport.pan.x * ppp),
        f64::from(state.viewport.pan.y * ppp),
    )) * Affine::scale(state.viewport.zoom * f64::from(ppp));

    // Carga perezosa: pide las ranuras `Idle` visibles (o del radio de
    // precarga alrededor de la activa) que quepan en el presupuesto de
    // cargas en vuelo.
    if let Some(folder) = deck.folder.clone() {
        for path in deck.request_loads(&visible) {
            loader::spawn_load_slot(
                folder.clone(),
                path,
                deck.generation(),
                sidecar_default,
                tx.clone(),
                ui.ctx().clone(),
            );
        }
    }
    // Descarte: libera memoria de ranuras lejanas, limpias y sin guardado en
    // curso, por encima del presupuesto.
    for scope in deck.evict() {
        renderer.forget_scope(scope);
    }

    // Una sola escena para todos los lienzos visibles ya cargados (activo o
    // `Ready`); el resto se pinta encima como miniatura/placeholder, con el
    // `Painter` normal de egui (más abajo, en `draw_slot_chrome`) — ya están
    // en GPU desde la galería, no hace falta subirlas de nuevo a vello.

    paint(
        state,
        deck,
        ui,
        rect,
        slot_rect,
        &visible,
        page_dims,
        base_view,
        surface,
        rs,
        renderer,
        &mut action,
    );

    size_popup_ui(state, ui.ctx());
    action
}
