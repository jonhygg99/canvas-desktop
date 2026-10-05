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
    if state.inline_text.is_some() || !matches!(state.gesture, Gesture::None) {
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
    ui.menu_button(format!("{} px", text.size), |ui| {
        for size in [12.0, 16.0, 24.0, 32.0, 48.0, 64.0, 96.0] {
            if ui
                .selectable_label(text.size == size, format!("{size} px"))
                .clicked()
            {
                text.size = size;
                ui.close();
            }
        }
    });
    if ui
        .selectable_label(text.weight >= 600, crate::i18n::tr("Bold"))
        .clicked()
    {
        text.weight = if text.weight >= 600 { 400 } else { 700 };
    }
    ui.menu_button(crate::i18n::tr("Color"), |ui| {
        for (label, color) in [
            ("Black", [0, 0, 0, 255]),
            ("White", [255, 255, 255, 255]),
            ("Blue", [0, 122, 255, 255]),
            ("Red", [255, 59, 48, 255]),
            ("Green", [52, 199, 89, 255]),
            ("Yellow", [255, 204, 0, 255]),
        ] {
            if ui
                .selectable_label(text.color == color, crate::i18n::tr(label))
                .clicked()
            {
                text.color = color;
                ui.close();
            }
        }
    });
    let after = LayerContent::Text(text);
    if after != before {
        let _ = state.apply_undo_step(Box::new(SetContent {
            layer: id,
            before,
            after,
        }));
    }
}
#[cfg(test)]
mod tests {
    use super::*;
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
