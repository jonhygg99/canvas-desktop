//! La camara del lienzo: centrado al saltar de ranura, los dos encajes
//! automaticos (`Ctrl+0` sobre el lienzo activo, `Ctrl+Alt+0` sobre la baraja
//! entera), el zoom con rueda/pellizco y el paneo.
//!
//! Movido tal cual desde `canvas_ui`, en el mismo orden: `panning` y
//! `space_down` salen por el valor de retorno porque los necesita el resto del
//! frame.

use eframe::egui;

use crate::deck::{Deck, DeckAxis};

use super::super::viewport::AutoFit;
use super::super::EditorState;

/// Estado de puntero que el resto del frame necesita conocer.
pub(super) struct Camera {
    pub(super) panning: bool,
    pub(super) space_down: bool,
}

pub(super) fn apply_camera(
    state: &mut EditorState,
    deck: &Deck,
    ui: &egui::Ui,
    rect: egui::Rect,
    response: &egui::Response,
) -> Camera {
    // Salto pedido por la tira, el teclado, un clic directo sobre otro
    // lienzo o "Añadir lienzo": centra sobre el nuevo lienzo activo sin
    // tocar el zoom. También arma `AutoFit::Active` — sin esto, si el modo
    // seguía en `All` (el usuario acababa de pulsar `Ctrl+Alt+0`), el
    // primer redimensionado después de este centrado volvía a encajar TODA
    // la baraja (`resized` más abajo) y deshacía el centrado, con el efecto
    // de "vuelve a la vista de siempre" — el nuevo centrado puntual pasa a
    // ser la referencia, no una excepción que el próximo resize revierta.
    if let Some(target) = state.viewport.center_request.take() {
        state.viewport.center_on(target, rect.size());
        state.viewport.auto_fit = AutoFit::Active;
    }

    // Ajustar el lienzo activo: Ctrl/Cmd+0 o primer frame. Ajustar TODA la
    // baraja: Ctrl+Alt+0 (más específico primero, mismo patrón que
    // redo/undo en `handle_shortcuts`).
    let fit_all = ui.ctx().input_mut(|i| {
        i.consume_shortcut(&egui::KeyboardShortcut::new(
            egui::Modifiers::COMMAND | egui::Modifiers::ALT,
            egui::Key::Num0,
        ))
    });
    let fit_active = ui.ctx().input_mut(|i| {
        i.consume_shortcut(&egui::KeyboardShortcut::new(
            egui::Modifiers::COMMAND,
            egui::Key::Num0,
        ))
    });
    // Reajuste automático si el área de dibujo cambió de tamaño desde el
    // frame anterior (ventana maximizada/restaurada, panel lateral
    // arrastrado): repite el último ajuste automático (`Ctrl+0`/
    // `Ctrl+Alt+0`) mientras siga armado — se desarma en cuanto el usuario
    // hace zoom o paneo a mano (`Viewport::manual_view_change`). SIEMPRE se
    // sella el tamaño, no solo cuando cambia, para no quedar desincronizado.
    let resized = state.viewport.note_size(rect.size());
    if fit_all {
        state.viewport.fit(deck.bounds(), rect.size(), AutoFit::All);
    } else if fit_active || state.viewport.needs_fit {
        state
            .viewport
            .fit(deck.active_rect(), rect.size(), AutoFit::Active);
    } else if resized {
        match state.viewport.auto_fit {
            AutoFit::Active => {
                state
                    .viewport
                    .fit(deck.active_rect(), rect.size(), AutoFit::Active);
            }
            AutoFit::All => {
                state.viewport.fit(deck.bounds(), rect.size(), AutoFit::All);
            }
            AutoFit::Off => {}
        }
    }

    // Zoom pedido desde el menú, anclado al centro del lienzo.
    if let Some(factor) = state.pending_zoom_factor.take() {
        state.viewport.zoom_at(rect.size() / 2.0, factor);
    }

    // Rueda: desplaza a lo largo del eje PRIMARIO de la baraja (Shift = eje
    // transversal); Ctrl+rueda y el pellizco hacen zoom, anclados al cursor.
    // Es uniforme también con un solo lienzo — una excepción "con un archivo
    // hace zoom" se sentiría como un fallo, no como una regla.
    if response.hovered() {
        let (raw_scroll, pinch, pointer, ctrl, shift) = ui.ctx().input(|i| {
            (
                i.smooth_scroll_delta,
                i.zoom_delta(),
                i.pointer.hover_pos(),
                i.modifiers.command,
                i.modifiers.shift,
            )
        });
        if ctrl && raw_scroll.y != 0.0 {
            let factor = (f64::from(raw_scroll.y) * 0.0025).exp();
            let anchor = pointer.map_or(rect.size() / 2.0, |p| p - rect.min);
            state.viewport.zoom_at(anchor, factor);
        } else if raw_scroll != egui::Vec2::ZERO {
            // El ratón manda un solo eje de rueda (`raw_scroll.y`); a qué
            // componente del pan va depende de cuál sea el eje primario de
            // la baraja — Shift pide el transversal. Un trackpad que ya
            // manda X (pellizco de dos dedos, Shift+rueda que el propio SO
            // convierte) se respeta tal cual, sin remapear.
            let is_horizontal = matches!(deck.axis, DeckAxis::Horizontal);
            let swap = shift != is_horizontal;
            let delta = if swap && raw_scroll.x == 0.0 {
                egui::vec2(raw_scroll.y, 0.0)
            } else {
                raw_scroll
            };
            // `+=`, no `-=`: mismo signo que el paneo por arrastre de más
            // abajo (`pan += drag_delta`) y que `egui::ScrollArea`
            // (`offset -= scroll_delta`, con `offset` en el sentido
            // contrario a `pan` — es la posición del contenido en pantalla,
            // no cuánto se ha desplazado dentro de él). Con `-=` la rueda
            // quedaba invertida respecto al resto de la propia app.
            state.viewport.manual_view_change();
            state.viewport.pan += delta;
        }
        if (pinch - 1.0).abs() > 1e-4 {
            let anchor = pointer.map_or(rect.size() / 2.0, |p| p - rect.min);
            state.viewport.zoom_at(anchor, f64::from(pinch));
        }
    }

    // Paneo: botón central, o espacio + arrastre primario.
    let space_down = ui.ctx().input(|i| i.key_down(egui::Key::Space));
    let panning = response.dragged_by(egui::PointerButton::Middle)
        || (space_down && response.dragged_by(egui::PointerButton::Primary));
    if panning {
        state.viewport.manual_view_change();
        state.viewport.pan += response.drag_delta();
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    } else if space_down && response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }

    Camera {
        panning,
        space_down,
    }
}
