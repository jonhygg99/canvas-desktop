//! La respuesta de vídeo valida dueño y captura antes de tocar el editor.
//! Las capas siguen leyendo la fuente duradera: no se resetean trims ni
//! transforms, y la preview no cambia de píxeles al terminar el render.

use super::super::{AppInner, Nav, View, Workspace};
use crate::{deck, editor, loader};
use eframe::egui;

#[cfg(test)]
#[path = "video_save_tests.rs"]
mod tests;

impl AppInner {
    pub(super) fn on_design_saved(
        &mut self,
        ws: &mut Workspace,
        done: loader::DesignSaveDone,
        ctx: &egui::Context,
        open_after: &mut Option<Nav>,
    ) {
        if !ws.deck.slots.get(ws.deck.active).is_some_and(|slot| {
            slot.id == done.target.slot_id && slot.scope == done.target.generation
        }) {
            return;
        }
        let View::Editor(state) = &mut ws.view else {
            return;
        };
        if !state.saving || state.saving_capture != Some(done.capture) {
            return;
        }
        if done.result.is_ok() {
            let unchanged = state.doc == done.document && state.is_idle_except_save();
            relink_sources(state, &mut ws.deck, &done.links);
            if !unchanged {
                state.history.mark_unsaved();
                state.saving_capture = Some((usize::MAX, done.capture.1));
            }
        }
        self.on_saved(ws, done.path, done.result, done.new_source, ctx, open_after);
    }

    pub(super) fn on_video_composition_rendered(
        &mut self,
        ws: &mut Workspace,
        done: loader::VideoSaveDone,
        ctx: &egui::Context,
        open_after: &mut Option<Nav>,
    ) {
        let slot_id = ws.deck.slots.get(ws.deck.active).map(|s| s.id);
        if !done.target.is_current(ws.deck.generation(), slot_id) {
            tracing::warn!("respuesta de vídeo obsoleta para {}", done.dst.display());
            return;
        }
        let View::Editor(state) = &mut ws.view else {
            return;
        };
        if !state.saving || state.saving_capture != Some(done.capture) {
            return;
        }
        let unchanged = state.doc == done.document && state.is_idle_except_save();
        if done.result.is_ok() {
            // Reenlazar tampoco altera el historial: una edición en vuelo
            // conserva su trim/geometría y sigue pendiente de guardado.
            relink_sources(state, &mut ws.deck, &done.links);
            if !unchanged {
                state.history.mark_unsaved();
                state.saving_capture = Some((usize::MAX, done.capture.1));
            }
        }
        let result = done.result.map(|_| ());
        self.on_saved(ws, done.dst, result, true, ctx, open_after);
    }
}

/// Reenlaza las capas de vídeo del documento activo Y de los `SlotDoc`
/// guardados en la baraja: todos los recortes del mismo temporal comparten
/// copia en `.download`, y un hermano que conservara la ruta temporal
/// perdería su fuente al limpiar Download aunque sus bytes ya estén en el
/// proyecto.
pub(super) fn relink_sources(
    state: &mut editor::EditorState,
    deck: &mut deck::Deck,
    links: &canvas_io::VideoSourceLinks,
) {
    if links.is_empty() {
        return;
    }
    relink_doc(&mut state.doc, links);
    for slot in &mut deck.slots {
        if let deck::SlotContent::Ready(doc) = &mut slot.content {
            relink_doc(&mut doc.doc, links);
        }
    }
}

fn relink_doc(doc: &mut canvas_core::Document, links: &canvas_io::VideoSourceLinks) {
    for page in &mut doc.pages {
        for layer in &mut page.layers {
            if let canvas_core::LayerContent::Video(video) = &mut layer.content {
                if let Some((_, dst)) = links
                    .iter()
                    .find(|(src, _)| video.source_path.as_ref() == Some(src))
                {
                    video.source_path = Some(dst.clone());
                }
            }
        }
    }
}
