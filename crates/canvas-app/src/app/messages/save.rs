//! Respuestas del camino de guardado: el guardado en si, la ruta elegida en
//! Guardar como..., y la reserva de nombre de una ranura provisional.

use std::path::PathBuf;

use eframe::egui;

use crate::{deck, loader};

use super::super::{AppInner, Nav, View, Workspace};

impl AppInner {
    /// Respuesta del diálogo «¿guardar los cambios?» lanzado en un hilo
    /// (ver `confirm_window_close` y `request_nav`). Aplica la decisión
    /// sobre lo que había pendiente detrás del diálogo.
    pub(super) fn on_unsaved_dialog_answer(
        &mut self,
        ws: &mut Workspace,
        decision: loader::DialogDecision,
        open_after: &mut Option<Nav>,
        ctx: &egui::Context,
    ) {
        use crate::app::UnsavedDialog;
        use loader::DialogDecision;

        let Some(dialog) = ws.unsaved_dialog.take() else {
            return;
        };
        match decision {
            DialogDecision::Cancel => {}
            DialogDecision::Save => match dialog {
                UnsavedDialog::WindowClose => {
                    ws.save.save_requested = true;
                    ws.save.close_after_save = true;
                }
                UnsavedDialog::Navigate(nav) => {
                    ws.save.save_requested = true;
                    ws.save.after_save = Some(nav);
                }
            },
            DialogDecision::Discard => match dialog {
                UnsavedDialog::WindowClose => {
                    ws.save.allow_close = true;
                    if ws.viewport == egui::ViewportId::ROOT {
                        // La raíz se cierra con la app entera.
                        ctx.send_viewport_cmd_to(
                            egui::ViewportId::ROOT,
                            egui::ViewportCommand::Close,
                        );
                    } else {
                        ws.close_requested = true;
                    }
                }
                UnsavedDialog::Navigate(nav) => {
                    *open_after = Some(nav);
                }
            },
        }
    }

    pub(super) fn on_saved(
        &mut self,
        ws: &mut Workspace,
        path: PathBuf,
        result: Result<(), String>,
        new_source: bool,
        ctx: &egui::Context,
        open_after: &mut Option<Nav>,
    ) {
        if let View::Editor(state) = &mut ws.view {
            state.saving = false;
            match result {
                Ok(()) => {
                    tracing::info!("guardado OK: {}", path.display());
                    state.history.mark_saved();
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
                        state.doc.source_path = Some(path);
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
                    if ws.save.close_after_save {
                        ws.save.allow_close = true;
                        // Cierra LA VENTANA de este workspace, no la app.
                        // Al viewport propio (no al del pase actual): este
                        // mensaje puede drenarse desde el pase de la raíz y
                        // un Close pelado cerraría la app entera.
                        ctx.send_viewport_cmd_to(ws.viewport, egui::ViewportCommand::Close);
                    } else if let Some(nav) = ws.save.after_save.take() {
                        *open_after = Some(nav);
                    }
                }
                Err(e) => {
                    ws.save.close_after_save = false;
                    ws.save.after_save = None;
                    if ws.save.save_all_queue.first().is_some_and(|&id| {
                        ws.deck.slots.get(ws.deck.active).map(|s| s.id) == Some(id)
                    }) {
                        ws.save.save_all_queue.clear();
                        ws.save.save_all_attempted = false;
                    }
                    state.save_error = Some(e);
                }
            }
        }
    }

    pub(super) fn on_canvas_path_reserved(
        &mut self,
        ws: &mut Workspace,
        folder: PathBuf,
        slot: u64,
        result: Result<PathBuf, String>,
    ) {
        // Libera el cerrojo PRIMERO y siempre.
        if ws.deck_ops.materializing == Some(slot) {
            ws.deck_ops.materializing = None;
        }
        if ws.deck.folder.as_deref() != Some(folder.as_path()) {
            tracing::warn!(
                "baraja: reserva de nombre para «{}» llegó tras cambiar de carpeta; \
             el archivo reservado queda huérfano",
                folder.display()
            );
            return;
        }
        match result {
            Ok(path) => {
                let Some(idx) = ws.deck.find_by_id(slot) else {
                    tracing::warn!(
                        "baraja: la ranura provisional ya no existe al reservar su nombre"
                    );
                    return;
                };
                if let Some(s) = ws.deck.slots.get_mut(idx) {
                    s.name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    s.path = path.clone();
                    s.is_placeholder = false;
                }
                if idx == ws.deck.active {
                    if let View::Editor(state) = &mut ws.view {
                        state.is_design = canvas_io::is_canvas_file(&path);
                        state.doc.source_path = Some(path);
                        state.save_clicked = true;
                    }
                } else if let deck::SlotContent::Ready(d) = &mut ws.deck.slots[idx].content {
                    d.doc.source_path = Some(path);
                }
                ws.deck.push_placeholder(
                    self.settings.last_page_size,
                    self.settings.new_canvas_format.extension(),
                );
            }
            Err(e) => {
                ws.deck_ops.materialize_blocked = Some(slot);
                tracing::warn!("no se pudo crear el archivo del nuevo lienzo: {e}");
                if let View::Editor(state) = &mut ws.view {
                    state.save_error =
                        Some(format!("Could not create a file for the new canvas: {e}"));
                }
            }
        }
    }
}
