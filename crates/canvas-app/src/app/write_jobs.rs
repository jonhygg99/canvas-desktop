//! Los trabajos concretos de la cola única de escritura
//! (`app/write_queue.rs`): qué se hornea, con qué destino y con qué `ticket`.
//!
//! Vive aparte de la cola para que esta se lea como lo que es —orden, un
//! worker, destinos seguros— sin mezclar el detalle de cómo se escribe un
//! clip, un raster o un diseño. Todas las escrituras de aquí son de FONDO: el
//! lienzo activo las lanza por el camino normal de guardado o, si es un clip
//! recién añadido, por `start_active` (`clip_save::start_clip_save`).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui;
use eframe::egui_wgpu::RenderState;

use crate::deck;
use crate::editor;
use crate::loader;

use super::clip_save::{self, clip_save_target, composition_of, is_clip_document};
use super::frame::EditorFrame;
use super::persistence::{self, SaveStart};
use super::write_target::{background_target, BackgroundRequest, BackgroundWrite};

/// La ranura en cola ES el lienzo activo (un clip recién añadido): se guarda
/// por el mismo camino que un `Ctrl+S` (`clip_save::start_clip_save`). Un
/// raster activo no llega aquí (su autoguardado lo escribe y `enqueue_dirty`
/// no lo encola).
pub(super) fn start_active(
    state: &mut editor::EditorState,
    ctx: &egui::Context,
    f: &mut EditorFrame<'_>,
    slot_id: u64,
    index: usize,
) {
    let slot = &f.deck.slots[index];
    let target = clip_save_target(
        &state.doc,
        state.background_layer,
        &slot.path,
        slot.is_placeholder,
        f.deck.folder.as_deref(),
    );
    match target {
        Some(dst) if is_clip_document(&state.doc) => {
            tracing::info!(
                ruta = %dst.display(),
                "clip: se guarda solo al añadirlo (lienzo activo)"
            );
            if clip_save::start_clip_save(state, f, ctx, dst) != SaveStart::Started {
                f.save.writes.finished(slot_id);
            }
        }
        _ => {
            tracing::info!(
                "escritura: el lienzo activo {slot_id} no tiene destino automático de clip"
            );
            f.save.writes.finished(slot_id);
        }
    }
    f.save.last_write = Some(Instant::now());
    ctx.request_repaint();
}

/// Ranura de FONDO: se escribe SIN activarla. Su `SlotDoc` guarda
/// `saving`/`saving_capture`, que viajan a `EditorState` si el usuario salta a
/// esa ranura mientras el worker escribe.
pub(super) fn start_background(
    ctx: &egui::Context,
    rs: &RenderState,
    f: &mut EditorFrame<'_>,
    slot_id: u64,
    index: usize,
) {
    let deck::SlotContent::Ready(doc) = &f.deck.slots[index].content else {
        f.save.writes.finished(slot_id);
        return;
    };
    if doc.saving || !doc.history.is_dirty() {
        // Ya está limpio (se guardó por otro camino, p. ej. su propio
        // `Ctrl+S`) o su escritura está en vuelo: no es un fallo.
        f.save.writes.finished(slot_id);
        return;
    }
    if is_clip_document(&doc.doc) {
        start_background_clip(ctx, f, slot_id, index);
    } else {
        start_background_raster(ctx, rs, f, slot_id, index);
    }
    f.save.last_write = Some(Instant::now());
    ctx.request_repaint();
}

/// Clip de fondo: se renderiza sin activarlo.
fn start_background_clip(ctx: &egui::Context, f: &mut EditorFrame<'_>, slot_id: u64, index: usize) {
    let generation = f.deck.generation();
    let (slot_path, is_placeholder, plan, capture, payload) = {
        let deck::SlotContent::Ready(doc) = &f.deck.slots[index].content else {
            f.save.writes.finished(slot_id);
            return;
        };
        let slot_path = f.deck.slots[index].path.clone();
        let is_placeholder = f.deck.slots[index].is_placeholder;
        let Some(dst) = clip_save_target(
            &doc.doc,
            doc.background_layer,
            &slot_path,
            is_placeholder,
            f.deck.folder.as_deref(),
        ) else {
            tracing::info!(
                "clip: la ranura {slot_id} no tiene destino automático (sin fuente, capas extra o sin carpeta)"
            );
            f.save.writes.finished(slot_id);
            return;
        };
        let Some(request) = composition_of(&doc.doc, doc.background_layer) else {
            f.save.writes.finished(slot_id);
            return;
        };
        let capture = (doc.history.undo_depth(), doc.history.revision());
        let payload = editor::state::payload_of(&doc.doc, &doc.images, doc.background_layer);
        (slot_path, is_placeholder, (dst, request), capture, payload)
    };
    let (dst, request) = plan;
    if let deck::SlotContent::Ready(doc) = &mut f.deck.slots[index].content {
        doc.saving = true;
        doc.saving_capture = Some(capture);
    }
    let reserve = is_placeholder && slot_path == dst && f.deck.folder.is_some();
    tracing::info!(
        ruta = %dst.display(),
        restantes = f.save.writes.remaining(generation),
        "clip: render de una ranura de fondo sin cambiar de lienzo"
    );
    loader::spawn_video_save(
        loader::VideoSaveRequest {
            target: loader::ImageInsertDest {
                generation,
                slot_id,
            },
            capture,
            payload,
            reserve,
            composition: request.into_composition(dst),
        },
        f.tx.clone(),
        ctx.clone(),
    );
}

/// Lienzo de fondo raster o diseño: se hornea su documento (el que guarda su
/// `SlotDoc`, no el de pantalla) y se escribe en su archivo.
fn start_background_raster(
    ctx: &egui::Context,
    rs: &RenderState,
    f: &mut EditorFrame<'_>,
    slot_id: u64,
    index: usize,
) {
    let (dst, is_design) = {
        let deck::SlotContent::Ready(doc) = &f.deck.slots[index].content else {
            f.save.writes.finished(slot_id);
            return;
        };
        let req = BackgroundRequest {
            doc: &doc.doc,
            is_placeholder: f.deck.slots[index].is_placeholder,
            is_design: doc.is_design,
            born_blank: doc.born_blank,
            external_change: doc.external_change,
            app_written: doc
                .doc
                .source_path
                .as_deref()
                .is_some_and(|path| f.save.is_app_written(path)),
            overwrite_ok: doc
                .doc
                .source_path
                .as_deref()
                .is_some_and(|path| f.save.overwrite_ok(path)),
            skip_overwrite_warning: f.settings.skip_overwrite_warning,
        };
        match background_target(req) {
            BackgroundWrite::Auto(path) => (path, doc.is_design),
            BackgroundWrite::Skip(reason) => {
                tracing::debug!(
                    ranura = slot_id,
                    motivo = reason,
                    "escritura de fondo: se deja para «Save all»"
                );
                f.save.writes.finished(slot_id);
                return;
            }
        }
    };
    if is_design {
        start_background_design(ctx, rs, f, slot_id, index, dst);
    } else {
        start_background_image(ctx, rs, f, slot_id, index, dst);
    }
}

fn start_background_image(
    ctx: &egui::Context,
    rs: &RenderState,
    f: &mut EditorFrame<'_>,
    slot_id: u64,
    index: usize,
    dst: PathBuf,
) {
    let generation = f.deck.generation();
    if let Err(message) = persistence::prepare_bake(f.deck, f.renderer) {
        reject_background(f, slot_id, index, message);
        return;
    }
    let scope = f.deck.slots[index].scope;
    let baked = {
        let deck::SlotContent::Ready(doc) = &f.deck.slots[index].content else {
            f.save.writes.finished(slot_id);
            return;
        };
        persistence::bake_for_save(&mut *f.renderer, rs, scope, &doc.doc, &doc.images)
    };
    let (rgba, width, height) = match baked {
        Ok(pixels) => pixels,
        Err(message) => {
            reject_background(f, slot_id, index, message);
            return;
        }
    };
    let (capture, sidecar, metadata) = {
        let deck::SlotContent::Ready(doc) = &f.deck.slots[index].content else {
            f.save.writes.finished(slot_id);
            return;
        };
        (
            (doc.history.undo_depth(), doc.history.revision()),
            doc.sidecar_enabled
                .then(|| editor::state::payload_of(&doc.doc, &doc.images, doc.background_layer)),
            doc.source_metadata.clone(),
        )
    };
    if let deck::SlotContent::Ready(doc) = &mut f.deck.slots[index].content {
        doc.saving = true;
        doc.saving_capture = Some(capture);
        doc.save_error = None;
    }
    *f.ignore_fs_events_until = Some(Instant::now() + Duration::from_secs(5));
    tracing::info!(
        ruta = %dst.display(),
        ranura = slot_id,
        "escritura de fondo: se hornea y se escribe su archivo"
    );
    loader::spawn_save(
        loader::SaveInput {
            path: dst,
            rgba,
            width,
            height,
            jpeg_quality: f.settings.jpeg_quality,
            metadata,
            new_source: false,
            sidecar,
            ticket: loader::SaveTicket {
                target: loader::ImageInsertDest {
                    generation,
                    slot_id,
                },
                capture,
            },
        },
        f.tx.clone(),
        ctx.clone(),
    );
}

fn start_background_design(
    ctx: &egui::Context,
    rs: &RenderState,
    f: &mut EditorFrame<'_>,
    slot_id: u64,
    index: usize,
    dst: PathBuf,
) {
    let generation = f.deck.generation();
    let capture = {
        let deck::SlotContent::Ready(doc) = &f.deck.slots[index].content else {
            f.save.writes.finished(slot_id);
            return;
        };
        (doc.history.undo_depth(), doc.history.revision())
    };
    let scope = f.deck.slots[index].scope;
    let mut payload = {
        let deck::SlotContent::Ready(doc) = &f.deck.slots[index].content else {
            f.save.writes.finished(slot_id);
            return;
        };
        editor::state::payload_of(&doc.doc, &doc.images, doc.background_layer)
    };
    // Miniatura de la galería: se hornea pequeña, como el `Ctrl+S` del diseño
    // activo (la celda enseña el diseño, no una foto vieja).
    let preview = {
        let deck::SlotContent::Ready(doc) = &f.deck.slots[index].content else {
            f.save.writes.finished(slot_id);
            return;
        };
        let (pw, ph) = doc
            .doc
            .page()
            .map(|p| (p.width, p.height))
            .unwrap_or((0.0, 0.0));
        let scale = canvas_io::preview_scale(pw, ph);
        f.renderer
            .bake_page(
                &rs.device,
                &rs.queue,
                canvas_render::FxScope(scope),
                &doc.doc,
                &doc.images,
                scale,
            )
            .ok()
    };
    if let Some((rgba, w, h)) = preview {
        payload.preview = canvas_io::make_preview(&rgba, w, h);
    }
    if let deck::SlotContent::Ready(doc) = &mut f.deck.slots[index].content {
        doc.saving = true;
        doc.saving_capture = Some(capture);
        doc.save_error = None;
    }
    *f.ignore_fs_events_until = Some(Instant::now() + Duration::from_secs(5));
    tracing::info!(
        ruta = %dst.display(),
        ranura = slot_id,
        "escritura de fondo: diseño"
    );
    loader::spawn_save_design(
        loader::DesignSaveRequest {
            target: loader::ImageInsertDest {
                generation,
                slot_id,
            },
            capture,
            path: dst,
            payload,
            reserve: false,
            new_source: false,
        },
        f.tx.clone(),
        ctx.clone(),
    );
}

/// Rechazo ANTES de escribir (RAM crítica u horneado inseguro): el mensaje va
/// al banner de SU ranura —se verá al activarla— y no se reintenta hasta que
/// el usuario edite algo.
fn reject_background(f: &mut EditorFrame<'_>, slot_id: u64, index: usize, message: String) {
    if let deck::SlotContent::Ready(doc) = &mut f.deck.slots[index].content {
        doc.save_error = Some(message.clone());
        let revision = doc.history.revision();
        f.save.blocked_writes.insert(slot_id, revision);
    }
    f.save.writes.finished(slot_id);
    tracing::warn!(ranura = slot_id, motivo = %message, "escritura de fondo rechazada");
}
