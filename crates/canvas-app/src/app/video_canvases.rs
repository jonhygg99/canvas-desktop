//! Crea los recortes del editor de vídeo dentro del proyecto actual:
//! los añade como lienzos nuevos de la baraja existente y salta al primero,
//! sin descartar el lienzo que se estaba editando.
//!
//! El mp4 descargado NO entra al proyecto: el lienzo enlaza el temporal de
//! Download (`%TEMP%/canvas-desktop-downloads`) y la ranura solo reserva su
//! nombre final (`1.mp4`, `2.mp4`…) como provisional. Solo el primer `Ctrl+S`
//! escribe en el proyecto, y es el render editado (página + fondo + clip
//! recortado), nunca la descarga en bruto. Un lienzo mixto (foto + capa de
//! vídeo) no entra aquí: se guarda como raster.
use std::path::{Path, PathBuf};

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
            new_docs.push((slot, clip_slot_name(accept, accepts.len() > 1), accept.size));
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
        // solo reserva el nombre final. El primer `Ctrl+S` renderiza el
        // editado a ese nombre y lo adopta (ver `video_save_in_place`).
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

/// Extensión de la ranura de un lienzo de clip: la del vídeo (mp4), nunca el
/// formato de Ajustes — ese es el que convertía el clip en `.png`. Con
/// formato «Canvas» manda la extensión de diseño (el lienzo es editable y el
/// mp4 copiado queda al lado, enlazado).
fn slot_ext(accept: &VideoAccept, format: NewCanvasFormat) -> &str {
    if format == NewCanvasFormat::Canvas {
        return canvas_io::CANVAS_EXTENSION;
    }
    accept
        .path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("mp4")
}

/// ¿Viene esta ruta del staging de Download (`%TEMP%/canvas-desktop-downloads`)?
/// Entonces es la descarga en bruto, nunca el editado: el primer guardado la
/// renderiza al proyecto y la adopta. Pura (testeable).
pub(super) fn is_staging_path(path: &Path) -> bool {
    let staging = crate::ytdlp::api::staging_dir();
    path.starts_with(&staging)
}

/// Reescribe los enlaces de un documento de clip del temporal al mp4 del
/// proyecto tras el primer render: la ruta del documento y las capas de vídeo
/// que bebían de `from` pasan a `to`. Pura sobre `SlotDoc` (testeable).
pub(super) fn relink_video_doc(doc: &mut deck::SlotDoc, from: &Path, to: &Path) {
    if doc.doc.source_path.as_deref() == Some(from) {
        doc.doc.source_path = Some(to.to_path_buf());
    }
    if let Ok(page) = doc.doc.page_mut() {
        for layer in &mut page.layers {
            if let canvas_core::LayerContent::Video(link) = &mut layer.content {
                if link.source_path.as_deref() == Some(from) {
                    link.source_path = Some(to.to_path_buf());
                }
            }
        }
    }
}

/// Adopta el primer render editado (`dst`) como origen del lienzo: reenlaza
/// documento + capas desde `src` (el temporal), convierte la provisional en
/// real con el nombre del render (`1.mp4`, `2.mp4`…) y resetea el trim al
/// archivo entero (el archivo ahora ES la composición). `rendered` es el
/// tamaño real del archivo (la página); ver `reset_rendered_trim`. Solo si la
/// ranura sigue ahí (id estable) y sigue esperando ESTE destino: si la baraja
/// cambió por el medio, se ignora sin tocar nada. Pura sobre `Deck` +
/// `SlotDoc` (testeable sin ventana).
pub(super) fn adopt_rendered_video(
    deck: &mut deck::Deck,
    doc: &mut deck::SlotDoc,
    slot_id: u64,
    src: &Path,
    dst: &Path,
    duration: f64,
    rendered: (u32, u32),
) -> bool {
    let Some(index) = deck.slots.iter().position(|s| s.id == slot_id) else {
        return false;
    };
    if deck.slots[index].path != dst {
        return false;
    }
    relink_video_doc(doc, src, dst);
    reset_rendered_trim(doc, dst, duration, rendered);
    deck.slots[index].name = dst
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    deck.slots[index].path = dst.to_path_buf();
    deck.slots[index].kind = crate::gallery::ItemKind::Video;
    deck.slots[index].is_placeholder = false;
    true
}

/// Composición a renderizar del documento de `state`: página + rectángulo de
/// la capa de vídeo + blur del fondo + trim. Pura (testeable).
pub(super) struct VideoCompositionRequest {
    pub(super) src: PathBuf,
    pub(super) start_s: f64,
    pub(super) end_s: Option<f64>,
    pub(super) page_w: u32,
    pub(super) page_h: u32,
    pub(super) fg_x: i32,
    pub(super) fg_y: i32,
    pub(super) fg_w: u32,
    pub(super) fg_h: u32,
    pub(super) bg_blur: f32,
}

/// Resuelve la composición a renderizar del documento de `state`: la primera
/// capa de vídeo visible con origen en disco + su In/Out no destructivo, el
/// tamaño de página y el blur del fondo. `None` cuando el documento NO es
/// renderizable a vídeo — sin capa de vídeo con origen, o con contenido
/// visible extra que ffmpeg no reproduciría (texto, formas, fotos, un vídeo
/// rotado/recortado/con transparencia): entonces el guardado sigue su camino
/// raster de siempre en vez de inventar un mp4 a medias. Pura (testeable).
pub(super) fn video_composition_request(
    state: &editor::EditorState,
) -> Option<VideoCompositionRequest> {
    let page = state.doc.page().ok()?;
    let video = page.layers.iter().find(|layer| {
        layer.visible && matches!(layer.content, canvas_core::LayerContent::Video(_))
    })?;
    let canvas_core::LayerContent::Video(link) = &video.content else {
        unreachable!("filtrado arriba");
    };
    let src = link.source_path.clone()?;
    // Solo la composición estándar (vídeo colocado + fondo con blur).
    // Pura a propósito (sin `is_file`): que el temporal se borrara se detecta
    // en el flujo de guardado (`video_src_missing` / `video_save_in_place`),
    // no aquí — los tests construyen composiciones sobre rutas ficticias.
    if video.transform.rotation != 0.0 || video.transform.flip_h || video.transform.flip_v {
        return None;
    }
    if link.crop.is_some() || video.opacity < 0.999 {
        return None;
    }
    let bg_id = state.background_layer;
    let extra = page.layers.iter().any(|layer| {
        layer.visible
            && layer.id != video.id
            && Some(layer.id) != bg_id
            && !matches!(layer.content, canvas_core::LayerContent::Group(_))
    });
    if extra {
        return None;
    }
    let bg_blur = bg_id
        .and_then(|id| page.layer(id))
        .filter(|bg| bg.visible)
        .map(|bg| bg.effects.blur_radius)
        .unwrap_or(0.0);
    // x264/yuv420p exigen dimensiones pares: el transform se redondea a px y
    // se baja 1 si sale impar (1 px frente al editor, documentado). La
    // posición va con signo: el `overlay` recorta igual que el editor.
    let even = |v: f64| (v.round() as u32).max(2) & !1;
    Some(VideoCompositionRequest {
        src,
        start_s: link.trim_start,
        end_s: link.trim_end,
        page_w: even(page.width),
        page_h: even(page.height),
        fg_x: video.transform.x.round() as i32,
        fg_y: video.transform.y.round() as i32,
        fg_w: even(video.transform.width),
        fg_h: even(video.transform.height),
        bg_blur,
    })
}

/// Sincroniza el documento con un renderizado en el sitio (`dst` es su propio
/// origen): las capas de vídeo que bebían de ese archivo ahora describen la
/// composición entera (trim completo + duración renderizada). Sin esto, el
/// próximo guardado recortaría dos veces. Además re-ancla la geometría: los
/// píxeles del archivo ya no son el clip original sino la página renderizada,
/// así que el `natural` pasa a `rendered` y el rectángulo a pantalla completa.
/// Sin esto, la reproducción re-escala el archivo al natural obsoleto y el
/// clip se ve distorsionado hasta recargar (el fondo con blur queda intacto
/// porque su capa no se toca). Solo toca las capas de ESE archivo: con
/// «Guardar como…» (otro destino) no se llama y el trim se conserva.
/// Mutación directa, sin comando: sincroniza con el disco, no edita — no toca
/// la revisión del historial. Pura sobre `SlotDoc` (testeable sin ventana).
pub(super) fn reset_rendered_trim(
    doc: &mut deck::SlotDoc,
    dst: &Path,
    duration: f64,
    rendered: (u32, u32),
) {
    let (page_w, page_h) = match doc.doc.page() {
        Ok(page) => (page.width, page.height),
        Err(_) => return,
    };
    let Ok(page) = doc.doc.page_mut() else {
        return;
    };
    for layer in &mut page.layers {
        if let canvas_core::LayerContent::Video(link) = &mut layer.content {
            if link.source_path.as_deref() == Some(dst) {
                link.trim_start = 0.0;
                link.trim_end = None;
                link.poster_time = 0.0;
                link.duration_secs = Some(duration);
                link.natural_width = rendered.0;
                link.natural_height = rendered.1;
                layer.transform.x = 0.0;
                layer.transform.y = 0.0;
                layer.transform.width = page_w;
                layer.transform.height = page_h;
            }
        }
    }
}

/// Nombre reservado para un lienzo creado desde un clip de Download: el del
/// clip (`clip (8).mp4`), nunca el número de la ranura (`38.mp4`) — la galería
/// mostraba el número y parecía un renombre. Deduplica contra el disco y
/// contra las ranuras ya reservadas en esta misma tanda (ningún archivo existe
/// aún, así que dos recortes del mismo clip acabarían en la misma ruta). Pura
/// salvo el `exists` (testeable con carpetas temporales).
fn reserve_clip_slot_path(deck: &deck::Deck, folder: &Path, stem: &str, ext: &str) -> PathBuf {
    let taken: Vec<&Path> = deck.slots.iter().map(|s| s.path.as_path()).collect();
    for n in 0..10_000u32 {
        let name = if n == 0 {
            format!("{stem}.{ext}")
        } else {
            format!("{stem} {}.{ext}", n + 1)
        };
        let candidate = folder.join(&name);
        if !candidate.exists() && !taken.contains(&candidate.as_path()) {
            return candidate;
        }
    }
    folder.join(format!("{stem}.{ext}"))
}

/// `stem` para `reserve_clip_slot_path`: el del archivo descargado (ya es un
/// nombre válido en este equipo) o, sin él, el del nombre visible.
fn clip_stem(accept: &VideoAccept, name: &str) -> String {
    accept
        .path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            Path::new(name)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "clip".to_owned())
}

/// Guarda el lienzo actual en su ranura y añade los nuevos como
/// provisionales listos (reserva del nombre en `Deck`, sin ventana):
/// devuelve el índice del primer lienzo nuevo. El llamador ya verificó baraja no
/// vacía, activo válido y `can_add_canvas`. No activa nada; el llamador hace
/// el `put_slot` + reencuadre.
///
/// Sin copia al proyecto: con formato de vídeo el documento sigue enlazando el
/// temporal de Download (que existe) y la ranura solo reserva el nombre del
/// clip (`clip (8).mp4`, deduplicado). El primer `Ctrl+S` renderiza el editado
/// a ese nombre y lo adopta; si no, el auto-materializado reservaría OTRO
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
    new_docs: Vec<(deck::SlotDoc, String, (f64, f64))>,
    accepts: &[VideoAccept],
    format: NewCanvasFormat,
) -> Result<usize, String> {
    debug_assert_eq!(new_docs.len(), accepts.len());
    deck.slots[deck.active].content = deck::SlotContent::Ready(Box::new(outgoing));
    let first_new = deck.slots.len();
    for ((mut doc, name, size), accept) in new_docs.into_iter().zip(accepts.iter()) {
        let ext = slot_ext(accept, format);
        let Some(index) = deck.push_placeholder(size, ext) else {
            deck.slots.truncate(first_new);
            return Err("Could not create the canvas.".to_owned());
        };
        as_video_slot(&mut deck.slots[index]);
        deck.slots[index].name = name.clone();
        // En un proyecto con carpeta la provisional lleva el nombre del clip,
        // no el número (`38.mp4`): es el archivo que el primer guardado crea.
        if deck.folder.as_deref().is_some_and(|p| p.is_dir()) {
            let folder = deck.folder.clone().expect("carpeta comprobada");
            let reserved = reserve_clip_slot_path(deck, &folder, &clip_stem(accept, &name), ext);
            deck.slots[index].path = reserved.clone();
            deck.slots[index].name = reserved
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or(name);
            if ext == canvas_io::CANVAS_EXTENSION {
                doc.doc.source_path = Some(reserved);
            }
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
    let mut states = states.into_iter();
    let first = states.next().expect("non-empty accepts checked by caller");
    // Sin editor no hay proyecto donde copiar: la ranura enlaza el temporal
    // de Download tal cual («Save as» lo recoloca al guardar).
    let mut new_deck = if accepts.len() == 1 {
        deck::Deck::single(accepts[0].path.clone())
    } else {
        deck::Deck::new_design(accepts[0].size)
    };
    as_video_slot(&mut new_deck.slots[0]);
    new_deck.slots[0].name = clip_slot_name(&accepts[0], accepts.len() > 1);
    for (mut state, accept) in states.zip(&accepts[1..]) {
        let index = new_deck
            .push_placeholder(accept.size, slot_ext(accept, format))
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
    state.doc.source_path = Some(accept.path.clone());
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
