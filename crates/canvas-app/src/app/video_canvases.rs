//! Crea todos los recortes antes de reemplazar el workspace, sin perder ninguno.
use super::{AppInner, View, Workspace};
use crate::{deck, editor, ytdlp::VideoAccept};
use eframe::egui;

impl AppInner {
    pub(super) fn open_video_canvases(
        &mut self,
        ws: &mut Workspace,
        accepts: Vec<VideoAccept>,
        ctx: &egui::Context,
    ) {
        if accepts.is_empty() {
            return;
        }
        let result = accepts
            .iter()
            .map(build_video_canvas)
            .collect::<Result<Vec<_>, _>>();
        let states = match result {
            Ok(states) => states,
            Err(error) => {
                if let View::Editor(state) = &mut ws.view {
                    state.ytdlp.error = Some(error);
                }
                return;
            }
        };
        let (mut state, new_deck) = assemble_canvases(states, &accepts);
        ws.deck = new_deck;
        self.apply_deck_prefs(ws);
        if accepts.len() > 1 {
            ws.deck.strip_visible = true;
        }
        state.from_gallery = ws.deck.folder.clone();
        state.sidecar_enabled = self.settings.sidecar_default;
        for slot in &mut ws.deck.slots {
            if let deck::SlotContent::Ready(doc) = &mut slot.content {
                doc.sidecar_enabled = self.settings.sidecar_default;
            }
        }
        self.remember_page_size(ws, &state.doc);
        self.settings.ytdlp_canvas_size = accepts[0].size;
        self.settings.save_in_background();
        ws.view = View::Editor(Box::new(state));
        self.sync_title(ctx, ws);
    }
}

fn assemble_canvases(
    states: Vec<editor::EditorState>,
    accepts: &[VideoAccept],
) -> (editor::EditorState, deck::Deck) {
    let mut states = states.into_iter();
    let first = states.next().expect("non-empty accepts checked by caller");
    let mut new_deck = if accepts.len() == 1 {
        deck::Deck::single(accepts[0].path.clone())
    } else {
        deck::Deck::new_design(accepts[0].size)
    };
    if accepts.len() > 1 {
        new_deck.slots[0].name = accepts[0].title.clone();
    }
    for (mut state, accept) in states.zip(&accepts[1..]) {
        let index = new_deck
            .push_placeholder(accept.size, "png")
            .expect("unsaved session supports new canvases");
        new_deck.slots[index].name = accept.title.clone();
        new_deck.slots[index].content = deck::SlotContent::Ready(Box::new(state.take_slot()));
    }
    (first, new_deck)
}

fn build_video_canvas(accept: &VideoAccept) -> Result<editor::EditorState, String> {
    let poster = if accept.poster.is_file() {
        canvas_io::load_image(&accept.poster)
    } else {
        canvas_io::load_video_frame(&accept.path, accept.trim_start)
    }
    .map_err(|_| "Could not read the video. Open the editor again.".to_owned())?;
    let probe = canvas_io::probe_video_size(&accept.path).ok();
    let (vw, vh) = accept
        .video_size
        .or_else(|| probe.map(|(w, h, _)| (f64::from(w), f64::from(h))))
        .unwrap_or((f64::from(poster.width), f64::from(poster.height)));
    let duration = probe.and_then(|(_, _, d)| d);
    let (pw, ph) = accept.size;
    let mut state = editor::EditorState::new_blank_image(pw, ph);
    let pixels = canvas_render::image_data_from_rgba(poster.rgba, poster.width, poster.height);
    if accept.blur_radius > 0.0 {
        let content = canvas_core::LayerContent::Image(canvas_core::ImageContent {
            source_path: None,
            natural_width: poster.width,
            natural_height: poster.height,
            crop: None,
        });
        let bg_id = state
            .doc
            .add_layer(
                "Blurred background",
                canvas_core::cover_transform(vw, vh, pw, ph),
                content,
            )
            .map_err(|e| e.to_string())?;
        state
            .doc
            .layer_mut(bg_id)
            .map_err(|e| e.to_string())?
            .effects
            .blur_radius = accept.blur_radius;
        state.images.insert(bg_id, pixels.clone());
        state.background_layer = Some(bg_id);
    }
    let content = canvas_core::VideoContent {
        source_path: Some(accept.path.clone()),
        natural_width: vw as u32,
        natural_height: vh as u32,
        crop: None,
        duration_secs: duration,
        poster_time: accept.trim_start,
        trim_start: accept.trim_start,
        trim_end: accept.trim_end,
    };
    let transform =
        crate::ytdlp::edit::positioned_transform(vw, vh, pw, ph, accept.zoom, accept.position);
    let id = state
        .doc
        .add_layer(
            &accept.title,
            transform,
            canvas_core::LayerContent::Video(content),
        )
        .map_err(|e| format!("Could not create the canvas: {e}"))?;
    state.images.insert(id, pixels);
    state.selection = canvas_core::Selection::single(id);
    state.history.mark_unsaved();
    Ok(state)
}

#[cfg(test)]
#[path = "video_canvases_tests.rs"]
mod tests;
