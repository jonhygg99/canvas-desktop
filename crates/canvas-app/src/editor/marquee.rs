//! Selección por rectángulo desde espacio vacío; Escape restaura la anterior.
use super::{
    interaction::Gesture,
    viewport::{page_to_screen, screen_to_page},
    EditorState, ACCENT,
};
use canvas_core::Selection;
use eframe::egui;

pub(super) struct Marquee {
    origin: (f64, f64),
    current: (f64, f64),
    prior: Selection,
    additive: bool,
}
pub(super) fn handle(
    state: &mut EditorState,
    ui: &egui::Ui,
    r: &egui::Response,
    rect: egui::Rect,
) -> bool {
    if r.drag_started_by(egui::PointerButton::Primary) && matches!(state.gesture, Gesture::None) {
        if let Some(pos) = ui.input(|i| i.pointer.press_origin()) {
            if super::interaction::crop_corner_at(state, rect, pos).is_some() {
                return false;
            }
            let origin = screen_to_page(&state.viewport, rect, pos);
            if state
                .doc
                .page()
                .is_ok_and(|p| p.layer_at(origin.0, origin.1).is_none())
            {
                state.gesture = Gesture::Marquee(Marquee {
                    origin,
                    current: origin,
                    prior: state.selection.clone(),
                    additive: ui.input(|i| i.modifiers.shift || i.modifiers.command),
                });
            }
        }
    }
    if !matches!(state.gesture, Gesture::Marquee(_)) {
        return false;
    }
    let Gesture::Marquee(mut marquee) = std::mem::replace(&mut state.gesture, Gesture::None) else {
        unreachable!()
    };
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        state.selection = marquee.prior;
        return true;
    }
    if let Some(pos) = r.interact_pointer_pos() {
        marquee.current = screen_to_page(&state.viewport, rect, pos);
    }
    state.selection = if marquee.additive {
        marquee.prior.clone()
    } else {
        Selection::default()
    };
    if let Ok(page) = state.doc.page() {
        for layer in &page.layers {
            if layer.parent_id.is_none()
                && page.effective_visible(layer.id)
                && !page.effective_locked(layer.id)
                && layer
                    .transform
                    .corners()
                    .iter()
                    .all(|&(x, y)| marquee.contains(x, y))
                && !state.selection.contains(layer.id)
            {
                state.selection.toggle(layer.id);
            }
        }
    }
    if !r.drag_stopped_by(egui::PointerButton::Primary) {
        state.gesture = Gesture::Marquee(marquee);
    }
    true
}
impl Marquee {
    fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.origin.0.min(self.current.0)
            && x <= self.origin.0.max(self.current.0)
            && y >= self.origin.1.min(self.current.1)
            && y <= self.origin.1.max(self.current.1)
    }
}
pub(super) fn draw(state: &EditorState, ui: &egui::Ui, coord: egui::Rect, clip: egui::Rect) {
    if let Gesture::Marquee(m) = &state.gesture {
        let a = page_to_screen(&state.viewport, coord, m.origin.0, m.origin.1);
        let b = page_to_screen(&state.viewport, coord, m.current.0, m.current.1);
        let r = egui::Rect::from_two_pos(a, b);
        ui.painter_at(clip)
            .rect_filled(r, 0.0, ACCENT.gamma_multiply(0.12));
        ui.painter_at(clip).rect_stroke(
            r,
            0.0,
            egui::Stroke::new(1.0, ACCENT),
            egui::StrokeKind::Inside,
        );
    }
}
