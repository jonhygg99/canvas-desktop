//! Respuestas del camino de guardado: el guardado en si, la ruta elegida en
//! Guardar como..., y la reserva de nombre de una ranura provisional.

use std::path::PathBuf;

use eframe::egui;

use crate::{deck, loader};

use super::super::{App, Nav, View};

impl App {
    pub(super) fn on_saved(
        &mut self,
        path: PathBuf,
        result: Result<(), String>,
        new_source: bool,
        ctx: &egui::Context,
        open_after: &mut Option<Nav>,
    ) {
        if let View::Editor(state) = &mut self.view {
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
                    self.ignore_fs_events_until =
                        Some(std::time::Instant::now() + std::time::Duration::from_secs(2));
                    self.watcher = None;
                    // Refresca la miniatura de la tira (y de la
                    // galería, si está abierta ahí) con el
                    // contenido recién guardado — sin esto, un
                    // diseño añadido o editado en esta misma
                    // sesión se queda con su miniatura en blanco
                    // hasta volver a abrir la carpeta, porque
                    // nada más dispara un rescan.
                    if let Some(folder) = path.parent() {
                        loader::spawn_single_thumb(
                            folder.to_path_buf(),
                            path.clone(),
                            self.thumb_cache.clone(),
                            self.tx.clone(),
                            ctx.clone(),
                        );
                    }
                    if new_source {
                        state.doc.source_path = Some(path);
                    }
                    // «Save all»: si lo que se acaba de guardar
                    // era el frente de la cola, avanza. Se
                    // comprueba por id de ranura, no por ruta:
                    // más robusto ante un renombrado en vuelo.
                    if self.save.save_all_queue.first().is_some_and(|&id| {
                        self.deck.slots.get(self.deck.active).map(|s| s.id) == Some(id)
                    }) {
                        self.save.save_all_queue.remove(0);
                        self.save.save_all_attempted = false;
                    }
                    if self.save.close_after_save {
                        self.save.allow_close = true;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    } else if let Some(nav) = self.save.after_save.take() {
                        *open_after = Some(nav);
                    }
                }
                Err(e) => {
                    self.save.close_after_save = false;
                    self.save.after_save = None;
                    // No hace falta esperar al frame siguiente
                    // para que el chequeo de la cola detecte el
                    // fallo: se aborta el lote aquí mismo si era
                    // su frente el que acaba de fallar.
                    if self.save.save_all_queue.first().is_some_and(|&id| {
                        self.deck.slots.get(self.deck.active).map(|s| s.id) == Some(id)
                    }) {
                        self.save.save_all_queue.clear();
                        self.save.save_all_attempted = false;
                    }
                    state.save_error = Some(e);
                }
            }
        }
    }

    pub(super) fn on_save_as_picked(&mut self, path: Option<PathBuf>) {
        self.save.pending_save_as = path;
    }

    pub(super) fn on_canvas_path_reserved(
        &mut self,
        folder: PathBuf,
        slot: u64,
        result: Result<PathBuf, String>,
    ) {
        // Libera el cerrojo PRIMERO y siempre, para que un
        // guardián de carpeta obsoleta (más abajo) no lo deje
        // atascado.
        if self.deck_ops.materializing == Some(slot) {
            self.deck_ops.materializing = None;
        }
        // Guarda de obsolescencia, igual que `SlotLoaded`: si la
        // baraja ya no es esta carpeta, el archivo reservado (0
        // bytes) queda huérfano — se registra, no se limpia
        // (borrarlo de fondo podría chocar con un usuario que
        // reabrió justo esa carpeta).
        if self.deck.folder.as_deref() != Some(folder.as_path()) {
            tracing::warn!(
                "baraja: reserva de nombre para «{}» llegó tras cambiar de carpeta; \
             el archivo reservado queda huérfano",
                folder.display()
            );
            return;
        }
        match result {
            Ok(path) => {
                let Some(idx) = self.deck.find_by_id(slot) else {
                    tracing::warn!(
                        "baraja: la ranura provisional ya no existe al reservar su nombre"
                    );
                    return;
                };
                // Mismo patrón que `DocumentRenamed`: la tira lee
                // ruta y nombre de la RANURA, no del documento.
                if let Some(s) = self.deck.slots.get_mut(idx) {
                    s.name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    s.path = path.clone();
                    s.is_placeholder = false;
                }
                if idx == self.deck.active {
                    if let View::Editor(state) = &mut self.view {
                        // `state.is_design` refleja la extensión
                        // REAL reservada (`settings.new_canvas_format`
                        // en el momento de crear la ranura), no un
                        // `true` fijo: la mayoría de lienzos nuevos
                        // hoy son un raster, no un diseño autónomo.
                        state.is_design = canvas_io::is_canvas_file(&path);
                        state.doc.source_path = Some(path);
                        // El bloque de guardado normal, más abajo
                        // en este mismo frame, toma la rama de
                        // diseño y llama a `start_save_design`
                        // con horneado de miniatura, ventana de
                        // gracia y `mark_saved()` de siempre —
                        // gratis, sin duplicar nada de eso aquí.
                        state.save_clicked = true;
                    }
                } else if let deck::SlotContent::Ready(d) = &mut self.deck.slots[idx].content {
                    // El usuario saltó a otro lienzo mientras la
                    // reserva estaba en vuelo: se deja lista para
                    // guardarse la próxima vez (Ctrl+S al volver
                    // a ella, o Save All), sin forzarlo ahora.
                    d.doc.source_path = Some(path);
                }
                // Relleno automático: siempre queda una
                // provisional lista al final, con o sin éxito
                // arriba.
                self.deck.push_placeholder(
                    self.settings.last_page_size,
                    self.settings.new_canvas_format.extension(),
                );
            }
            Err(e) => {
                self.deck_ops.materialize_blocked = Some(slot);
                tracing::warn!("no se pudo crear el archivo del nuevo lienzo: {e}");
                if let View::Editor(state) = &mut self.view {
                    state.save_error =
                        Some(format!("Could not create a file for the new canvas: {e}"));
                }
            }
        }
    }
}
