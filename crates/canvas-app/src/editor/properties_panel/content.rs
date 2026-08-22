//! Despacho de las propiedades de contenido de una capa (texto o forma) a su
//! submódulo correspondiente, con edición en vivo y consolidación en UN paso
//! de deshacer.

use canvas_core::{LayerContent, LayerId};
use eframe::egui;

use super::content_shape::shape_content_ui;
use super::content_text::text_content_ui;
use super::EditorState;

pub(super) fn content_properties_ui(state: &mut EditorState, ui: &mut egui::Ui, sel: LayerId) {
    let Ok(layer) = state.doc.layer(sel) else {
        return;
    };
    let original = layer.content.clone();
    let mut edited = original.clone();

    let (changed, commit) = match &mut edited {
        LayerContent::Text(text) => text_content_ui(ui, text),
        LayerContent::Shape(shape) => shape_content_ui(ui, shape),
        _ => return,
    };

    if changed && edited != original {
        if state.content_edit.is_none() {
            state.content_edit = Some((sel, original));
        }
        if let Ok(l) = state.doc.layer_mut(sel) {
            l.content = edited;
        }
    }
    if commit {
        if let Some((id, before)) = state.content_edit.take() {
            let after = state
                .doc
                .layer(id)
                .map(|l| l.content.clone())
                .unwrap_or_else(|_| before.clone());
            if after != before {
                state.push_undo_step(Box::new(canvas_core::SetContent {
                    layer: id,
                    before,
                    after,
                }));
            }
        }
    }
}
