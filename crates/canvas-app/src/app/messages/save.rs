//! Respuestas del camino de guardado: el guardado en si, la ruta elegida en
//! Guardar como..., y la reserva de nombre de una ranura provisional.

use std::path::PathBuf;

use eframe::egui;

use crate::{deck, loader};

use super::super::{
    persistence::{saved_exit_ready, start_save_all_flow},
    AppInner, Nav, View, Workspace,
};

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
            DialogDecision::Save => {
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
        result: Result<(), canvas_io::IoError>,
        new_source: bool,
        ctx: &egui::Context,
        open_after: &mut Option<Nav>,
    ) {
        if let View::Editor(state) = &mut ws.view {
            state.saving = false;
            match result {
                Ok(()) => {
                    tracing::info!("guardado OK: {}", path.display());
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
                        adopt_saved_source(state, &mut ws.deck, path);
                    }
                    // A04: el worker escribió la captura tomada al lanzar el
                    // guardado. Solo se marca como guardado si el historial
                    // sigue en esa revisión: las ediciones hechas DURANTE la
                    // escritura no están en disco y deben seguir pendientes.
                    // Sin captura (no debería pasar: `start_save` siempre la
                    // sella), se conserva el comportamiento anterior.
                    let marked = match captured {
                        Some((depth, revision)) => state.history.mark_saved_at(depth, revision),
                        None => {
                            state.history.mark_saved();
                            true
                        }
                    };
                    if !marked {
                        // El archivo contiene la captura anterior; los cambios
                        // nuevos siguen sin guardar y NO se ejecuta ningún
                        // cierre o navegación diferidos: cerrarlos perdería
                        // esos cambios sin avisar.
                        ws.save.close_after_save = false;
                        ws.save.after_save = None;
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

    /// La composición de un lienzo de vídeo terminó de renderizarse a `dst`
    /// (`duration` = segundos renderizados). Guardando en el sitio (`dst` es
    /// el propio origen), el archivo ahora ES la composición y el trim se
    /// resetea al archivo entero — si no, el próximo guardado recortaría dos
    /// veces. El primer guardado desde Download (`dst` = nombre reservado de
    /// la provisional, origen en el temporal) y el «Guardar como…» que sale
    /// del temporal adoptan `dst` como nuevo origen; el «Guardar como…» desde
    /// un archivo ya en proyecto SIGUE apuntando a su origen (es una copia).
    /// Por lo demás, mismo contrato que `on_saved`: sello A04, y los
    /// cierres/navegaciones diferidos solo se ejecutan si no hubo ediciones
    /// durante el renderizado.
    pub(super) fn on_video_composition_rendered(
        &mut self,
        ws: &mut Workspace,
        dst: PathBuf,
        result: Result<canvas_io::VideoRenderInfo, canvas_io::IoError>,
        ctx: &egui::Context,
        open_after: &mut Option<Nav>,
    ) {
        if let View::Editor(state) = &mut ws.view {
            state.saving = false;
            match result {
                Ok(info) => {
                    tracing::info!("composición renderizada OK: {}", dst.display());
                    let captured = state.saving_capture.take();
                    state.born_blank = false;
                    let duration = info.duration_secs;
                    let rendered = (info.width, info.height);
                    let src = state.doc.source_path.clone();
                    if src.as_deref() == Some(dst.as_path()) {
                        // En el sitio: el archivo ahora ES la composición (ver
                        // `reset_rendered_trim`).
                        let mut slot = state.take_slot();
                        crate::app::video_canvases::reset_rendered_trim(
                            &mut slot,
                            dst.as_path(),
                            duration,
                            rendered,
                        );
                        state.put_slot(slot);
                    } else if src
                        .as_deref()
                        .is_some_and(crate::app::video_canvases::is_staging_path)
                    {
                        // Primer editado: el origen era el temporal de Download.
                        // Se adopta el render como origen (documento + ranura) y
                        // el temporal sigue intacto en Download hasta borrarlo.
                        let slot_id = ws.deck.slots.get(ws.deck.active).map(|s| s.id);
                        let src_path = src.clone().expect("origen con staging");
                        let mut slot = state.take_slot();
                        let adopted = slot_id.is_some_and(|id| {
                            crate::app::video_canvases::adopt_rendered_video(
                                &mut ws.deck,
                                &mut slot,
                                id,
                                &src_path,
                                &dst,
                                duration,
                                rendered,
                            )
                        });
                        if !adopted {
                            // Sin proyecto (sesión sin guardar o ranura movida por
                            // el medio): el destino elegido es el nuevo hogar.
                            // Se reenlazan también las capas — si no, seguirían
                            // bebiendo del temporal y el próximo guardado
                            // recortaría dos veces sobre un archivo que quizá ya
                            // no exista.
                            if let Some(from) = slot.doc.page().ok().and_then(|page| {
                                page.layers.iter().find_map(|layer| match &layer.content {
                                    canvas_core::LayerContent::Video(link) => {
                                        link.source_path.clone()
                                    }
                                    _ => None,
                                })
                            }) {
                                crate::app::video_canvases::relink_video_doc(
                                    &mut slot, &from, &dst,
                                );
                            }
                            crate::app::video_canvases::reset_rendered_trim(
                                &mut slot, &dst, duration, rendered,
                            );
                            if let Some(id) = slot_id {
                                if let Some(s) = ws.deck.slots.iter_mut().find(|s| s.id == id) {
                                    s.path = dst.clone();
                                    s.name = dst
                                        .file_name()
                                        .map(|n| n.to_string_lossy().into_owned())
                                        .unwrap_or(s.name.clone());
                                    s.kind = crate::gallery::ItemKind::Video;
                                    s.is_placeholder = false;
                                }
                            }
                        }
                        state.put_slot(slot);
                        state.from_gallery = ws.deck.folder.clone();
                    }
                    let marked = match captured {
                        Some((depth, revision)) => state.history.mark_saved_at(depth, revision),
                        None => {
                            state.history.mark_saved();
                            true
                        }
                    };
                    if !marked {
                        ws.save.close_after_save = false;
                        ws.save.after_save = None;
                        state.save_error = Some(
                            "Rendered, but edits made during the render are still unsaved — \
                             save again to include them."
                                .into(),
                        );
                        return;
                    }
                    if (ws.save.close_after_save || ws.save.after_save.is_some())
                        && !saved_exit_ready(state, &ws.deck, &mut ws.save)
                    {
                        return;
                    }
                    if ws.save.close_after_save {
                        if ws.save.save_all_queue.is_empty() {
                            ws.save.allow_close = true;
                            ctx.send_viewport_cmd_to(ws.viewport, egui::ViewportCommand::Close);
                        }
                    } else if ws.save.save_all_queue.is_empty() {
                        if let Some(nav) = ws.save.after_save.take() {
                            *open_after = Some(nav);
                        }
                    }
                }
                Err(e) => {
                    // En el log queda el motivo (el banner solo lo muestra en
                    // la UI): sin esto, un render que falla en bucle es
                    // imposible de diagnosticar a posteriori.
                    tracing::warn!("render vídeo a {} falló: {e}", dst.display());
                    state.saving_capture = None;
                    ws.save.close_after_save = false;
                    ws.save.after_save = None;
                    state.save_error = Some(e.to_string());
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

fn adopt_saved_source(
    state: &mut crate::editor::EditorState,
    deck: &mut deck::Deck,
    path: PathBuf,
) {
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
