//! Guardar y «guardar como»: los botones del panel y los atajos de teclado,
//! el aviso de sobrescritura destructiva y la eleccion de rama (imagen o
//! diseno) segun la extension final de la ruta elegida.
//!
//! Movido tal cual desde `editor_view_ui`, en el mismo orden: `Ctrl+Shift+S`
//! se resuelve ANTES que `Ctrl+S`.

use std::path::PathBuf;

use eframe::egui;
use eframe::egui_wgpu::RenderState;

use crate::{editor, loader};

use super::super::super::frame::EditorFrame;
use super::super::super::persistence::{
    saved_exit_ready, start_save, start_save_design, SaveContext,
};
use super::super::super::video_canvases::{video_composition_request, VideoCompositionRequest};
use super::super::super::Nav;

pub(super) fn handle_save(
    state: &mut editor::EditorState,
    ctx: &egui::Context,
    rs: &RenderState,
    f: &mut EditorFrame<'_>,
    open_next: &mut Option<Nav>,
) {
    if state.saving || state.exporting || f.save.save_path_dialog_open {
        return;
    }
    // Guardar / Guardar como: botones del panel o atajos de teclado (el
    // orden importa: Ctrl+Shift+S primero).
    let save_as = std::mem::take(&mut state.save_as_clicked)
        || ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                egui::Key::S,
            ))
        });
    let mut save = f.save.save_requested
        || std::mem::take(&mut state.save_clicked)
        || ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::COMMAND,
                egui::Key::S,
            ))
        });
    f.save.save_requested = false;

    if save_as {
        f.save.save_path_dialog_open = true;
        if state.is_design {
            loader::spawn_pick_design_path(Some(state.file_name()), f.tx.clone(), ctx.clone());
        } else if is_video_document(state) && video_composition_request(state).is_some() {
            // El documento ES un vídeo renderizable: «Guardar como…»
            // renderiza la composición (página + fondo + clip).
            loader::spawn_pick_video_export_path(
                Some(state.file_name()),
                f.tx.clone(),
                ctx.clone(),
            );
        } else if is_video_document(state) {
            // Vídeo no renderizable (capas extra, clip rotado…): lo único
            // que funciona es el póster — el diálogo de vídeo acabaría en
            // un error de formato tras elegir nombre.
            loader::spawn_pick_save_path(Some(state.file_name()), f.tx.clone(), ctx.clone());
        } else {
            loader::spawn_pick_save_path(Some(state.file_name()), f.tx.clone(), ctx.clone());
        }
        save = false;
    }
    if save {
        if !state.is_dirty() {
            // Un guardado sin cambios no reescribe nada: en JPEG,
            // recomprimir sin motivo costaría calidad. Si veníamos de un
            // diálogo de cerrar/volver, su flujo continúa — salvo que la
            // cola de «Save all» siga con fondo por guardar (A05): el
            // diferido espera al drenaje en `on_saved`, no se adelanta aquí.
            tracing::info!("documento sin cambios: no se reescribe el archivo");
            if (f.save.close_after_save || f.save.after_save.is_some())
                && saved_exit_ready(state, f.deck, f.save)
            {
                if f.save.close_after_save {
                    f.save.allow_close = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                } else if let Some(nav) = f.save.after_save.take() {
                    *open_next = Some(nav);
                }
            }
        } else if state.is_design {
            // Un diseño no se rasteriza: no hay nada destructivo que
            // avisar, ni SVG/GIF que redirigir, así que se saltan ambos
            // modales.
            match state.doc.source_path.clone() {
                Some(path) if canvas_io::is_canvas_file(&path) => {
                    let mut sctx = SaveContext {
                        renderer: f.renderer,
                        rs,
                        tx: f.tx,
                        ctx,
                        ignore_fs_events_until: f.ignore_fs_events_until,
                        scope: f.deck.slots.get(f.deck.active).map_or(0, |s| s.scope),
                    };
                    start_save_design(state, &mut sctx, path, false);
                }
                _ => {
                    f.save.save_path_dialog_open = true;
                    loader::spawn_pick_design_path(
                        Some(state.file_name()),
                        f.tx.clone(),
                        ctx.clone(),
                    );
                }
            }
        } else if is_video_document(state) {
            // El documento ES un vídeo: `Ctrl+S` renderiza la composición
            // (página + fondo + clip recortado). Con archivo real en el
            // sitio se guarda en silencio; la primera vez (provisional que
            // enlaza el temporal) al nombre reservado; si no (mp4 suelto sin
            // proyecto), diálogo para elegir dónde.
            // Sin origen en disco (se borró el temporal desde Download) no hay
            // composición que renderizar: se explica y se propone el póster,
            // que sigue en memoria. Va ANTES del `match`: si no, el diálogo de
            // vídeo sería un callejón (falla tras elegir nombre).
            if video_src_missing(state) {
                state.save_error = Some(
                    "The original clip is gone (it was deleted from Download's temporary folder). \
                     The poster can still be saved with \"Save as…\"."
                        .into(),
                );
                f.save.save_path_dialog_open = true;
                loader::spawn_pick_save_path(Some(state.file_name()), f.tx.clone(), ctx.clone());
                return;
            }
            match video_save_in_place(state, f) {
                Some(dst) => render_video_composition(state, f, ctx, dst),
                None if video_composition_request(state).is_some() => {
                    f.save.save_path_dialog_open = true;
                    loader::spawn_pick_video_export_path(
                        Some(state.file_name()),
                        f.tx.clone(),
                        ctx.clone(),
                    );
                }
                // Sin archivo en el sitio y sin composición renderizable, el
                // diálogo de vídeo sería un callejón (acaba en error de
                // formato): se propone el póster, que sí funciona.
                None => {
                    f.save.save_path_dialog_open = true;
                    loader::spawn_pick_save_path(
                        Some(state.file_name()),
                        f.tx.clone(),
                        ctx.clone(),
                    );
                }
            }
        } else {
            match state.doc.source_path.clone() {
                // SVG/GIF: no se sobrescriben nunca; se explica y se
                // redirige a «Save as…».
                Some(path) if !canvas_io::can_overwrite(&path) => {
                    f.save.readonly_prompt = Some(path);
                }
                // El documento ya no tiene NINGUNA capa de imagen (el usuario
                // borró la última foto): sobrescribir el raster aplanado
                // descartaría la copia editable y destruiría la foto original
                // sin dejar rastro. Se pide confirmación ANTES de hornear
                // (se salta también el modal de sobrescritura: este aviso es
                // más específico). Un lienzo `born_blank` —creado por la app,
                // sin foto que pudo borrarse— no recibe el aviso.
                Some(path)
                    if !state.born_blank
                        && !f.save.discard_raster_ok(&path)
                        && !crate::app::persistence::has_raster_layers(&state.doc) =>
                {
                    f.save.discard_raster_prompt = Some(path);
                }
                Some(path) => {
                    // Aviso de sobrescritura destructiva: la primera vez de
                    // cada ARCHIVO (salvo que el usuario pidiera no volver a
                    // preguntar), y NUNCA para un lienzo `born_blank` — lo
                    // creó la propia app en blanco, no hay píxeles del
                    // usuario que este primer guardado pudiera destruir.
                    if !state.born_blank
                        && !f.settings.skip_overwrite_warning
                        && !f.save.overwrite_ok(&path)
                    {
                        f.save.overwrite_dont_ask = false;
                        f.save.overwrite_prompt = Some(path);
                    } else {
                        let mut sctx = SaveContext {
                            renderer: f.renderer,
                            rs,
                            tx: f.tx,
                            ctx,
                            ignore_fs_events_until: f.ignore_fs_events_until,
                            scope: f.deck.slots.get(f.deck.active).map_or(0, |s| s.scope),
                        };
                        start_save(
                            state,
                            &mut sctx,
                            f.deck,
                            path,
                            false,
                            f.settings.jpeg_quality,
                        );
                    }
                }
                // Sin origen en disco: cae a «Guardar como…».
                None => {
                    f.save.save_path_dialog_open = true;
                    loader::spawn_pick_save_path(Some(state.file_name()), f.tx.clone(), ctx.clone())
                }
            }
        }
    }
    if let Some(path) = f.save.pending_save_as.take() {
        // La extensión final de la ruta elegida decide la rama, venga del
        // diálogo de diseño, del de imagen o del de vídeo (los tres acaban
        // en el mismo `SaveAsPicked`).
        if canvas_io::is_video_file(&path) && !state.is_design {
            match video_composition_request(state) {
                Some(request) => {
                    // Composición a un mp4 (nuevo o el propio origen): el
                    // diálogo ya eligió el destino. Mismo sello A04 que el
                    // guardado raster para conservar cambios posteriores.
                    render_video_composition_to(state, f, ctx, path, request);
                }
                None => {
                    // Sin capa de vídeo con origen no hay nada que recortar:
                    // cae al camino raster, que rechaza el mp4 con el error
                    // de formato de siempre.
                    let mut sctx = SaveContext {
                        renderer: f.renderer,
                        rs,
                        tx: f.tx,
                        ctx,
                        ignore_fs_events_until: f.ignore_fs_events_until,
                        scope: f.deck.slots.get(f.deck.active).map_or(0, |s| s.scope),
                    };
                    start_save(
                        state,
                        &mut sctx,
                        f.deck,
                        path,
                        true,
                        f.settings.jpeg_quality,
                    );
                }
            }
        } else if canvas_io::is_canvas_file(&path) {
            let mut sctx = SaveContext {
                renderer: f.renderer,
                rs,
                tx: f.tx,
                ctx,
                ignore_fs_events_until: f.ignore_fs_events_until,
                scope: f.deck.slots.get(f.deck.active).map_or(0, |s| s.scope),
            };
            start_save_design(state, &mut sctx, path, true);
        } else {
            let mut sctx = SaveContext {
                renderer: f.renderer,
                rs,
                tx: f.tx,
                ctx,
                ignore_fs_events_until: f.ignore_fs_events_until,
                scope: f.deck.slots.get(f.deck.active).map_or(0, |s| s.scope),
            };
            start_save(
                state,
                &mut sctx,
                f.deck,
                path,
                true,
                f.settings.jpeg_quality,
            );
        }
    }
}

/// ¿El clip de origen ya no está en disco (se borró el temporal desde
/// Download)? Entonces no hay composición que renderizar, pero el póster
/// sigue en memoria y «Save as» lo salva. Pura (testeable).
fn video_src_missing(state: &editor::EditorState) -> bool {
    let Ok(page) = state.doc.page() else {
        return false;
    };
    page.layers
        .iter()
        .find_map(|layer| match &layer.content {
            canvas_core::LayerContent::Video(link) => Some(link),
            _ => None,
        })
        .and_then(|link| link.source_path.clone())
        .is_some_and(|src| !src.is_file())
}

/// ¿El documento ES un vídeo (su origen es un mp4)? Solo entonces `Ctrl+S`
/// renderiza: un lienzo mixto (foto + capa de vídeo insertada) sigue el
/// camino raster de siempre (hornea el póster) aunque tenga capas de vídeo.
fn is_video_document(state: &editor::EditorState) -> bool {
    state
        .doc
        .source_path
        .as_deref()
        .is_some_and(canvas_io::is_video_file)
}

/// Destino del guardado en el sitio de un lienzo de vídeo, en un proyecto con
/// carpeta. Dos casos: (1) ranura real cuyo archivo es el propio origen — el
/// `Ctrl+S` de siempre, en el sitio; (2) primer guardado de un lienzo creado
/// desde Download: la ranura es provisional con el nombre final reservado
/// (`1.mp4`…) y el documento enlaza el temporal — se renderiza el editado a
/// ese nombre y se adopta. Un mp4 suelto abierto directo va por diálogo: es el
/// único original y no se destruye sin avisar.
fn video_save_in_place(state: &editor::EditorState, f: &EditorFrame<'_>) -> Option<PathBuf> {
    let slot = f.deck.slots.get(f.deck.active)?;
    let request = video_composition_request(state)?;
    if slot.is_placeholder {
        f.deck.folder.as_ref()?;
        // Primer guardado: el nombre reservado es el destino y el temporal es
        // el origen. Sin origen en disco no hay nada que renderizar.
        if !request.src.is_file() || !canvas_io::is_video_file(&slot.path) {
            return None;
        }
        return Some(slot.path.clone());
    }
    let src = state.doc.source_path.clone()?;
    // Un vídeo suelto sin sidecar sigue siendo el único original: diálogo.
    if f.deck.folder.is_none() && request.src == src {
        return None;
    }
    if slot.path != src || !src.is_file() {
        return None;
    }
    Some(src)
}

/// Lanza el renderizado en el sitio (el `dst` ya está decidido): si el
/// documento no es renderizable (capas extra, vídeo rotado…), se explica y se
/// redirige a «Save as…» en vez de inventar un mp4 a medias.
fn render_video_composition(
    state: &mut editor::EditorState,
    f: &mut EditorFrame<'_>,
    ctx: &egui::Context,
    dst: PathBuf,
) {
    let Some(request) = video_composition_request(state) else {
        f.save.readonly_prompt = Some(dst);
        return;
    };
    render_video_composition_to(state, f, ctx, dst, request);
}

/// Sella el estado de guardado (A04) y lanza el worker de renderizado. Como
/// `start_save`: con un render en curso no se lanza otro — dos workers
/// escribirían el mismo temporal y el mp4 saldría corrupto.
fn render_video_composition_to(
    state: &mut editor::EditorState,
    f: &mut EditorFrame<'_>,
    ctx: &egui::Context,
    dst: PathBuf,
    request: VideoCompositionRequest,
) {
    if state.saving {
        return;
    }
    // Suelta los handles propios ANTES de tocar disco: el preview (ffmpeg hijo
    // leyendo el origen) y el audio (rodio con el `File` abierto) bloquean
    // `ReplaceFileW` con 0x80070020 al guardar en el sitio. El reintento de
    // `replace_file_with_tmp` cubre lo transitorio (Drive, cierre en curso).
    state.pause_video();
    crate::audio::stop();
    tracing::info!(
        "render vídeo {}x{} desde {} (trim {:?}-{:?}) a {}",
        request.page_w,
        request.page_h,
        request.src.display(),
        request.start_s,
        request.end_s,
        dst.display()
    );
    state.saving = true;
    state.saving_capture = Some((state.history.undo_depth(), state.history.revision()));
    state.save_error = None;
    let target = loader::ImageInsertDest {
        generation: f.deck.generation(),
        slot_id: f.deck.slots.get(f.deck.active).map_or(0, |s| s.id),
    };
    let reserve = f
        .deck
        .slots
        .get(f.deck.active)
        .is_some_and(|s| s.is_placeholder && s.path == dst)
        && f.deck.folder.is_some();
    loader::spawn_video_save(
        loader::VideoSaveRequest {
            target,
            capture: state.saving_capture.expect("captured above"),
            payload: state.sidecar_payload(),
            reserve,
            composition: request.into_composition(dst),
        },
        f.tx.clone(),
        ctx.clone(),
    );
}
