//! Confirmacion de gestos y seleccion por clic.
use super::super::viewport::screen_to_page;
use super::super::EditorState;
use super::Gesture;
use canvas_core::{LayerContent, SetCrop, SetTransform};
use eframe::egui;
pub(super) fn commit(state: &mut EditorState, response: &egui::Response) {
    // Fin de gesto: consolida en UN comando de deshacer.
    if response.drag_stopped_by(egui::PointerButton::Primary) {
        state.snap_guides = (Vec::new(), Vec::new());
        match std::mem::replace(&mut state.gesture, Gesture::None) {
            Gesture::Move { layer, start, .. }
            | Gesture::Resize { layer, start, .. }
            | Gesture::Rotate { layer, start, .. } => {
                if let Ok(l) = state.doc.layer(layer) {
                    let after = l.transform;
                    if after != start {
                        state.push_undo_step(Box::new(SetTransform {
                            layer,
                            before: start,
                            after,
                        }));
                    }
                }
            }
            Gesture::Crop {
                layer,
                start_t,
                start_crop,
                ..
            } => {
                if let Ok(l) = state.doc.layer(layer) {
                    let after_t = l.transform;
                    let after_crop = match &l.content {
                        LayerContent::Image(content) => content.crop,
                        LayerContent::Video(content) => content.crop,
                        _ => None,
                    };
                    if after_t != start_t || after_crop != start_crop {
                        state.push_undo_step(Box::new(canvas_core::Composite::new(
                            "Recortar",
                            vec![
                                Box::new(SetTransform {
                                    layer,
                                    before: start_t,
                                    after: after_t,
                                }),
                                Box::new(SetCrop {
                                    layer,
                                    before: start_crop,
                                    after: after_crop,
                                }),
                            ],
                        )));
                    }
                }
            }
            Gesture::None | Gesture::Selection(_) | Gesture::Marquee(_) => {}
        }
    }
}

pub(super) fn select(
    state: &mut EditorState,
    ui: &egui::Ui,
    response: &egui::Response,
    rect: egui::Rect,
) {
    // Click sin arrastre: seleccionar / deseleccionar.
    if response.clicked_by(egui::PointerButton::Primary) {
        if let Some(pos) = response.interact_pointer_pos() {
            let (px, py) = screen_to_page(&state.viewport, rect, pos);
            if ui.input(|i| i.modifiers.alt) {
                super::super::layer_picking::cycle(state, (px, py));
                return;
            }
            let hit = state.doc.page().ok().and_then(|p| p.layer_at(px, py));
            if hit != state.selection.primary() {
                state.crop_mode = false;
            }
            let mods = ui.input(|i| i.modifiers);
            if mods.command {
                if let Some(id) = hit {
                    state.selection.toggle(id);
                }
            } else if mods.shift {
                if let (Some(id), Ok(page)) = (hit, state.doc.page()) {
                    state.selection.extend_range(page, id);
                }
            } else {
                state.selection.set(hit);
            }
        }
    }
}
