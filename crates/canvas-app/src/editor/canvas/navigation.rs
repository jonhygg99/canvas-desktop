//! Controles visibles de encuadre; reutilizan la misma cámara que los atajos.
use super::super::{selection_geometry, viewport::AutoFit, EditorState};
use crate::deck::{Deck, DeckRect};
use eframe::egui;

pub(super) fn show(state: &mut EditorState, deck: &Deck, ui: &egui::Ui, rect: egui::Rect) {
    egui::Area::new(egui::Id::new("canvas_navigation"))
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-12.0, -12.0))
        .constrain_to(rect)
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui
                        .button("−")
                        .on_hover_text(crate::i18n::tr("Zoom out"))
                        .clicked()
                    {
                        state.viewport.zoom_at(rect.size() / 2.0, 0.8);
                    }
                    ui.menu_button(format!("{:.0}%", state.viewport.zoom * 100.0), |ui| {
                        for percent in [25.0, 50.0, 100.0, 200.0, 400.0] {
                            if ui.button(format!("{percent:.0}%")).clicked() {
                                let factor = percent / 100.0 / state.viewport.zoom;
                                state.viewport.zoom_at(rect.size() / 2.0, factor);
                                ui.close();
                            }
                        }
                    });
                    if ui
                        .button("+")
                        .on_hover_text(crate::i18n::tr("Zoom in"))
                        .clicked()
                    {
                        state.viewport.zoom_at(rect.size() / 2.0, 1.25);
                    }
                    if ui
                        .button(crate::i18n::tr("Fit page"))
                        .on_hover_text("Ctrl/Cmd+0")
                        .clicked()
                    {
                        state
                            .viewport
                            .fit(deck.active_rect(), rect.size(), AutoFit::Active);
                    }
                    let target = selection_target(state, deck.active_origin());
                    if ui
                        .add_enabled(
                            target.is_some(),
                            egui::Button::new(crate::i18n::tr("View selection")),
                        )
                        .clicked()
                    {
                        state
                            .viewport
                            .fit(target.unwrap(), rect.size(), AutoFit::Off);
                    }
                });
            });
        });
}

fn selection_target(state: &EditorState, origin: (f64, f64)) -> Option<DeckRect> {
    let items = super::super::layer_ops::selection_transforms(state);
    let bounds = selection_geometry::bounds(&items)?;
    Some(DeckRect {
        x: origin.0 + bounds.x,
        y: origin.1 + bounds.y,
        w: bounds.width,
        h: bounds.height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use canvas_core::{LayerContent, ShapeContent, Transform};
    #[test]
    fn selection_view_uses_rotated_bounds_and_active_canvas_origin() {
        let mut state = EditorState::new_blank(600.0, 500.0);
        let mut t = Transform::new(20.0, 30.0, 80.0, 40.0);
        t.rotation = 90.0;
        let id = state
            .doc
            .add_layer("shape", t, LayerContent::Shape(ShapeContent::default()))
            .unwrap();
        state.selection.set(Some(id));
        let target = selection_target(&state, (1000.0, 2000.0)).unwrap();
        assert!((target.x - 1040.0).abs() < 1e-6);
        assert!((target.y - 2010.0).abs() < 1e-6);
        assert!((target.w - 40.0).abs() < 1e-6);
        state.selection.clear();
        assert!(selection_target(&state, (0.0, 0.0)).is_none());
    }
}
