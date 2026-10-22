//! La respuesta de vídeo valida dueño y captura antes de tocar el editor.
//! Las capas siguen leyendo la fuente duradera: no se resetean trims ni
//! transforms, y la preview no cambia de píxeles al terminar el render.
//!
//! El dueño puede NO ser el lienzo activo: la cola única de escritura
//! (`app/write_queue.rs`) renderiza también clips de fondo, y esa respuesta se
//! adopta en SU ranura (`messages/background_saved.rs`) sin tocar la pantalla.

use super::super::{AppInner, Nav, View, Workspace};
use super::background_saved::BackgroundWriteDone;
use super::saved::SavedFile;
use crate::gallery::ItemKind;
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
        // La respuesta solo vale para la ranura que la pidió en ESTA baraja
        // (`scope` es el id del FxScope GPU, no la generación: compararlos
        // era una lotería númerica que descartaba guardados completos y
        // dejaba `state.saving` encendido para siempre, con el autoguardado
        // muerto y la barra en «Saving…»).
        ws.save.writes.finished(done.target.slot_id);
        let active_slot = ws.deck.slots.get(ws.deck.active).map(|slot| slot.id);
        if !done.target.is_current(ws.deck.generation(), active_slot) {
            // Escritura de un lienzo de fondo (o de una baraja que ya no
            // existe): reenlazar fuentes vale para todos los documentos, pero
            // el archivo se aplica a SU ranura, no a la que hay en pantalla.
            // Reenlazar es una corrección de fuentes, no una edición: solo se
            // aplica si la respuesta es de una ranura que sigue en esta baraja
            // (una respuesta de otra baraja o de una ranura borrada no toca
            // ni el documento en pantalla ni sus fuentes).
            if done.result.is_ok() && ws.deck.find_by_id(done.target.slot_id).is_some() {
                if let View::Editor(state) = &mut ws.view {
                    relink_sources(state, &mut ws.deck, &done.links);
                }
            }
            let written = BackgroundWriteDone {
                generation: done.target.generation,
                slot_id: done.target.slot_id,
                path: done.path,
                result: done
                    .result
                    .map_err(|error| format!("Could not write the design: {error}")),
                new_source: done.new_source,
                capture: done.capture,
                kind: Some(ItemKind::Design),
            };
            self.on_background_saved(ws, written, ctx);
            return;
        }
        let View::Editor(state) = &mut ws.view else {
            return;
        };
        if !state.saving || state.saving_capture != Some(done.capture) {
            return;
        }
        // La captura sellada al lanzar el guardado es el único juez de «hubo
        // ediciones mientras se escribía» (ver el mismo razonamiento en
        // `on_video_composition_rendered`): `on_saved` la compara con el
        // historial y no se toca desde aquí.
        if done.result.is_ok() {
            relink_sources(state, &mut ws.deck, &done.links);
        }
        self.on_saved(
            ws,
            SavedFile {
                path: done.path,
                result: done.result,
                new_source: done.new_source,
                ticket: loader::SaveTicket {
                    target: done.target,
                    capture: done.capture,
                },
            },
            ctx,
            open_after,
        );
    }

    pub(super) fn on_video_composition_rendered(
        &mut self,
        ws: &mut Workspace,
        done: loader::VideoSaveDone,
        ctx: &egui::Context,
        open_after: &mut Option<Nav>,
    ) {
        let slot_id = ws.deck.slots.get(ws.deck.active).map(|s| s.id);
        // La escritura terminó: la cola puede seguir, sea cual sea su dueño.
        ws.save.writes.finished(done.target.slot_id);
        if Some(done.target.slot_id) != slot_id {
            // No es el lienzo activo: render de FONDO de la cola de escritura.
            // Una ranura a la que el usuario saltó a mitad se resuelve por el
            // camino activo de abajo (el destino ya coincide con el activo).
            if done.result.is_ok() && ws.deck.find_by_id(done.target.slot_id).is_some() {
                if let View::Editor(state) = &mut ws.view {
                    relink_sources(state, &mut ws.deck, &done.links);
                }
            }
            let written = BackgroundWriteDone {
                generation: done.target.generation,
                slot_id: done.target.slot_id,
                path: done.dst,
                result: done
                    .result
                    .map(|_| ())
                    .map_err(|error| format!("Could not write the clip: {error}")),
                new_source: true,
                capture: done.capture,
                kind: Some(ItemKind::Video),
            };
            self.on_background_saved(ws, written, ctx);
            return;
        }
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
        // «¿Hubo ediciones durante el render?» lo decide SOLO el historial
        // (`mark_saved_at` dentro de `on_saved`, A04): la captura sellada al
        // lanzar el render viaja intacta. Comparar documentos enteros aquí era
        // imposible de acertar — el preview escribe `poster_time` en
        // `state.doc` en cada fotograma, sin comando de historial, así que el
        // documento NUNCA coincide con el que viajó al worker. El guardado se
        // daba por «editado durante la escritura», el lienzo se quedaba sucio
        // para siempre y el autoguardado re-renderizaba el mismo clip cada
        // `VIDEO_QUIET`. Tampoco se envenena la captura: si el historial se
        // movió, `mark_saved_at` devuelve `false` y el banner lo explica.
        if done.result.is_ok() {
            // Reenlazar tampoco altera el historial: una edición en vuelo
            // conserva su trim/geometría y sigue pendiente de guardado.
            relink_sources(state, &mut ws.deck, &done.links);
        }
        let result = done.result.map(|_| ());
        self.on_saved(
            ws,
            SavedFile {
                path: done.dst,
                result,
                new_source: true,
                ticket: loader::SaveTicket {
                    target: done.target,
                    capture: done.capture,
                },
            },
            ctx,
            open_after,
        );
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
