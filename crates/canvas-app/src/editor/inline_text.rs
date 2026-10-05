//! Edición de texto en el lienzo. Una sesión conserva contenido original
//! para confirmar todo como un paso o cancelar sin modificar el historial.
use super::{viewport::layer_corners_screen, EditorState};
use canvas_core::{LayerContent, LayerId, SetContent};
use eframe::egui;

pub(super) struct InlineText {
    layer: LayerId,
    before: LayerContent,
    focus: bool,
}
pub(super) fn begin(state: &mut EditorState, id: LayerId) {
    if state.doc.page().is_ok_and(|p| p.effective_locked(id)) {
        return;
    }
    finish(state, false);
    if let Some((layer, before)) = state.content_edit.take() {
        if let Ok(after) = state.doc.layer(layer).map(|l| l.content.clone()) {
            if after != before {
                state.push_undo_step(Box::new(SetContent {
                    layer,
                    before,
                    after,
                }));
            }
        }
    }
    if let Ok(layer) = state.doc.layer(id) {
        if matches!(layer.content, LayerContent::Text(_)) {
            state.inline_text = Some(InlineText {
                layer: id,
                before: layer.content.clone(),
                focus: true,
            });
            state.selection.set(Some(id));
            state.crop_mode = false;
        }
    }
}
pub(crate) fn finish(state: &mut EditorState, cancel: bool) {
    let Some(edit) = state.inline_text.take() else {
        return;
    };
    let Ok(layer) = state.doc.layer_mut(edit.layer) else {
        return;
    };
    if cancel {
        layer.content = edit.before;
    } else {
        let after = layer.content.clone();
        if after != edit.before {
            state.push_undo_step(Box::new(SetContent {
                layer: edit.layer,
                before: edit.before,
                after,
            }));
        }
    }
}

pub(super) fn pending_changes(state: &EditorState) -> bool {
    state.inline_text.as_ref().is_some_and(|edit| {
        state
            .doc
            .layer(edit.layer)
            .is_ok_and(|l| l.content != edit.before)
    })
}
/// Antes de guardados y navegación: nunca guardar una edición sin historial.
pub(crate) fn prepare(state: &mut EditorState, ctx: &egui::Context) {
    if state.inline_text.is_none() {
        return;
    }
    if state.save_clicked
        || state.save_as_clicked
        || state.return_requested
        || ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::S))
    {
        finish(state, false);
    }
}
pub(super) fn show(state: &mut EditorState, ui: &egui::Ui, coord: egui::Rect, clip: egui::Rect) {
    let Some(mut edit) = state.inline_text.take() else {
        return;
    };
    let Ok(layer) = state.doc.layer(edit.layer) else {
        return;
    };
    if state.selection.primary() != Some(edit.layer) {
        state.inline_text = Some(edit);
        finish(state, false);
        return;
    }
    let LayerContent::Text(mut text) = layer.content.clone() else {
        return;
    };
    let corners = layer_corners_screen(&state.viewport, coord, &layer.transform);
    let bounds = egui::Rect::from_points(&corners);
    let mut done = false;
    let mut cancel = false;
    egui::Area::new(egui::Id::new("canvas_inline_text"))
        .order(egui::Order::Foreground)
        .fixed_pos(bounds.min)
        .constrain_to(clip)
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                let width = bounds.width().clamp(80.0, (clip.width() - 16.0).max(80.0));
                let r = ui.add(
                    egui::TextEdit::multiline(&mut text.text)
                        .id(egui::Id::new(("inline_text", edit.layer.raw())))
                        .font(egui::FontId::proportional(
                            text.size * state.viewport.zoom as f32,
                        ))
                        .desired_width(width)
                        .desired_rows(2),
                );
                if edit.focus {
                    r.request_focus();
                    edit.focus = false;
                }
                cancel = ui.input(|i| i.key_pressed(egui::Key::Escape));
                done = r.lost_focus()
                    || ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter));
                ui.horizontal(|ui| {
                    done |= ui.button(crate::i18n::tr("Done")).clicked();
                    cancel |= ui.button(crate::i18n::tr("Cancel")).clicked();
                });
            });
        });
    if let Ok(layer) = state.doc.layer_mut(edit.layer) {
        layer.content = LayerContent::Text(text);
    }
    state.inline_text = Some(edit);
    if cancel || done {
        finish(state, cancel);
    }
}
