//! Copias en vivo: no entran en el historial hasta soltar el puntero.
use super::{interaction::Gesture, viewport::screen_to_page, EditorState};
use canvas_core::{Command, Composite, InsertLayer, Layer, LayerId, RemoveLayer, Selection};
use eframe::egui;
use std::collections::{HashMap, HashSet};

pub(super) struct Draft {
    items: Vec<(usize, Layer)>,
    roots: Vec<LayerId>,
    previous: Selection,
    origin: egui::Pos2,
    delta: (f64, f64),
}
impl Draft {
    fn create(state: &mut EditorState, origin: egui::Pos2) -> Option<Self> {
        let page = state.doc.page().ok()?;
        let roots: Vec<_> = state
            .selection
            .roots(page)
            .into_iter()
            .filter(|id| !page.effective_locked(*id))
            .collect();
        let mut ids: HashSet<_> = roots.iter().copied().collect();
        for root in &roots {
            ids.extend(page.descendants(*root));
        }
        let source: Vec<_> = page
            .layers
            .iter()
            .filter(|l| ids.contains(&l.id))
            .cloned()
            .collect();
        if source.is_empty() {
            return None;
        }
        let map: HashMap<_, _> = source
            .iter()
            .map(|l| (l.id, state.doc.allocate_layer_id()))
            .collect();
        let previous = state.selection.clone();
        let mut items = Vec::new();
        for mut layer in source {
            let old = layer.id;
            layer.id = map[&old];
            layer.parent_id = layer.parent_id.and_then(|id| map.get(&id).copied());
            if let Some(image) = state.images.get(&old).cloned() {
                state.images.insert(layer.id, image);
            }
            let index = state.doc.page().ok()?.layers.len();
            let mut command = InsertLayer {
                index,
                layer: layer.clone(),
            };
            command.apply(&mut state.doc).ok()?;
            items.push((index, layer));
        }
        let roots: Vec<_> = roots.iter().map(|id| map[id]).collect();
        state.selection.clear();
        for id in &roots {
            state.selection.toggle(*id);
        }
        Some(Self {
            items,
            roots,
            previous,
            origin,
            delta: (0.0, 0.0),
        })
    }
    fn update(&mut self, state: &mut EditorState, delta: (f64, f64)) {
        self.delta = delta;
        for (_, before) in &self.items {
            if let Ok(layer) = state.doc.layer_mut(before.id) {
                layer.transform.x = before.transform.x + delta.0;
                layer.transform.y = before.transform.y + delta.1;
            }
        }
    }
    fn finish(self, state: &mut EditorState) {
        let commands: Vec<Box<dyn Command>> = self
            .items
            .into_iter()
            .filter_map(|(index, before)| {
                let layer = state.doc.layer(before.id).ok()?.clone();
                Some(Box::new(InsertLayer { index, layer }) as Box<dyn Command>)
            })
            .collect();
        state.push_undo_step(Box::new(Composite::new("Duplicate selection", commands)));
        state.repeat_offset = Some(self.delta);
    }
    fn cancel(self, state: &mut EditorState) {
        for root in self.roots {
            let _ = RemoveLayer::new(root).apply(&mut state.doc);
        }
        for (_, layer) in self.items {
            state.images.remove(&layer.id);
        }
        state.selection = self.previous;
    }
}
pub(crate) fn duplicate(state: &mut EditorState, repeat: bool) {
    if !matches!(state.gesture, Gesture::None) {
        return;
    }
    let delta = if repeat {
        state.repeat_offset.unwrap_or((24.0, 24.0))
    } else {
        (24.0, 24.0)
    };
    if let Some(mut draft) = Draft::create(state, egui::Pos2::ZERO) {
        draft.update(state, delta);
        draft.finish(state);
    }
}
pub(super) fn handle(
    state: &mut EditorState,
    ui: &egui::Ui,
    response: &egui::Response,
    rect: egui::Rect,
) -> bool {
    if !matches!(state.gesture, Gesture::Duplicate(_)) {
        if !matches!(state.gesture, Gesture::None)
            || !response.drag_started_by(egui::PointerButton::Primary)
            || !ui.input(|i| i.modifiers.command && i.modifiers.alt)
        {
            return false;
        }
        let Some(origin) = ui.input(|i| i.pointer.press_origin()) else {
            return false;
        };
        let (x, y) = screen_to_page(&state.viewport, rect, origin);
        let Some(hit) = state.doc.page().ok().and_then(|p| p.layer_at(x, y)) else {
            return false;
        };
        if !super::layer_ops::selection_transforms(state)
            .iter()
            .any(|(id, _)| *id == hit)
        {
            state.selection.set(Some(hit));
        }
        let Some(draft) = Draft::create(state, origin) else {
            return false;
        };
        state.gesture = Gesture::Duplicate(draft);
    }
    let Gesture::Duplicate(mut draft) = std::mem::replace(&mut state.gesture, Gesture::None) else {
        unreachable!()
    };
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        draft.cancel(state);
        return true;
    }
    if let Some(pos) = response.interact_pointer_pos() {
        let mut delta = (
            f64::from(pos.x - draft.origin.x) / state.viewport.zoom,
            f64::from(pos.y - draft.origin.y) / state.viewport.zoom,
        );
        if ui.input(|i| i.modifiers.shift) {
            if delta.0.abs() >= delta.1.abs() {
                delta.1 = 0.0
            } else {
                delta.0 = 0.0
            }
        }
        draft.update(state, delta);
    }
    if response.drag_stopped_by(egui::PointerButton::Primary) {
        draft.finish(state);
    } else {
        state.gesture = Gesture::Duplicate(draft);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn command_alt_drag_copies_selection_instead_of_moving_originals() {
        let (ctx, mut state, a, b) = crate::editor::selection_gesture_tests::fixture();
        let modifiers = egui::Modifiers::COMMAND | egui::Modifiers::ALT;
        for (x, pressed) in [(35.0, Some(true)), (45.0, None), (85.0, Some(false))] {
            let mut events = crate::editor::selection_gesture_tests::pointer(x, 35.0, pressed);
            for event in &mut events {
                if let egui::Event::PointerButton {
                    modifiers: value, ..
                } = event
                {
                    *value = modifiers;
                }
            }
            let _ = ctx.run_ui(
                egui::RawInput {
                    modifiers,
                    events,
                    ..Default::default()
                },
                |ui| {
                    let rect =
                        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(600.0, 500.0));
                    let response = ui.allocate_rect(rect, egui::Sense::click_and_drag());
                    super::super::interaction::layer_interaction(&mut state, ui, &response, rect);
                },
            );
        }
        assert_eq!(state.doc.layer(a).unwrap().transform.x, 20.0);
        assert_eq!(state.doc.layer(b).unwrap().transform.x, 100.0);
        assert_eq!(state.doc.page().unwrap().layers.len(), 4);
        assert_eq!(state.history.undo_depth(), 1);
        assert_eq!(state.repeat_offset, Some((50.0, 0.0)));
        state.undo();
        assert_eq!(state.doc.page().unwrap().layers.len(), 2);
    }
    #[test]
    fn copies_repeat_spacing_and_undo_as_a_single_step() {
        let (_, mut state, a, _) = crate::editor::selection_gesture_tests::fixture();
        state.selection.set(Some(a));
        let mut draft = Draft::create(&mut state, egui::Pos2::ZERO).unwrap();
        draft.update(&mut state, (70.0, 0.0));
        draft.finish(&mut state);
        let first = state.selection.primary().unwrap();
        assert_eq!(state.doc.layer(a).unwrap().transform.x, 20.0);
        assert_eq!(state.doc.layer(first).unwrap().transform.x, 90.0);
        duplicate(&mut state, true);
        let next = state.selection.primary().unwrap();
        assert_eq!(state.doc.layer(next).unwrap().transform.x, 160.0);
        state.undo();
        assert!(state.doc.layer(next).is_err());
        assert!(state.doc.layer(first).is_ok());
    }
    #[test]
    fn cancelling_a_copy_restores_selection_without_history() {
        let (_, mut state, a, _) = crate::editor::selection_gesture_tests::fixture();
        let before = state.selection.clone();
        let mut draft = Draft::create(&mut state, egui::Pos2::ZERO).unwrap();
        draft.update(&mut state, (50.0, 20.0));
        draft.cancel(&mut state);
        assert_eq!(state.doc.page().unwrap().layers.len(), 2);
        assert_eq!(state.selection.ids(), before.ids());
        assert_eq!(state.doc.layer(a).unwrap().transform.x, 20.0);
        assert_eq!(state.history.undo_depth(), 0);
    }
    #[test]
    fn copying_a_group_preserves_parent_links_and_redo() {
        let (_, mut state, _, _) = crate::editor::selection_gesture_tests::fixture();
        crate::layers_panel::group_selection(&mut state);
        let original = state.selection.primary().unwrap();
        duplicate(&mut state, false);
        let copied = state.selection.primary().unwrap();
        assert_ne!(copied, original);
        assert_eq!(state.doc.page().unwrap().descendants(copied).len(), 2);
        state.undo();
        assert!(state.doc.layer(copied).is_err());
        state.redo();
        assert_eq!(state.doc.page().unwrap().descendants(copied).len(), 2);
    }
}
