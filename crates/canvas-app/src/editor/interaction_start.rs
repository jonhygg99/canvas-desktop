//! Inicio del gesto sobre manejadores o capas.
use super::super::EditorState;
use super::super::{
    viewport::{layer_corners_screen, rotation_handle_screen, screen_to_page},
    HANDLE_SIZE,
};
use super::corner_at;
use super::Gesture;
use canvas_core::{Corner, LayerContent};
use eframe::egui;
pub(super) fn begin(
    state: &mut EditorState,
    ui: &egui::Ui,
    response: &egui::Response,
    rect: egui::Rect,
) {
    // Inicio de gesto.
    if response.drag_started_by(egui::PointerButton::Primary) {
        if let Some(pos) = ui
            .input(|i| i.pointer.press_origin())
            .or_else(|| response.interact_pointer_pos())
        {
            state.gesture = Gesture::None;
            // ¿Sobre un manejador de la selección actual?
            start_handle(state, rect, pos);
            // Si no, ¿sobre una capa? (selecciona y empieza a mover)
            start_move(state, rect, pos);
        }
    }
}

fn start_handle(state: &mut EditorState, rect: egui::Rect, pos: egui::Pos2) {
    let editable_primary = state
        .selection
        .primary()
        .filter(|&id| state.doc.page().is_ok_and(|p| !p.effective_locked(id)));
    if let Some(sel) = editable_primary {
        if let Ok(layer) = state.doc.layer(sel) {
            let t = layer.transform;
            let corners = layer_corners_screen(&state.viewport, rect, &t);
            let on_rotate = rotation_handle_screen(&state.viewport, rect, &t).distance(pos)
                <= HANDLE_SIZE / 2.0 + 3.0;
            if on_rotate {
                let (px, py) = screen_to_page(&state.viewport, rect, pos);
                let (cx, cy) = t.center();
                let pointer_angle = (py - cy).atan2(px - cx).to_degrees();
                state.gesture = Gesture::Rotate {
                    layer: sel,
                    start: t,
                    grab_offset: t.rotation - pointer_angle,
                };
            } else if let Some(corner) = corner_at(corners, pos) {
                state.gesture = if state.crop_mode {
                    let start_crop = match &layer.content {
                        LayerContent::Image(c) => c.crop,
                        LayerContent::Video(c) => c.crop,
                        _ => None,
                    };
                    Gesture::Crop {
                        layer: sel,
                        corner,
                        start_t: t,
                        start_crop,
                        origin: pos,
                    }
                } else {
                    Gesture::Resize {
                        layer: sel,
                        corner,
                        side: None,
                        start: t,
                        origin: pos,
                    }
                };
            } else if let Some(side) =
                super::super::edge_handles::at(corners, pos).filter(|_| !state.crop_mode)
            {
                state.gesture = Gesture::Resize {
                    layer: sel,
                    corner: Corner::TopLeft,
                    side: Some(side),
                    start: t,
                    origin: pos,
                };
            }
        }
    }
}

fn start_move(state: &mut EditorState, rect: egui::Rect, pos: egui::Pos2) {
    if matches!(state.gesture, Gesture::None) {
        let (px, py) = screen_to_page(&state.viewport, rect, pos);
        let hit = state.doc.page().ok().and_then(|p| p.layer_at(px, py));
        if hit != state.selection.primary() {
            state.crop_mode = false;
        }
        // Los modificadores de selección pertenecen al clic. Durante un
        // arrastre Shift restringe el eje sin extender el tramo de capas.
        if hit != state.selection.primary() {
            state.selection.set(hit);
        }
        if let Some(id) = hit.filter(|&id| state.doc.page().is_ok_and(|p| !p.effective_locked(id)))
        {
            if let Ok(layer) = state.doc.layer(id) {
                state.gesture = Gesture::Move {
                    layer: id,
                    start: layer.transform,
                    origin: pos,
                };
            }
        }
    }
}
