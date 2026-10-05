//! Selección profunda y previsualización de objetos bajo el cursor.
use super::{
    interaction::Gesture,
    viewport::{layer_corners_screen, screen_to_page},
    EditorState, ACCENT,
};
use canvas_core::{LayerContent, LayerId};
use eframe::egui;

pub(super) fn candidates(state: &EditorState, point: (f64, f64)) -> Vec<(LayerId, String)> {
    let Ok(page) = state.doc.page() else {
        return Vec::new();
    };
    page.layers
        .iter()
        .rev()
        .filter(|l| {
            !matches!(l.content, LayerContent::Group(_))
                && page.effective_visible(l.id)
                && l.transform.contains_point(point.0, point.1)
        })
        .map(|l| (l.id, l.name.clone()))
        .collect()
}
pub(super) fn cycle(state: &mut EditorState, point: (f64, f64)) {
    let items = candidates(state, point);
    if items.is_empty() {
        return;
    }
    let index = items
        .iter()
        .position(|(id, _)| Some(*id) == state.selection.primary())
        .map_or(0, |index| (index + 1) % items.len());
    state.selection.set(Some(items[index].0));
    state.crop_mode = false;
}
pub(super) fn menu(state: &mut EditorState, ui: &mut egui::Ui) {
    let Some(point) = state.context_point else {
        return;
    };
    let items = candidates(state, point);
    if items.is_empty() {
        return;
    }
    ui.menu_button(crate::i18n::tr("Select layer"), |ui| {
        for (id, name) in &items {
            if ui
                .selectable_label(state.selection.contains(*id), name)
                .clicked()
            {
                state.selection.set(Some(*id));
                state.crop_mode = false;
                ui.close();
            }
        }
    });
    ui.separator();
}
pub(super) fn draw(state: &EditorState, ui: &egui::Ui, coord: egui::Rect, clip: egui::Rect) {
    if !matches!(state.gesture, Gesture::None) || ui.input(|i| i.pointer.any_down()) {
        return;
    }
    let Some(pos) = ui
        .input(|i| i.pointer.hover_pos())
        .filter(|p| clip.contains(*p))
    else {
        return;
    };
    let point = screen_to_page(&state.viewport, coord, pos);
    let Some((id, _)) = candidates(state, point).first().cloned() else {
        return;
    };
    if state.selection.contains(id) || state.doc.page().is_ok_and(|p| p.effective_locked(id)) {
        return;
    }
    if let Ok(layer) = state.doc.layer(id) {
        let [tl, tr, bl, br] = layer_corners_screen(&state.viewport, coord, &layer.transform);
        ui.painter_at(clip).add(egui::Shape::closed_line(
            vec![tl, tr, br, bl],
            egui::Stroke::new(1.0, ACCENT.gamma_multiply(0.5)),
        ));
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cycle_selects_underlapping_visible_layers_in_order() {
        let mut s = EditorState::new_blank(600.0, 400.0);
        for name in ["bottom", "hidden", "top"] {
            s.insert_layer_centered(
                name,
                40.0,
                40.0,
                LayerContent::Shape(canvas_core::ShapeContent::default()),
            );
        }
        s.doc.page_mut().unwrap().layers[1].visible = false;
        let ids: Vec<_> = s.doc.page().unwrap().layers.iter().map(|l| l.id).collect();
        cycle(&mut s, (300.0, 200.0));
        assert_eq!(s.selection.primary(), Some(ids[0]));
        cycle(&mut s, (300.0, 200.0));
        assert_eq!(s.selection.primary(), Some(ids[2]));
    }
}
