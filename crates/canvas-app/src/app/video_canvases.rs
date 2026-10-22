use super::{AppInner, View, Workspace};
use crate::settings::NewCanvasFormat;
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
        // Solo un guardado/exportación en vuelo obliga a esperar (`blocks_leaving`):
        // una edición de UI a medias se consolida abajo, y esperar por ella
        // aparcaba los lienzos de vídeo para siempre.
        if let Some(reason) = match &ws.view {
            View::Editor(state) => state.blocks_leaving(),
            _ => None,
        } {
            // El motivo real (un guardado, una exportación o un framing), no
            // «un guardado» fijo: antes cualquier espera se anunciaba como
            // guardado, y encima en el canal de errores.
            if let View::Editor(state) = &mut ws.view {
                state.notice = Some(super::navigation::park_notice_with(
                    reason,
                    "Creating canvases",
                ));
            }
            ws.deferred_nav
                .push_back(super::Nav::OpenVideos { accepts });
            return;
        }
        // Fuera del camino de espera: consolida un texto inline o un campo de
        // panel a medias antes de tocar la baraja.
        if let View::Editor(state) = &mut ws.view {
            editor::prepare_for_navigation(state);
        }
        // El formato de los lienzos nuevos es el de Ajustes, igual que
        // «✚ New design»: un clip no tiene por qué acabar en PNG.
        let format = self.settings.new_canvas_format;
        let result = accepts
            .iter()
            .map(|accept| build_video_canvas(accept, format))
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
        if let Some(leftover) = try_append_video_canvases(self, ws, states, &accepts, format, ctx) {
            let states = leftover;
            let (mut state, new_deck) = assemble_canvases(states, &accepts, format);
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
            // Sin carpeta de proyecto no hay destino que escribir sin
            // preguntar: el clip sigue siendo una provisional y su destino se
            // decide con «Save as…». Traza para no buscarlo a ciegas.
            if ws.deck.folder.is_none() {
                tracing::info!(
                    "clip: sin carpeta de proyecto, no se guarda solo; se pedirá destino al guardar"
                );
            }
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
    format: NewCanvasFormat,
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
            new_docs.push((slot, accept.size));
        }
        let outgoing = state.take_slot();
        let first_new = match stash_and_push(&mut ws.deck, outgoing, new_docs, accepts, format) {
            Ok(ok) => ok,
            Err(error) => {
                // Sin sitio para la provisional: devuelve el lienzo a `state`.
                let back = std::mem::replace(
                    &mut ws.deck.slots[ws.deck.active].content,
                    deck::SlotContent::Active,
                );
                if let deck::SlotContent::Ready(back) = back {
                    state.put_slot(*back);
                }
                state.ytdlp.error = Some(error);
                return None;
            }
        };
        // Sin copia: el documento sigue enlazando el temporal y la ranura
        // solo reserva el nombre final. El primer guardado renderiza el
        // editado a ese nombre y lo adopta (ver `clip_save_target`).
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
        // Todas las ranuras de la tanda —la activa incluida— se escriben solas
        // en cuanto el editor esté ocioso (ver `write_queue::tick`): el clip deja
        // de ser una provisional que enlaza el temporal de Download sin
        // esperar a `Ctrl+S` ni a «Back to gallery».
        if ws.deck.folder.is_some() {
            let generation = ws.deck.generation();
            let new_ids = ws.deck.slots[first_new..].iter().map(|slot| slot.id);
            ws.save.writes.enqueue(generation, new_ids);
        } else {
            // Sin carpeta de proyecto no hay destino que la app pueda escribir
            // sola (un mp4 suelto pide «Save as…»): no se encola nada.
            tracing::info!("clip: la baraja no tiene carpeta; no se encola el guardado automático");
        }
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

/// Extensión de la ranura de un lienzo de clip: la del vídeo (mp4), nunca el
/// formato de Ajustes — ese es el que convertía el clip en `.png`. Con
/// formato «Canvas» manda la extensión de diseño (el lienzo es editable y el
/// mp4 copiado queda al lado, enlazado).
pub(super) fn slot_ext(accept: &VideoAccept, format: NewCanvasFormat) -> &str {
    if format == NewCanvasFormat::Canvas {
        return canvas_io::CANVAS_EXTENSION;
    }
    accept
        .path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("mp4")
}

/// Guarda el lienzo actual en su ranura y añade los nuevos como
/// provisionales listos (reserva del nombre en `Deck`, sin ventana):
/// devuelve el índice del primer lienzo nuevo. El llamador ya verificó baraja no
/// vacía, activo válido y `can_add_canvas`. No activa nada; el llamador hace
/// el `put_slot` + reencuadre.
///
/// Sin copia al proyecto: con formato de vídeo el documento sigue enlazando el
/// temporal de Download (que existe) y la ranura solo reserva el nombre del
/// clip (`clip-edwin-luna-01.mp4`, deduplicado). El primer `Ctrl+S` renderiza
/// el editado a ese nombre y lo adopta; si no, el auto-materializado reservaría OTRO
/// archivo y reescribiría el origen. Con formato «Canvas» el documento apunta
/// desde ya al `.canvas` reservado (un diseño se escribe tal cual: si apuntara
/// al temporal, el primer guardado machacaría la descarga con el contenedor).
/// Si un `push_placeholder` falla, retira los nuevos a medias (el lienzo
/// saliente ya quedó guardado en su ranura y el llamador debe devolverlo a
/// `state`). Si el temporal se borra desde Download antes de guardar, el render
/// falla con un error limpio que ofrece el póster por «Save as».
fn stash_and_push(
    deck: &mut deck::Deck,
    outgoing: deck::SlotDoc,
    new_docs: Vec<(deck::SlotDoc, (f64, f64))>,
    accepts: &[VideoAccept],
    format: NewCanvasFormat,
) -> Result<usize, String> {
    debug_assert_eq!(new_docs.len(), accepts.len());
    deck.slots[deck.active].content = deck::SlotContent::Ready(Box::new(outgoing));
    let first_new = deck.slots.len();
    for ((mut doc, size), accept) in new_docs.into_iter().zip(accepts.iter()) {
        let ext = slot_ext(accept, format);
        let Some(index) = deck.push_placeholder(size, ext) else {
            deck.slots.truncate(first_new);
            return Err("Could not create the canvas.".to_owned());
        };
        if let Err(error) = super::video_canvas_names::configure_video_slot(
            deck,
            index,
            &mut doc,
            accept,
            format,
            accepts.len() > 1,
        ) {
            deck.slots.truncate(first_new);
            return Err(error);
        }
        deck.slots[index].content = deck::SlotContent::Ready(Box::new(doc));
    }
    Ok(first_new)
}

fn assemble_canvases(
    states: Vec<editor::EditorState>,
    accepts: &[VideoAccept],
    format: NewCanvasFormat,
) -> (editor::EditorState, deck::Deck) {
    let mut new_deck = deck::Deck::new_design(accepts[0].size);
    let mut states = states.into_iter();
    let mut first = states.next().expect("non-empty accepts checked by caller");
    let mut first_doc = first.take_slot();
    super::video_canvas_names::configure_video_slot(
        &mut new_deck,
        0,
        &mut first_doc,
        &accepts[0],
        format,
        accepts.len() > 1,
    )
    .expect("first canvas has a free name");
    first.put_slot(first_doc);
    for (mut state, accept) in states.zip(&accepts[1..]) {
        let index = new_deck
            .push_placeholder(accept.size, slot_ext(accept, format))
            .expect("unsaved session supports new canvases");
        let mut doc = state.take_slot();
        super::video_canvas_names::configure_video_slot(
            &mut new_deck,
            index,
            &mut doc,
            accept,
            format,
            accepts.len() > 1,
        )
        .expect("new canvas has a free name");
        new_deck.slots[index].content = deck::SlotContent::Ready(Box::new(doc));
    }
    (first, new_deck)
}

/// Nombre visible de la ranura de un lienzo creado desde un clip. Con un solo
/// recorte es el archivo del clip: la ranura ES ese vídeo, igual que abrirlo
/// desde la galería. Con varios, el título, que ya los numera.
pub(super) fn clip_slot_name(accept: &VideoAccept, multiple: bool) -> String {
    if multiple {
        return accept.title.clone();
    }
    accept
        .path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| accept.title.clone())
}

fn build_video_canvas(
    accept: &VideoAccept,
    format: NewCanvasFormat,
) -> Result<editor::EditorState, String> {
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
    // `Canvas` es un diseño autónomo (guarda sus capas sin rasterizar); el
    // resto, un raster real con sidecar — mismo reparto que `new_design`.
    let mut state = if format == NewCanvasFormat::Canvas {
        editor::EditorState::new_blank(pw, ph)
    } else {
        editor::EditorState::new_blank_image(pw, ph)
    };
    // El lienzo ES el clip (mp4), no su póster: `source_path` decide el
    // nombre que se propone al guardar y lo que muestra el título. Sin
    // esto, el documento heredaba el provisional (`Untitled N.png`) y el
    // vídeo acabado parecía —y se guardaba como— un PNG.
    state.doc.source_path = (!state.is_design).then(|| accept.path.clone());
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
#[path = "video_save_tests.rs"]
mod save_tests;
#[cfg(test)]
#[path = "video_canvases_tests.rs"]
mod tests;
