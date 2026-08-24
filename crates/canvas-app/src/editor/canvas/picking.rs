//! Que pasa cuando se pulsa el boton primario sobre el area de edicion:
//! cabecera de un lienzo (renombrar/duplicar/borrar/bloquear/aislar), zona «+»
//! del final de la baraja, o salto a otro lienzo visible.
//!
//! Movido tal cual desde `canvas_ui`, en el mismo orden.

use eframe::egui;
use eframe::egui_wgpu::RenderState;

use crate::deck::{Deck, MoveDir};

use super::super::slot_chrome::slot_header_layout;
use super::super::viewport::{page_to_screen, screen_to_page};
use super::super::EditorState;
use super::CanvasAction;

#[allow(clippy::too_many_arguments)]
pub(super) fn handle_press(
    state: &mut EditorState,
    deck: &mut Deck,
    ui: &egui::Ui,
    rect: egui::Rect,
    response: &egui::Response,
    visible: &[usize],
    space_down: bool,
    new_canvas_ext: &str,
    _rs: &RenderState,
    action: &mut Option<CanvasAction>,
) {
    // Pulsación sobre un lienzo que no es el activo: lo activa (el
    // intercambio en sí lo aplica `deck::apply_jump`, fuera de este módulo,
    // para no mutar el documento activo a mitad de este mismo frame). Se
    // decide en el frame de la PULSACIÓN, no en el de soltar limpio
    // (`response.clicked()`, que solo se cumple si el puntero no se movió
    // más allá del umbral de arrastre de egui entre pulsar y soltar — un
    // clic humano real casi nunca es tan quieto). Cuando egui clasifica esa
    // pulsación como arrastre en vez de clic, `clicked()` no llega nunca, y
    // `layer_interaction` SÍ corría: como usa `slot_rect` (siempre el
    // espacio de la ranura ACTIVA), un clic con el más mínimo temblor sobre
    // OTRO lienzo agarraba y movía una capa del documento activo — la
    // «Position X» que cambiaba sola en el panel de propiedades sin que el
    // usuario tocase ninguna capa.
    if ui.input(|i| i.pointer.primary_pressed()) && !space_down && response.contains_pointer() {
        if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
            // Cabecera de CUALQUIER lienzo visible (activo o no) se
            // comprueba PRIMERO, en espacio de pantalla — antes del hit-test
            // en espacio de página de más abajo, que solo conoce el cuerpo
            // del lienzo, no su cabecera (que vive por encima, fuera de su
            // `DeckRect`). Un acierto aquí consume la pulsación entera: no
            // cae al hit-test de activación ni a `layer_interaction`.
            let mut header_hit = false;
            for &idx in visible {
                if header_hit {
                    break;
                }
                let Some((id, is_placeholder, s_rect)) = deck
                    .slots
                    .get(idx)
                    .map(|s| (s.id, s.is_placeholder, s.rect))
                else {
                    continue;
                };
                let top_left = page_to_screen(&state.viewport, rect, s_rect.x, s_rect.y);
                let top_right =
                    page_to_screen(&state.viewport, rect, s_rect.x + s_rect.w, s_rect.y);
                let Some(header) = slot_header_layout(top_left.x, top_right.x, top_left.y) else {
                    continue;
                };
                if !is_placeholder && header.name.contains(pos) {
                    // Ver `draw_rename_overlay`: el propio cuadro de texto
                    // se dibuja ahí, en un `egui::Area` de primer plano, no
                    // aquí — aquí solo se arma el estado y se le pide el
                    // foco.
                    let stem = deck
                        .slots
                        .get(idx)
                        .and_then(|s| s.path.file_stem())
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    deck.rename_edit = Some((id, stem));
                    ui.memory_mut(|m| {
                        m.request_focus(egui::Id::new(("canvas_slot_rename", id)));
                    });
                    header_hit = true;
                } else if header.prev.contains(pos) {
                    deck.move_slot(id, MoveDir::Prev);
                    header_hit = true;
                } else if header.next.contains(pos) {
                    deck.move_slot(id, MoveDir::Next);
                    header_hit = true;
                } else if header.lock.contains(pos) {
                    if let Some(s) = deck.slots.get_mut(idx) {
                        s.locked = !s.locked;
                    }
                    header_hit = true;
                } else if header.isolate.contains(pos) {
                    if idx != deck.active {
                        deck.jump_to = Some(idx);
                        deck.jump_reframe = true;
                    }
                    state.isolate = !state.isolate;
                    header_hit = true;
                } else if header.dup.contains(pos) {
                    *action = Some(CanvasAction::Duplicate(id));
                    header_hit = true;
                } else if header.del.contains(pos) {
                    *action = Some(CanvasAction::Delete(id));
                    header_hit = true;
                }
            }
            if header_hit {
                state.press_on_other_slot = true;
            } else {
                let (dx, dy) = screen_to_page(&state.viewport, rect, pos);
                let target = deck.slots.iter().position(|s| {
                    dx >= s.rect.x
                        && dx <= s.rect.x + s.rect.w
                        && dy >= s.rect.y
                        && dy <= s.rect.y + s.rect.h
                });
                if let Some(idx) = target {
                    if idx != deck.active {
                        deck.jump_to = Some(idx);
                        // A petición del usuario: cambiar de activo desde el
                        // área central SIEMPRE reencuadra la vista sobre él
                        // (zoom de ajuste + centrado, el mismo encuadre que
                        // `Ctrl+0`), en vez de dejarlo donde cayera — así no
                        // hace falta ir a buscarlo tras el salto.
                        deck.jump_reframe = true;
                        state.press_on_other_slot = true;
                    }
                } else if deck.folder.is_some()
                    && dx >= deck.add_zone.x
                    && dx <= deck.add_zone.x + deck.add_zone.w
                    && dy >= deck.add_zone.y
                    && dy <= deck.add_zone.y + deck.add_zone.h
                {
                    // Pulsación sobre la zona "+" al final de la baraja: crea
                    // y activa un lienzo en blanco, igual que
                    // `App::add_canvas` (botón de la tira) pero resuelto
                    // aquí mismo — es una operación puramente en memoria, no
                    // toca disco ni el watcher, así que no hace falta pasar
                    // por `main.rs`.
                    if let Some(idx) =
                        deck.push_placeholder((deck.add_zone.w, deck.add_zone.h), new_canvas_ext)
                    {
                        deck.jump_to = Some(idx);
                        deck.jump_reframe = true;
                        state.press_on_other_slot = true;
                    }
                }
            }
        }
    }
}
