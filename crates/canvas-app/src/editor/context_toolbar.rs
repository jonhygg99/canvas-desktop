//! Acciones próximas a la selección, restringidas al viewport y ocultas
//! durante gestos/edición de texto. Reutiliza comandos y controles del editor.
use super::{
    interaction::Gesture, selection_geometry::selection_box, viewport::layer_corners_screen,
    EditorState,
};
use canvas_core::{LayerContent, LayerId, SetContent};
use eframe::egui;
pub(super) fn show(
    state: &mut EditorState,
    ui: &egui::Ui,
    coord: egui::Rect,
    clip: egui::Rect,
) -> Option<LayerId> {
    if state.inline_text.is_some()
        || state.insert_tool.is_some()
        || !matches!(state.gesture, Gesture::None)
    {
        return None;
    }
    let t = selection_box(state)?;
    let corners = layer_corners_screen(&state.viewport, coord, &t);
    let bounds = egui::Rect::from_points(&corners);
    if !bounds.intersects(clip) || clip.width() < 160.0 {
        return None;
    }
    let size = egui::vec2((clip.width() - 16.0).min(350.0), 38.0);
    let pos = position(bounds, clip, size);
    let mut replace = None;
    egui::Area::new(egui::Id::new("canvas_context_toolbar"))
        .order(egui::Order::Foreground)
        .fixed_pos(pos)
        .constrain_to(clip)
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_max_width(size.x - 16.0);
                ui.horizontal_wrapped(|ui| {
                    if state.selection.len() > 1 {
                        super::selection_layout::controls(state, ui);
                        if ui.button(crate::i18n::tr("Group")).clicked() {
                            crate::layers_panel::group_selection(state);
                        }
                    } else if let Some(id) = state.selection.primary() {
                        single(state, ui, id, &mut replace);
                    }
                });
            });
        });
    replace
}
fn position(bounds: egui::Rect, clip: egui::Rect, size: egui::Vec2) -> egui::Pos2 {
    let x = bounds.center().x - size.x / 2.0;
    let y = if bounds.top() - size.y - 42.0 >= clip.top() {
        bounds.top() - size.y - 42.0
    } else {
        bounds.bottom() + 12.0
    };
    egui::pos2(
        x.clamp(
            clip.left() + 8.0,
            (clip.right() - size.x - 8.0).max(clip.left() + 8.0),
        ),
        y.clamp(
            clip.top() + 8.0,
            (clip.bottom() - size.y - 8.0).max(clip.top() + 8.0),
        ),
    )
}
fn single(state: &mut EditorState, ui: &mut egui::Ui, id: LayerId, replace: &mut Option<LayerId>) {
    let Ok(content) = state.doc.layer(id).map(|l| l.content.clone()) else {
        return;
    };
    let image = matches!(content, LayerContent::Image(_));
    match content {
        LayerContent::Text(_) => text_controls(state, ui, id),
        LayerContent::Image(_) | LayerContent::Video(_) => {
            let label = if state.crop_mode { "Done" } else { "Crop" };
            if ui.button(crate::i18n::tr(label)).clicked() {
                state.crop_mode = !state.crop_mode;
            }
            if image {
                ui.menu_button(crate::i18n::tr("Replace"), |ui| {
                    if ui.button(crate::i18n::tr("From local file")).clicked() {
                        *replace = Some(id);
                        ui.close();
                    }
                    if ui.button(crate::i18n::tr("From internet URL")).clicked() {
                        state.replace_url_popup = Some((id, String::new()));
                        ui.close();
                    }
                });
            }
        }
        LayerContent::Group(_) => {
            if ui.button(crate::i18n::tr("Ungroup")).clicked() {
                crate::layers_panel::ungroup_selection(state);
            }
        }
        LayerContent::Shape(_) => shape_controls(state, ui, id),
        _ => super::selection_layout::controls(state, ui),
    }
}
fn text_controls(state: &mut EditorState, ui: &mut egui::Ui, id: LayerId) {
    let Ok(before) = state.doc.layer(id).map(|l| l.content.clone()) else {
        return;
    };
    let LayerContent::Text(mut text) = before.clone() else {
        return;
    };
    if ui.button(crate::i18n::tr("Edit text")).clicked() {
        super::inline_text::begin(state, id);
    }
    ui.menu_button(crate::i18n::tr("Font"), |ui| {
        for family in ["", "Arial", "Georgia", "Courier New"] {
            let label = if family.is_empty() {
                crate::i18n::tr("System default")
            } else {
                family
            };
            if ui.selectable_label(text.family == family, label).clicked() {
                text.family = family.to_owned();
                ui.close();
            }
        }
    });
    ui.add(
        egui::DragValue::new(&mut text.size)
            .range(4.0..=800.0)
            .suffix(" px"),
    )
    .on_hover_text(crate::i18n::tr("Size"));
    if ui
        .selectable_label(text.weight >= 600, crate::i18n::tr("Bold"))
        .clicked()
    {
        text.weight = if text.weight >= 600 { 400 } else { 700 };
    }
    if ui
        .selectable_label(text.italic, crate::i18n::tr("Italic"))
        .clicked()
    {
        text.italic = !text.italic;
    }
    color_control(ui, "Color", &mut text.color);
    live_content(
        state,
        id,
        before,
        LayerContent::Text(text),
        ui.input(|i| !i.pointer.primary_down()) && !ui.ctx().text_edit_focused(),
    );
}
fn shape_controls(state: &mut EditorState, ui: &mut egui::Ui, id: LayerId) {
    let Ok(before) = state.doc.layer(id).map(|l| l.content.clone()) else {
        return;
    };
    let LayerContent::Shape(mut shape) = before.clone() else {
        return;
    };
    color_control(ui, "Fill", &mut shape.fill);
    color_control(ui, "Stroke", &mut shape.stroke);
    ui.add(
        egui::DragValue::new(&mut shape.stroke_width)
            .range(0.0..=100.0)
            .suffix(" px"),
    )
    .on_hover_text(crate::i18n::tr("Stroke width"));
    live_content(
        state,
        id,
        before,
        LayerContent::Shape(shape),
        ui.input(|i| !i.pointer.primary_down()) && !ui.ctx().text_edit_focused(),
    );
}
fn color_control(ui: &mut egui::Ui, label: &str, rgba: &mut [u8; 4]) {
    ui.label(crate::i18n::tr(label));
    let mut color = egui::Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]);
    if ui
        .color_edit_button_srgba(&mut color)
        .on_hover_text(crate::i18n::tr(label))
        .changed()
    {
        *rgba = color.to_array();
    }
}
// Un arrastre de valor o de color conserva una sola instantanea de contenido.
fn live_content(
    state: &mut EditorState,
    id: LayerId,
    before: LayerContent,
    after: LayerContent,
    commit: bool,
) {
    if before != after {
        if state.content_edit.is_none() {
            state.content_edit = Some((id, before));
        }
        if let Ok(layer) = state.doc.layer_mut(id) {
            layer.content = after;
        }
    }
    if commit
        && state
            .content_edit
            .as_ref()
            .is_some_and(|(layer, _)| *layer == id)
    {
        let (layer, before) = state.content_edit.take().unwrap();
        if let Ok(after) = state.doc.layer(layer).map(|l| l.content.clone()) {
            if before != after {
                state.push_undo_step(Box::new(SetContent {
                    layer,
                    before,
                    after,
                }));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn continuous_content_adjustment_is_one_undo_step() {
        use canvas_core::{ShapeContent, Transform};
        let mut state = EditorState::new_blank(600.0, 500.0);
        let initial = LayerContent::Shape(ShapeContent::default());
        let id = state
            .doc
            .add_layer(
                "shape",
                Transform::new(0.0, 0.0, 50.0, 50.0),
                initial.clone(),
            )
            .unwrap();
        for width in [2.0, 4.0, 8.0] {
            let before = state.doc.layer(id).unwrap().content.clone();
            let LayerContent::Shape(mut shape) = before.clone() else {
                panic!()
            };
            shape.stroke_width = width;
            live_content(&mut state, id, before, LayerContent::Shape(shape), false);
        }
        assert_eq!(state.history.undo_depth(), 0);
        let current = state.doc.layer(id).unwrap().content.clone();
        live_content(&mut state, id, current.clone(), current, true);
        assert_eq!(state.history.undo_depth(), 1);
        state.undo();
        assert_eq!(state.doc.layer(id).unwrap().content, initial);
    }
    #[test]
    fn toolbar_stays_in_view_at_every_canvas_edge() {
        let clip = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(600.0, 400.0));
        for p in [
            egui::pos2(-200.0, -200.0),
            egui::pos2(580.0, 380.0),
            egui::pos2(100.0, 120.0),
        ] {
            let size = egui::vec2(350.0, 38.0);
            let pos = position(
                egui::Rect::from_min_size(p, egui::vec2(100.0, 100.0)),
                clip,
                size,
            );
            assert!(clip.contains_rect(egui::Rect::from_min_size(pos, size)));
        }
    }
}
