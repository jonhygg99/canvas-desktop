//! Gestos sobre la caja común de varias capas o de un grupo.
use super::{
    interaction::{corner_at, Gesture},
    layer_ops::selection_transforms,
    overlay::{format_dims, show_drag_tag},
    selection_geometry::{selection_box, transform_item},
    viewport::{layer_corners_screen, rotation_handle_screen, screen_to_page},
    EditorState, HANDLE_SIZE,
};
use canvas_core::{Command, Composite, Corner, LayerContent, LayerId, SetTransform, Transform};
use eframe::egui;

pub(super) struct SelectionGesture {
    items: Vec<(LayerId, Transform)>,
    from: Transform,
    pub(super) to: Transform,
    origin: egui::Pos2,
    kind: Kind,
}
enum Kind {
    Move,
    Resize(Corner),
    Rotate(f64),
}

pub(super) fn handle(
    state: &mut EditorState,
    ui: &egui::Ui,
    r: &egui::Response,
    rect: egui::Rect,
) -> bool {
    if matches!(state.gesture, Gesture::Selection(_)) {
        let Gesture::Selection(mut gesture) = std::mem::replace(&mut state.gesture, Gesture::None)
        else {
            unreachable!()
        };
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            gesture.to = gesture.from;
            gesture.apply(state);
            state.snap_guides = (Vec::new(), Vec::new());
            return true;
        }
        if let Some(pos) = r.interact_pointer_pos() {
            if r.dragged_by(egui::PointerButton::Primary) {
                gesture.update(state, ui, rect, pos);
            }
        }
        if r.drag_stopped_by(egui::PointerButton::Primary) {
            gesture.finish(state);
        } else {
            state.gesture = Gesture::Selection(gesture);
        }
        return true;
    }
    begin(state, ui, r, rect)
}

fn begin(state: &mut EditorState, ui: &egui::Ui, r: &egui::Response, rect: egui::Rect) -> bool {
    let is_group = state.selection.primary().is_some_and(|id| {
        state
            .doc
            .layer(id)
            .is_ok_and(|l| matches!(l.content, LayerContent::Group(_)))
    });
    if state.selection.len() < 2 && !is_group {
        return false;
    }
    let Some(from) = selection_box(state) else {
        return false;
    };
    let Some(pos) = r.interact_pointer_pos().or_else(|| r.hover_pos()) else {
        return false;
    };
    let current = pos;
    let pos = if r.drag_started_by(egui::PointerButton::Primary) {
        ui.input(|i| i.pointer.press_origin()).unwrap_or(pos)
    } else {
        pos
    };
    let corners = layer_corners_screen(&state.viewport, rect, &from);
    let corner = corner_at(corners, pos);
    let rotate = rotation_handle_screen(&state.viewport, rect, &from).distance(pos)
        <= HANDLE_SIZE / 2.0 + 6.0;
    let (px, py) = screen_to_page(&state.viewport, rect, pos);
    if corner.is_some() || rotate || from.contains_point(px, py) {
        ui.ctx().set_cursor_icon(if corner.is_some() {
            egui::CursorIcon::ResizeNwSe
        } else if rotate {
            egui::CursorIcon::Crosshair
        } else {
            egui::CursorIcon::Move
        });
        if r.drag_started_by(egui::PointerButton::Primary) {
            let (cx, cy) = from.center();
            let kind = if rotate {
                Kind::Rotate(from.rotation - (py - cy).atan2(px - cx).to_degrees())
            } else if let Some(corner) = corner {
                Kind::Resize(corner)
            } else {
                Kind::Move
            };
            state.crop_mode = false;
            let mut gesture = SelectionGesture {
                items: selection_transforms(state),
                from,
                to: from,
                origin: pos,
                kind,
            };
            gesture.update(state, ui, rect, current);
            state.gesture = Gesture::Selection(gesture);
            return true;
        }
    }
    false
}

impl SelectionGesture {
    fn update(
        &mut self,
        state: &mut EditorState,
        ui: &egui::Ui,
        rect: egui::Rect,
        pos: egui::Pos2,
    ) {
        let (dx, dy) = (
            f64::from(pos.x - self.origin.x) / state.viewport.zoom,
            f64::from(pos.y - self.origin.y) / state.viewport.zoom,
        );
        let mods = ui.input(|i| i.modifiers);
        self.to = match self.kind {
            Kind::Move => self.move_box(state, ui, dx, dy),
            Kind::Resize(corner) => {
                // Escala uniforme: el modelo no representa cizalla de capas rotadas.
                let min = self
                    .items
                    .iter()
                    .map(|(_, t)| (1.0 / t.width).max(1.0 / t.height))
                    .fold(0.0_f64, f64::max)
                    * self.from.width.max(self.from.height);
                canvas_core::resize_rotated_from_corner(
                    &self.from,
                    corner,
                    dx,
                    dy,
                    true,
                    min.max(1.0),
                )
            }
            Kind::Rotate(offset) => {
                let (px, py) = screen_to_page(&state.viewport, rect, pos);
                let (cx, cy) = self.from.center();
                let mut angle = offset + (py - cy).atan2(px - cx).to_degrees();
                if mods.shift {
                    angle = (angle / 15.0).round() * 15.0;
                }
                Transform {
                    rotation: angle,
                    ..self.from
                }
            }
        };
        self.apply(state);
        show_drag_tag(
            ui,
            pos,
            if matches!(self.kind, Kind::Rotate(_)) {
                format!("{:.0}°", self.to.rotation)
            } else {
                format_dims(&self.to)
            },
        );
    }
    fn move_box(
        &self,
        state: &mut EditorState,
        ui: &egui::Ui,
        mut dx: f64,
        mut dy: f64,
    ) -> Transform {
        let mods = ui.input(|i| i.modifiers);

        if mods.shift {
            if dx.abs() >= dy.abs() {
                dy = 0.0;
            } else {
                dx = 0.0;
            }
        }
        let mut t = Transform {
            x: self.from.x + dx,
            y: self.from.y + dy,
            ..self.from
        };
        state.snap_guides = (Vec::new(), Vec::new());
        if !mods.alt {
            if let Ok(page) = state.doc.page() {
                let others: Vec<_> = page
                    .layers
                    .iter()
                    .filter(|l| {
                        !self.items.iter().any(|(id, _)| *id == l.id)
                            && !matches!(l.content, LayerContent::Group(_))
                            && page.effective_visible(l.id)
                    })
                    .map(|l| l.transform)
                    .collect();
                let snap = canvas_core::snap_translation(
                    &t,
                    &others,
                    page.width,
                    page.height,
                    6.0 / state.viewport.zoom,
                );
                if !mods.shift || dy == 0.0 {
                    t.x += snap.dx;
                    state.snap_guides.0 = snap.v_guides;
                }
                if !mods.shift || dx == 0.0 {
                    t.y += snap.dy;
                    state.snap_guides.1 = snap.h_guides;
                }
            }
        }
        t
    }
    fn apply(&self, state: &mut EditorState) {
        for &(id, t) in &self.items {
            if let Ok(layer) = state.doc.layer_mut(id) {
                layer.transform = transform_item(t, self.from, self.to);
            }
        }
    }
    fn finish(self, state: &mut EditorState) {
        state.snap_guides = (Vec::new(), Vec::new());
        let cmds: Vec<Box<dyn Command>> = self
            .items
            .into_iter()
            .filter_map(|(layer, before)| {
                let after = state.doc.layer(layer).ok()?.transform;
                (after != before).then(|| {
                    Box::new(SetTransform {
                        layer,
                        before,
                        after,
                    }) as Box<dyn Command>
                })
            })
            .collect();
        if !cmds.is_empty() {
            state.push_undo_step(Box::new(Composite::new("Transformar selección", cmds)));
        }
    }
}
