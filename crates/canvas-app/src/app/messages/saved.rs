//! La respuesta de una escritura (`AppMsg::Saved`): a qué ranura pertenece y
//! qué se hace con ella.
//!
//! La escritura puede ser del lienzo ACTIVO o de una ranura de FONDO (la cola
//! única de `app/write_queue.rs` escribe también lo que quedó sucio al cambiar
//! de baraja). El `ticket` de la petición —ranura + captura del historial— es
//! lo que decide: si ya no es la ranura en pantalla, el archivo se aplica a SU
//! ranura (`on_background_saved`) sin tocar el documento activo ni la
//! navegación diferida.

use std::path::PathBuf;

use eframe::egui;

use super::super::{persistence::saved_exit_ready, AppInner, Nav, View, Workspace};
use super::background_saved::BackgroundWriteDone;
use crate::{deck, loader};

pub(super) struct SavedFile {
    pub path: PathBuf,
    pub result: Result<(), canvas_io::IoError>,
    pub new_source: bool,
    pub ticket: loader::SaveTicket,
}

impl AppInner {
    pub(super) fn on_saved(
        &mut self,
        ws: &mut Workspace,
        saved: SavedFile,
        ctx: &egui::Context,
        open_after: &mut Option<Nav>,
    ) {
        let SavedFile {
            path,
            result,
            new_source,
            ticket,
        } = saved;
        // La escritura terminó: la cola puede seguir, sea cual sea su dueño.
        ws.save.writes.finished(ticket.target.slot_id);
        let active_slot = ws.deck.slots.get(ws.deck.active).map(|slot| slot.id);
        if !ticket.target.is_current(ws.deck.generation(), active_slot) {
            // El usuario cambió de lienzo (o de proyecto) mientras se
            // escribía: el archivo se aplica a SU ranura sin tocar el
            // documento en pantalla ni la navegación diferida.
            let kind = canvas_io::is_canvas_file(&path).then_some(crate::gallery::ItemKind::Design);
            self.on_background_saved(
                ws,
                BackgroundWriteDone {
                    generation: ticket.target.generation,
                    slot_id: ticket.target.slot_id,
                    path,
                    result: result.map_err(|error| error.to_string()),
                    new_source,
                    capture: ticket.capture,
                    kind,
                },
                ctx,
            );
            return;
        }
        if let View::Editor(state) = &mut ws.view {
            state.saving = false;
            // Solo el autoguardado necesita el resultado para su backoff: un
            // fallo de un `Ctrl+S` manual no debe anunciarse como fallo del
            // autoguardado (el banner ya lo explica).
            let autosave_own = std::mem::take(&mut ws.save.autosave_in_flight);
            match result {
                Ok(()) => {
                    if autosave_own {
                        ws.save.autosave.on_saved();
                    }
                    tracing::info!("guardado OK: {}", path.display());
                    // Bytes que este guardado dejó en disco. El watcher los
                    // comparará con el tamaño del archivo cuando llegue un
                    // evento fuera de la ventana de gracia: un cliente de
                    // sincronización de nube toca el archivo después de
                    // subirlo (mismo tamaño, `mtime` nuevo) y ese evento no
                    // debe levantar el banner de «cambió fuera» ni, con él,
                    // pausar el autoguardado (ver `Workspace::saved_len`).
                    ws.saved_len = std::fs::metadata(&path)
                        .ok()
                        .map(|meta| (path.clone(), meta.len()));
                    // El disco contiene por construcción lo que se acaba de
                    // escribir, así que un banner de «cambió fuera» previo
                    // (típico: un evento propio del guardado anterior que
                    // cayó fuera de la ventana de gracia) es obsoleto aquí.
                    // Nada más lo limpiaba: el aviso se quedaba pegado para
                    // siempre en el mismo lienzo (`messages/document.rs`).
                    if state.external_change {
                        tracing::info!(
                            "guardado OK: se limpia el banner de cambio externo obsoleto"
                        );
                        state.external_change = false;
                    }
                    let captured = state.saving_capture.take();
                    // A partir de este guardado ya hay píxeles
                    // del usuario en disco: el próximo `Ctrl+S`
                    // vuelve a pedir confirmación si sobrescribe.
                    state.born_blank = false;
                    // Los eventos de disco inminentes son de este
                    // guardado: ventana de gracia y watcher nuevo
                    // (la sustitución atómica puede invalidarlo).
                    ws.ignore_fs_events_until =
                        Some(std::time::Instant::now() + std::time::Duration::from_secs(2));
                    ws.watcher = None;
                    // Refresca la miniatura de la tira (y de la
                    // galería, si está abierta ahí) con el
                    // contenido recién guardado.
                    if let Some(folder) = path.parent() {
                        loader::spawn_single_thumb(
                            folder.to_path_buf(),
                            path.clone(),
                            self.thumb_cache.clone(),
                            ws.tx.clone(),
                            ctx.clone(),
                        );
                    }
                    if new_source {
                        // La app acaba de crear este archivo (lienzo nuevo
                        // materializado o «Save as»): no hay un original del
                        // usuario que proteger, así que ni `Ctrl+S` ni el
                        // autoguardado vuelven a pedir la confirmación de
                        // sobrescritura sobre él.
                        ws.save.mark_app_written(&path);
                        adopt_saved_source(state, &mut ws.deck, path);
                    }
                    // A04: el worker escribió la captura tomada al lanzar el
                    // guardado. Solo se marca como guardado si el historial
                    // sigue en esa revisión: las ediciones hechas DURANTE la
                    // escritura no están en disco y deben seguir pendientes.
                    // Sin captura (no debería pasar: `start_save` siempre la
                    // sella), se conserva el comportamiento anterior.
                    let (depth, revision) = captured.unwrap_or(ticket.capture);
                    let marked = state.history.mark_saved_at(depth, revision);
                    tracing::info!(
                        marcado = marked,
                        sucio = state.is_dirty(),
                        "guardado: estado del historial tras la escritura"
                    );
                    if !marked {
                        // El archivo contiene la captura anterior; los cambios
                        // nuevos siguen sin guardar y NO se ejecuta ningún
                        // cierre o navegación diferidos: cerrarlos perdería
                        // esos cambios sin avisar.
                        ws.save.close_after_save = false;
                        ws.save.after_save = None;
                        ws.save.save_all_queue.clear();
                        ws.save.save_all_attempted = false;
                        state.save_error = Some(
                            "Saved, but edits made during the save are still unsaved — \
                             save again to include them."
                                .into(),
                        );
                        return;
                    }
                    // «Save all»: si lo que se acaba de guardar
                    // era el frente de la cola, avanza. Se
                    // comprueba por id de ranura, no por ruta.
                    if ws.save.save_all_queue.first().is_some_and(|&id| {
                        ws.deck.slots.get(ws.deck.active).map(|s| s.id) == Some(id)
                    }) {
                        ws.save.save_all_queue.remove(0);
                        ws.save.save_all_attempted = false;
                    }
                    // A05: la navegación/cierre diferidos esperan a que la
                    // cola de «Save all» se vacíe: con varios sucios, el
                    // primer `Saved` es solo el activo y el conductor de
                    // `deck_nav` sigue saltando y guardando el fondo. Sin
                    // esta espera, salir con «Save» perdería el fondo tras
                    // guardar solo el activo.
                    if (ws.save.close_after_save || ws.save.after_save.is_some())
                        && !saved_exit_ready(state, &ws.deck, &mut ws.save)
                    {
                        return;
                    }
                    if ws.save.close_after_save {
                        if ws.save.save_all_queue.is_empty() {
                            ws.save.allow_close = true;
                            // Cierra LA VENTANA de este workspace, no la app.
                            // Al viewport propio (no al del pase actual): este
                            // mensaje puede drenarse desde el pase de la raíz y
                            // un Close pelado cerraría la app entera.
                            ctx.send_viewport_cmd_to(ws.viewport, egui::ViewportCommand::Close);
                        }
                        // Cola sin vaciar: el conductor sigue guardando el
                        // fondo; el cierre espera al drenaje.
                    } else if ws.save.save_all_queue.is_empty() {
                        if let Some(nav) = ws.save.after_save.take() {
                            *open_after = Some(nav);
                        }
                    }
                }
                Err(e) => {
                    if autosave_own {
                        // Reintentar un destino que no admite escritura (solo
                        // lectura, permisos, disco lleno) cuesta un horneado
                        // completo y otro banner por intento, y no lo arregla:
                        // solo se reintenta lo transitorio (RAM, GPU, horneado
                        // inestable).
                        if crate::app::autosave::is_permanent_save_error(&e) {
                            ws.save.autosave.give_up();
                        } else {
                            ws.save.autosave.on_failed(std::time::Instant::now());
                        }
                    }
                    tracing::warn!(
                        ruta = %path.display(),
                        error = %e,
                        "guardado FALLÓ: se cancelan cierre y navegación diferidos"
                    );
                    state.saving_capture = None;
                    ws.save.close_after_save = false;
                    ws.save.after_save = None;
                    if ws.save.save_all_queue.first().is_some_and(|&id| {
                        ws.deck.slots.get(ws.deck.active).map(|s| s.id) == Some(id)
                    }) {
                        ws.save.save_all_queue.clear();
                        ws.save.save_all_attempted = false;
                    }
                    state.save_error = Some(e.to_string());
                }
            }
        }
    }
}

fn adopt_saved_source(
    state: &mut crate::editor::EditorState,
    deck: &mut deck::Deck,
    path: PathBuf,
) {
    state.canvas_name = None;
    state.is_design = canvas_io::is_canvas_file(&path);
    if let Some(slot) = deck.slots.get_mut(deck.active) {
        slot.path = path.clone();
        slot.name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        slot.is_placeholder = false;
        slot.kind = if state.is_design {
            crate::gallery::ItemKind::Design
        } else if canvas_io::is_video_file(&path) {
            crate::gallery::ItemKind::Video
        } else {
            crate::gallery::ItemKind::Image
        };
    }
    state.doc.source_path = Some(path);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::EditorState;

    #[test]
    fn saved_image_uses_raster_format_and_new_slot_path() {
        let old = PathBuf::from("old.canvas");
        let new = PathBuf::from("new.png");
        let mut state = EditorState::new_blank(20.0, 20.0);
        state.doc.source_path = Some(old.clone());
        let mut deck = deck::Deck::single(old);
        adopt_saved_source(&mut state, &mut deck, new.clone());
        assert!(!state.is_design);
        assert_eq!(state.doc.source_path, Some(new.clone()));
        assert_eq!(deck.slots[0].path, new);
        assert_eq!(deck.slots[0].name, "new.png");
    }

    #[test]
    fn successful_design_save_adopts_design_format() {
        let old = PathBuf::from("photo.png");
        let new = PathBuf::from("photo.canvas");
        let mut state = EditorState::new_blank_image(20.0, 20.0);
        state.doc.source_path = Some(old.clone());
        let mut deck = deck::Deck::single(old);
        adopt_saved_source(&mut state, &mut deck, new.clone());
        assert!(state.is_design);
        assert_eq!(state.doc.source_path, Some(new));
    }
}
