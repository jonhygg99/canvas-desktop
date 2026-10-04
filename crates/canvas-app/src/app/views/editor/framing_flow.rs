//! Captura de la página completa mediante el mismo bake que el export normal.
use super::super::super::frame::EditorFrame;
use crate::{editor::EditorState, framing::Session};
use eframe::{egui, egui_wgpu::RenderState};

pub(super) fn update(
    state: &mut EditorState,
    ctx: &egui::Context,
    rs: &RenderState,
    frame: &mut EditorFrame<'_>,
) {
    if let Some(session) = &mut state.framing {
        session.poll(ctx);
        state.saved_framing = session.saved;
        if session.closed {
            state.framing = None;
        }
    }
    if !std::mem::take(&mut state.framing_requested) {
        return;
    }
    if crate::deck::is_critical_free_ram(crate::deck::free_ram_bytes()) {
        state.save_error = Some("Not enough available memory to preview framing safely".into());
        return;
    }
    state.pause_video();
    let scope = canvas_render::FxScope(
        frame
            .deck
            .slots
            .get(frame.deck.active)
            .map_or(0, |slot| slot.scope),
    );
    match frame.renderer.bake_page_counting(
        &rs.device,
        &rs.queue,
        scope,
        &state.doc,
        &state.images,
        1.0,
    ) {
        Ok((rgba, width, height, skipped)) => {
            if super::super::super::persistence::bake_came_out_blank_or_incomplete(
                &state.doc, &rgba, skipped,
            ) {
                state.save_error = Some(
                    "The composition could not be rendered completely. Framing was not opened."
                        .into(),
                );
                return;
            }
            state.framing = Some(Session::from_composition(
                state.doc.source_path.clone(),
                canvas_io::LoadedImage {
                    rgba,
                    width,
                    height,
                },
                ctx,
            ));
        }
        Err(error) => state.save_error = Some(format!("Could not preview framing: {error}")),
    }
}
