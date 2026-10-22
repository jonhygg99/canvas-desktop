//! Respuestas del camino de guardado que llegan tras un diálogo: la decisión
//! de «¿guardar los cambios?» y la reserva de nombre de una ranura
//! provisional. La respuesta de una escritura vive en `saved.rs`.

use std::path::PathBuf;

use eframe::egui;

use crate::{deck, loader};

use super::super::{persistence::start_save_all_flow, AppInner, Nav, View, Workspace};

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
            DialogDecision::Cancel => {
                // Traza: el «Back no hace nada» puede ser exactamente esto —
                // un diálogo que se cancela (o se cierra) y una navegación
                // que muere con él.
                tracing::info!("diálogo de cambios sin guardar: Cancel → no se navega");
                // El usuario acaba de quedarse: el aviso de «salir está
                // esperando a…» ya no describe nada.
                if let View::Editor(state) = &mut ws.view {
                    state.notice = None;
                }
            }
            DialogDecision::Save => {
                tracing::info!("diálogo de cambios sin guardar: Save → Save all + salida diferida");
                // A05: «Save» guarda TODOS los lienzos sucios, no solo el
                // activo: se arma la cola de «Save all» (el activo se guarda
                // de inmediato con `save_requested`; el fondo lo salta y
                // guarda uno a uno el conductor de `deck_nav`). La
                // navegación/cierre diferidos esperan al drenaje en
                // `on_saved` en vez de ejecutarse tras el primer guardado.
                if let View::Editor(state) = &mut ws.view {
                    start_save_all_flow(state, &mut ws.deck, &mut ws.save);
                }
                match dialog {
                    UnsavedDialog::WindowClose => {
                        ws.save.save_requested = true;
                        ws.save.close_after_save = true;
                    }
                    UnsavedDialog::Navigate(nav) => {
                        ws.save.save_requested = true;
                        ws.save.after_save = Some(nav);
                    }
                }
            }
            DialogDecision::Discard => {
                tracing::info!("diálogo de cambios sin guardar: Discard → se sale sin guardar");
                // La navegación se aplica DESPUÉS del frame del editor: sin
                // desarmar el autoguardado, escribiría en ese único frame lo
                // que el usuario acaba de descartar.
                ws.save.autosave.on_discarded();
                match dialog {
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
                }
            }
        }
    }

    pub(super) fn on_canvas_path_reserved(
        &mut self,
        ws: &mut Workspace,
        folder: PathBuf,
        slot: u64,
        result: Result<PathBuf, canvas_io::IoError>,
    ) {
        // Libera el cerrojo PRIMERO y siempre.
        if ws.deck_ops.materializing == Some(slot) {
            ws.deck_ops.materializing = None;
        }
        if ws.deck.folder.as_deref() != Some(folder.as_path()) {
            // El archivo reservado ya no tiene dueño: se retira el hueco de
            // 0 bytes que deja (la galería lo listaría como lienzo en
            // blanco) — la baraja no volverá a esta carpeta.
            if let Ok(path) = &result {
                let _ = std::fs::remove_file(path);
            }
            tracing::warn!(
                "baraja: reserva de nombre para «{}» llegó tras cambiar de carpeta; \
             el archivo reservado queda huérfano",
                folder.display()
            );
            return;
        }
        match result {
            Ok(path) => {
                // Materializa la provisional SIN crear ninguna ranura nueva:
                // la provisional ya ocupa su sitio en la baraja y su
                // documento ya está en memoria (activo o en su `SlotDoc`).
                match ws.deck.materialize_placeholder(slot, path.clone()) {
                    Some(idx) if idx == ws.deck.active => {
                        if let View::Editor(state) = &mut ws.view {
                            state.is_design = canvas_io::is_canvas_file(&path);
                            state.doc.source_path = Some(path);
                            state.save_clicked = true;
                        }
                    }
                    Some(idx) => {
                        if let deck::SlotContent::Ready(d) = &mut ws.deck.slots[idx].content {
                            d.doc.source_path = Some(path);
                        }
                    }
                    // La provisional ya no existe (el usuario la descartó
                    // entre la petición y la respuesta): el archivo reservado
                    // es un hueco de 0 bytes sin dueño que la galería listaría
                    // como lienzo en blanco — se retira.
                    None => {
                        let _ = std::fs::remove_file(&path);
                        tracing::warn!(
                            "baraja: la ranura provisional ya no existe al reservar su nombre; \
                         se retira el archivo vacío"
                        );
                    }
                }
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
