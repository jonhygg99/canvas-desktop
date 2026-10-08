//! Crea los recortes del editor de vídeo dentro del proyecto actual:
//! los añade como lienzos nuevos de la baraja existente y salta al primero,
//! sin descartar el lienzo que se estaba editando.
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
        // Si ya hay un editor abierto, los lienzos nuevos se añaden a SU
        // baraja (mismo proyecto) en vez de reemplazar el workspace: antes
        // `Create canvas` tiraba el proyecto en curso y abría otro.
        // Devuelve los estados si no hay dónde añadir (hay que reemplazar).
        if let Some(leftover) = try_append_video_canvases(self, ws, states, &accepts, ctx) {
            let states = leftover;
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
}

/// Añade los lienzos ya construidos a la baraja del workspace en curso y
/// activa el primero. Devuelve `None` si los gestionó (éxito o error
/// mostrado); devuelve `Some(estados)` si no hay editor/baraja donde
/// añadirlos y el llamador debe usar el camino antiguo (reemplazar).
fn try_append_video_canvases(
    app: &mut AppInner,
    ws: &mut Workspace,
    states: Vec<editor::EditorState>,
    accepts: &[VideoAccept],
    ctx: &egui::Context,
) -> Option<Vec<editor::EditorState>> {
    if !matches!(ws.view, View::Editor(_)) {
        return Some(states);
    }
    if ws.deck.slots.is_empty() || ws.deck.active >= ws.deck.slots.len() {
        return Some(states);
    }
    // Una baraja de un solo archivo suelto no admite hermanos: se convierte
    // en sesión sin guardar para conservar el lienzo actual y poder añadir.
    if !ws.deck.can_add_canvas() {
        ws.deck.unsaved_session = true;
    }
    if !ws.deck.can_add_canvas() {
        return Some(states);
    }
    let sidecar_default = app.settings.sidecar_default;
    // Préstamos disjuntos de `ws.view` y `ws.deck`: mientras `state` vive
    // no se puede pedir `ws` entero (p. ej. `apply_deck_prefs`), así que el
    // reencuadre y los ajustes se hacen después, sin el préstamo.
    {
        let View::Editor(state) = &mut ws.view else {
            return Some(states);
        };
        // Guarda el lienzo actual en su ranura y añade los nuevos: puro
        // sobre `Deck` (testeable sin ventana) + activación del primero.
        let mut new_docs = Vec::with_capacity(states.len());
        for (mut video_state, accept) in states.into_iter().zip(accepts.iter()) {
            let mut slot = video_state.take_slot();
            slot.sidecar_enabled = sidecar_default;
            new_docs.push((slot, clip_slot_name(accept, accepts.len() > 1), accept.size));
        }
        let outgoing = state.take_slot();
        let Some(first_new) = stash_and_push(&mut ws.deck, outgoing, new_docs) else {
            // Sin sitio para hermanos: devuelve el lienzo a `state`.
            let back = std::mem::replace(
                &mut ws.deck.slots[ws.deck.active].content,
                deck::SlotContent::Active,
            );
            if let deck::SlotContent::Ready(back) = back {
                state.put_slot(*back);
            }
            state.ytdlp.error = Some("Could not create the canvas.".to_owned());
            return None;
        };
        let incoming = std::mem::replace(
            &mut ws.deck.slots[first_new].content,
            deck::SlotContent::Active,
        );
        let deck::SlotContent::Ready(incoming) = incoming else {
            unreachable!("ranura recién creada");
        };
        state.put_slot(*incoming);
        state.from_gallery = ws.deck.folder.clone();
        state.sidecar_enabled = sidecar_default;
        if ws.deck.slots.len() > 1 {
            ws.deck.strip_visible = true;
        }
        ws.deck.active = first_new;
        ws.deck.jump_to = None;
        ws.deck.jump_reframe = false;
        ws.deck.layout_dirty = true;
        state.viewport.request_fit();
    }
    app.apply_deck_prefs(ws);
    if let View::Editor(state) = &ws.view {
        app.remember_page_size(ws, &state.doc);
    }
    app.settings.ytdlp_canvas_size = accepts[0].size;
    app.settings.save_in_background();
    app.sync_title(ctx, ws);
    None
}

/// Guarda el lienzo actual en su ranura y añade los nuevos como
/// provisionales listos. Puro sobre `Deck` (sin ventana): devuelve el índice
/// del primer lienzo nuevo o `None` si un `push_placeholder` falló (entonces
/// retira los nuevos a medias, pero el lienzo saliente ya quedó guardado en
/// su ranura y el llamador debe devolverlo a `state`). El llamador ya
/// verificó baraja no vacía, activo válido y `can_add_canvas`.
/// No activa nada; el llamador hace el `put_slot` + reencuadre.
fn stash_and_push(
    deck: &mut deck::Deck,
    outgoing: deck::SlotDoc,
    new_docs: Vec<(deck::SlotDoc, String, (f64, f64))>,
) -> Option<usize> {
    deck.slots[deck.active].content = deck::SlotContent::Ready(Box::new(outgoing));
    let first_new = deck.slots.len();
    for (doc, name, size) in new_docs {
        let Some(index) = deck.push_placeholder(size, "png") else {
            deck.slots.truncate(first_new);
            return None;
        };
        as_video_slot(&mut deck.slots[index]);
        deck.slots[index].name = name;
        deck.slots[index].content = deck::SlotContent::Ready(Box::new(doc));
    }
    Some(first_new)
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
    as_video_slot(&mut new_deck.slots[0]);
    new_deck.slots[0].name = clip_slot_name(&accepts[0], accepts.len() > 1);
    for (mut state, accept) in states.zip(&accepts[1..]) {
        let index = new_deck
            .push_placeholder(accept.size, "png")
            .expect("unsaved session supports new canvases");
        as_video_slot(&mut new_deck.slots[index]);
        new_deck.slots[index].name = clip_slot_name(accept, accepts.len() > 1);
        new_deck.slots[index].content = deck::SlotContent::Ready(Box::new(state.take_slot()));
    }
    (first, new_deck)
}

/// Nombre visible de la ranura de un lienzo creado desde un clip. Con un solo
/// recorte es el archivo del clip: la ranura ES ese vídeo, igual que abrirlo
/// desde la galería. Con varios, el título, que ya los numera.
fn clip_slot_name(accept: &VideoAccept, multiple: bool) -> String {
    if multiple {
        return accept.title.clone();
    }
    accept
        .path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| accept.title.clone())
}

/// La ranura de un lienzo de clip se muestra como vídeo, no como imagen: su
/// contenido es una capa de vídeo enlazada al clip de origen.
fn as_video_slot(slot: &mut deck::Slot) {
    slot.kind = crate::gallery::ItemKind::Video;
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
