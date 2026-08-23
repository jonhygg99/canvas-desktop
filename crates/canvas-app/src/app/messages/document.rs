//! Respuestas de las operaciones sobre el archivo abierto en el editor:
//! renombrar, borrar, restaurar de la papelera, y el aviso del watcher de que
//! cambio en disco.

use std::path::PathBuf;

use eframe::egui;

use crate::{deck, editor, loader};

use super::super::{App, Nav, View};

impl App {
    pub(super) fn on_document_renamed(
        &mut self,
        old_path: PathBuf,
        result: Result<PathBuf, String>,
    ) {
        let is_active = matches!(&self.view, View::Editor(state)
        if state.doc.source_path.as_deref() == Some(old_path.as_path()));
        if is_active {
            if let View::Editor(state) = &mut self.view {
                match result {
                    Ok(new_path) => {
                        // La ranura activa de la baraja lleva su
                        // propia copia de la ruta/nombre (la
                        // tira los lee de ahí, no del documento):
                        // sin esto, renombrar dejaría la tira con
                        // el nombre viejo hasta el próximo
                        // reescaneo.
                        if let Some(slot) = self.deck.slots.get_mut(self.deck.active) {
                            slot.path = new_path.clone();
                            slot.name = new_path
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_default();
                        }
                        state.doc.source_path = Some(new_path);
                    }
                    // Reutiliza el banner de error que ya existe
                    // en el panel: no hace falta un campo nuevo.
                    Err(e) => state.save_error = Some(e),
                }
            }
        } else {
            // Ranura de FONDO (cabecera del lienzo en el área
            // central, no la activa): sin `state.doc` que
            // actualizar, solo la propia ranura de la baraja —
            // mismo campo que arriba, generalizado por ruta en
            // vez de "la activa". Sin banner de error propio
            // para una ranura que no se está mirando: se
            // registra y ya.
            match result {
                Ok(new_path) => {
                    if let Some(slot) = self.deck.slots.iter_mut().find(|s| s.path == old_path) {
                        slot.path = new_path.clone();
                        slot.name = new_path
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default();
                    }
                }
                Err(e) => tracing::warn!(
                    "no se pudo renombrar {} en segundo plano: {e}",
                    old_path.display()
                ),
            }
        }
    }

    pub(super) fn on_document_deleted(
        &mut self,
        path: PathBuf,
        result: Result<(), String>,
        open_after: &mut Option<Nav>,
    ) {
        // `state` toma prestado `self.view`; no se puede
        // reasignar `self.view` mientras siga vivo, así que la
        // decisión se guarda en una variable local y se aplica
        // después de que el préstamo termine.
        let mut go_to_welcome = false;
        // `remove` (no `get`): de un solo uso — `Some(sidecar)`
        // si este borrado lo pidió el usuario directamente (no
        // como consecuencia de deshacer un `Create`), con el
        // sidecar que tenía (si tenía uno) anotado ANTES de
        // borrar. Se apila como `GlobalStep::Delete` más abajo,
        // solo si el borrado tuvo éxito.
        let undoable_delete = self.deck_ops.undoable_deletes.remove(&path);
        if result.is_ok() {
            if let (Some(sidecar), View::Editor(state)) = (undoable_delete, &mut self.view) {
                state.record_delete(editor::DeleteRecord {
                    path: path.clone(),
                    sidecar,
                });
            }
        }
        let is_active = matches!(&self.view, View::Editor(state)
        if state.doc.source_path.as_deref() == Some(path.as_path()));
        if is_active {
            if let View::Editor(state) = &mut self.view {
                match result {
                    Ok(()) => {
                        // El archivo ya no existe: no tiene
                        // sentido preguntar por cambios sin
                        // guardar (no hay dónde guardarlos). Si
                        // la baraja tiene más ranuras y la
                        // vecina ya está cargada, se salta a
                        // ella en vez de salir del editor entero
                        // — el archivo desapareció, pero el
                        // resto de la carpeta sigue teniendo
                        // sentido en pantalla.
                        let mut jumped = false;
                        if self.deck.slots.len() > 1 {
                            let removed = self.deck.active;
                            self.deck.slots.remove(removed);
                            // Sin esto los supervivientes se
                            // quedan con el `rect` viejo
                            // (calculado con la borrada
                            // todavía en la pila) hasta el
                            // próximo cambio que sí encienda
                            // el flag — se ve como un hueco
                            // vacío que nadie ocupa.
                            self.deck.layout_dirty = true;
                            let neighbor = removed.min(self.deck.slots.len().saturating_sub(1));
                            if let Some(slot) = self.deck.slots.get_mut(neighbor) {
                                if matches!(slot.content, deck::SlotContent::Ready(_)) {
                                    let deck::SlotContent::Ready(incoming) = std::mem::replace(
                                        &mut slot.content,
                                        deck::SlotContent::Active,
                                    ) else {
                                        unreachable!("comprobado justo arriba");
                                    };
                                    state.put_slot(*incoming);
                                    self.deck.active = neighbor;
                                    jumped = true;
                                }
                            }
                        }
                        if !jumped {
                            match state.from_gallery.clone() {
                                Some(folder) => *open_after = Some(Nav::Open(folder)),
                                None => go_to_welcome = true,
                            }
                        }
                    }
                    Err(e) => state.save_error = Some(e),
                }
            }
        } else {
            // Ranura de FONDO (cabecera del lienzo en el área
            // central, no la activa): borrar generaliza el mismo
            // bloque de arriba que YA quita la ranura activa de
            // `self.deck.slots` — sin salto ni pantalla de
            // bienvenida, porque el usuario no estaba mirando
            // este lienzo.
            match result {
                Ok(()) => {
                    if let Some(idx) = self.deck.slots.iter().position(|s| s.path == path) {
                        self.deck.slots.remove(idx);
                        // Si la borrada estaba ANTES de la
                        // activa en el `Vec`, todo lo posterior
                        // se desplaza un puesto — sin este
                        // ajuste `deck.active` (un índice, no un
                        // id) pasaría a apuntar a la ranura
                        // equivocada, y la que de verdad sigue
                        // activa dejaría de encajar en ninguna
                        // rama del render (ni "es la activa" ni
                        // "tiene contenido `Ready`", porque su
                        // contenido es el marcador `Active`) —
                        // su cuerpo desaparecía aunque la
                        // cabecera se siguiera pintando.
                        if idx < self.deck.active {
                            self.deck.active -= 1;
                        }
                        self.deck.layout_dirty = true;
                    }
                }
                Err(e) => {
                    tracing::warn!("no se pudo borrar {} en segundo plano: {e}", path.display())
                }
            }
        }
        if go_to_welcome {
            self.view = View::Welcome { error: None };
        }
    }

    pub(super) fn on_document_restored(
        &mut self,
        path: PathBuf,
        result: Result<(), String>,
        ctx: &egui::Context,
    ) {
        match result {
            Ok(()) => {
                // Igual que tras una `GalleryOp` (`GalleryOpDone`,
                // arriba): si la carpeta activa (baraja o galería)
                // es la del archivo restaurado, se rescanea para que
                // reaparezca como ranura/miniatura — no hace falta
                // reconstruir un `Slot` a mano.
                self.ignore_fs_events_until =
                    Some(std::time::Instant::now() + std::time::Duration::from_secs(2));
                if let Some(folder) = path.parent().map(PathBuf::from) {
                    if self.deck.folder.as_deref() == Some(folder.as_path()) {
                        loader::spawn_gallery_scan(
                            folder.clone(),
                            self.thumb_cache.clone(),
                            self.tx.clone(),
                            ctx.clone(),
                        );
                    }
                    if matches!(&self.view, View::Gallery(g) if g.folder == folder
                    || g.folder.parent() == Some(folder.as_path()))
                    {
                        self.rescan_gallery(ctx);
                        if let View::Gallery(g) = &mut self.view {
                            g.refresh_folder_lists();
                        }
                    }
                }
            }
            Err(e) => {
                tracing::warn!("no se pudo restaurar «{}»: {e}", path.display());
                if let View::Editor(state) = &mut self.view {
                    state.save_error =
                        Some(format!("Could not restore \"{}\": {e}", path.display()));
                }
            }
        }
    }

    pub(super) fn on_source_changed_on_disk(&mut self, path: PathBuf) {
        let own_save = self
            .ignore_fs_events_until
            .is_some_and(|t| std::time::Instant::now() < t);
        if !own_save {
            if let View::Editor(state) = &mut self.view {
                if state.doc.source_path.as_deref() == Some(path.as_path()) {
                    state.external_change = true;
                }
            }
        }
    }
}
