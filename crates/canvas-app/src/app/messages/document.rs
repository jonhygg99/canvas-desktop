//! Respuestas de las operaciones sobre el archivo abierto en el editor:
//! renombrar, borrar, restaurar de la papelera, y el aviso del watcher de que
//! cambio en disco.

use std::path::{Path, PathBuf};

use eframe::egui;

use crate::{deck, editor, loader};

use super::super::{AppInner, Nav, View, Workspace};

impl AppInner {
    pub(super) fn on_document_renamed(
        &mut self,
        ws: &mut Workspace,
        old_path: PathBuf,
        result: Result<PathBuf, canvas_io::IoError>,
    ) {
        let is_active = matches!(&ws.view, View::Editor(state)
        if state.doc.source_path.as_deref() == Some(old_path.as_path()));
        if is_active {
            if let View::Editor(state) = &mut ws.view {
                match result {
                    Ok(new_path) => {
                        if let Some(slot) = ws.deck.slots.get_mut(ws.deck.active) {
                            rename_slot(slot, &new_path);
                        }
                        state.doc.source_path = Some(new_path);
                    }
                    Err(e) => state.save_error = Some(e.to_string()),
                }
            }
        } else {
            match result {
                Ok(new_path) => {
                    if let Some(slot) = ws.deck.slots.iter_mut().find(|s| s.path == old_path) {
                        rename_slot(slot, &new_path);
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
        ws: &mut Workspace,
        path: PathBuf,
        result: Result<editor::DeleteRecord, canvas_io::IoError>,
        warning: Option<String>,
        open_after: &mut Option<Nav>,
    ) {
        let mut go_to_welcome = false;
        let undoable_delete = ws.deck_ops.undoable_deletes.remove(&path);
        if undoable_delete {
            if let (Ok(record), View::Editor(state)) = (&result, &mut ws.view) {
                state.record_delete(record.clone());
            }
        }
        let is_active = matches!(&ws.view, View::Editor(state)
        if state.doc.source_path.as_deref() == Some(path.as_path()));
        if is_active {
            if let View::Editor(state) = &mut ws.view {
                match result {
                    Ok(_) => {
                        let mut jumped = false;
                        if ws.deck.slots.len() > 1 {
                            let removed = ws.deck.active;
                            ws.deck.slots.remove(removed);
                            ws.deck.layout_dirty = true;
                            let neighbor = removed.min(ws.deck.slots.len().saturating_sub(1));
                            if let Some(slot) = ws.deck.slots.get_mut(neighbor) {
                                if matches!(slot.content, deck::SlotContent::Ready(_)) {
                                    let deck::SlotContent::Ready(incoming) = std::mem::replace(
                                        &mut slot.content,
                                        deck::SlotContent::Active,
                                    ) else {
                                        unreachable!("comprobado justo arriba");
                                    };
                                    state.put_slot(*incoming);
                                    ws.deck.active = neighbor;
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
                    Err(e) => state.save_error = Some(e.to_string()),
                }
            }
        } else {
            match result {
                Ok(_) => {
                    if let Some(idx) = ws.deck.slots.iter().position(|s| s.path == path) {
                        ws.deck.slots.remove(idx);
                        if idx < ws.deck.active {
                            ws.deck.active -= 1;
                        }
                        ws.deck.layout_dirty = true;
                    }
                }
                Err(e) => {
                    tracing::warn!("no se pudo borrar {} en segundo plano: {e}", path.display())
                }
            }
        }
        if go_to_welcome {
            ws.view = View::Welcome {
                error: None,
                failed_path: None,
                failed_kind: None,
            };
        }
        // El archivo se trasteó pero su sidecar no pudo acompañarlo: hay que
        // decírselo, porque el deshacer solo traerá el archivo.
        if let Some(warning) = warning {
            if let View::Editor(state) = &mut ws.view {
                state.save_error = Some(warning);
            }
        }
    }

    pub(super) fn on_document_restored(
        &mut self,
        ws: &mut Workspace,
        path: PathBuf,
        result: Result<(), canvas_io::IoError>,
        warning: Option<String>,
        ctx: &egui::Context,
    ) {
        match result {
            Ok(()) => {
                ws.ignore_fs_events_until =
                    Some(std::time::Instant::now() + std::time::Duration::from_secs(2));
                if let Some(folder) = path.parent().map(PathBuf::from) {
                    if ws.deck.folder.as_deref() == Some(folder.as_path()) {
                        loader::spawn_gallery_scan(
                            folder.clone(),
                            self.thumb_cache.clone(),
                            ws.tx.clone(),
                            ctx.clone(),
                        );
                    }
                    if matches!(&ws.view, View::Gallery(g) if g.is_affected_by(&folder)) {
                        self.rescan_gallery(ws, ctx);
                        if let View::Gallery(g) = &mut ws.view {
                            g.refresh_folder_lists();
                        }
                    }
                }
            }
            Err(e) => {
                tracing::warn!("no se pudo restaurar «{}»: {e}", path.display());
                if let View::Editor(state) = &mut ws.view {
                    state.save_error =
                        Some(format!("Could not restore \"{}\": {e}", path.display()));
                }
            }
        }
        // El archivo volvió; sus capas editables siguen en la papelera.
        if let Some(warning) = warning {
            if let View::Editor(state) = &mut ws.view {
                state.save_error = Some(warning);
            }
        }
    }

    pub(super) fn on_source_changed_on_disk(&mut self, ws: &mut Workspace, path: PathBuf) {
        let within_grace = ws
            .ignore_fs_events_until
            .is_some_and(|t| std::time::Instant::now() < t);
        // Fuera de la ventana de gracia, un evento con EXACTAMENTE los bytes
        // que dejó nuestro último guardado no es una edición ajena: es un
        // toque de metadatos (el cliente de sincronización de la carpeta, que
        // sube el archivo y le cambia el `mtime`) o un evento propio que llegó
        // tarde. Antes eso levantaba el banner de «cambió fuera» y, con él, el
        // autoguardado quedaba pausado para siempre (ver `Workspace::saved_len`
        // y el aviso del banner).
        let same_size = same_saved_size(
            ws.saved_len.as_ref(),
            &path,
            std::fs::metadata(&path).ok().map(|meta| meta.len()),
        );
        let own_save = within_grace || same_size;
        if let View::Editor(state) = &mut ws.view {
            // Traza de diagnóstico (Task 1 del plan): el bug del banner ⚠ es
            // justo la discriminación entre «lo escribí yo» y «alguien más
            // tocó el archivo», así que el log deja las piezas: qué evento
            // llegó, qué documento está abierto, si caía dentro de la ventana
            // de gracia y si el tamaño es el que dejó nuestro guardado.
            tracing::info!(
                evento = %path.display(),
                abierto = ?state.doc.source_path.as_deref().map(|p| p.display().to_string()),
                gracia = within_grace,
                mismo_tamano = same_size,
                propio = own_save,
                "watcher: evento de disco sobre el archivo del editor"
            );
            if should_flag_external_change(state.doc.source_path.as_deref(), &path, own_save) {
                state.external_change = true;
                tracing::info!("watcher: se levanta el banner de cambio externo");
            } else if same_size
                && state.external_change
                && state.doc.source_path.as_deref() == Some(path.as_path())
            {
                // El archivo en disco vuelve a tener exactamente los bytes del
                // último guardado: el aviso que hubiera levantado un evento
                // anterior (propio, o del cliente de sincronización) ya no
                // describe nada. Se retira sin preguntar, y con él vuelve el
                // autoguardado.
                state.external_change = false;
                tracing::info!(
                    "watcher: el archivo tiene los bytes del último guardado; se retira el banner"
                );
            }
        }
    }
}

/// Decide si un evento de disco debe levantar el banner de «este archivo
/// cambió fuera de Canvas Desktop»: solo el documento ABIERTO, y solo si el
/// evento no es nuestro (ventana de gracia o mismo tamaño que el último
/// guardado propio: la sustitución atómica, el reescaneo de miniaturas y el
/// cliente de sincronización de la carpeta generan eventos propios o de
/// metadatos). Pura para probarla sin watcher, disco ni reloj.
fn should_flag_external_change(open: Option<&Path>, event: &Path, own_write: bool) -> bool {
    !own_write && open == Some(event)
}

/// ¿El archivo en disco tiene EXACTAMENTE los bytes que dejó el último
/// guardado propio de este documento? `saved` es (ruta guardada, tamaño) y
/// `disk_len` el tamaño actual leído del disco (`None` si no se pudo leer).
/// Pura (tabla de tests): separa la DECISIÓN de la lectura del disco.
fn same_saved_size(saved: Option<&(PathBuf, u64)>, event: &Path, disk_len: Option<u64>) -> bool {
    saved.is_some_and(|(saved_path, len)| saved_path == event && Some(*len) == disk_len)
}

fn rename_slot(slot: &mut deck::Slot, path: &std::path::Path) {
    slot.path = path.to_owned();
    slot.name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    match &mut slot.content {
        deck::SlotContent::Ready(doc) => doc.doc.source_path = Some(path.to_owned()),
        deck::SlotContent::Loading => slot.content = deck::SlotContent::Idle,
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_change_only_flags_the_open_document_outside_own_save() {
        let open = Path::new("fotos/a.png");
        // (documento abierto, evento, ¿dentro del guardado propio?, esperado)
        let cases = [
            (Some(open), Path::new("fotos/a.png"), false, true),
            (Some(open), Path::new("fotos/a.png"), true, false),
            (Some(open), Path::new("fotos/b.png"), false, false),
            (None, Path::new("fotos/a.png"), false, false),
        ];
        for (open, event, own_save, expected) in cases {
            assert_eq!(
                should_flag_external_change(open, event, own_save),
                expected,
                "abierto={open:?} evento={event:?} propio={own_save}"
            );
        }
    }

    /// Un toque del cliente de sincronización de la nube deja el MISMO tamaño
    /// que escribió la app: no es un cambio de contenido y no debe levantar el
    /// banner (que además pausa el autoguardado). Un tamaño distinto sí.
    #[test]
    fn an_event_with_the_bytes_we_wrote_is_not_an_external_change() {
        let saved_path = PathBuf::from("fotos/a.png");
        let other = PathBuf::from("fotos/b.png");
        let saved = Some((saved_path.clone(), 8_192_u64));
        // (ruta del evento, bytes en disco, esperado)
        let cases = [
            (&saved_path, Some(8_192_u64), true),
            (&saved_path, Some(8_193_u64), false),
            (&saved_path, None, false),
            (&other, Some(8_192_u64), false),
        ];
        for (event, disk_len, expected) in cases {
            assert_eq!(
                same_saved_size(saved.as_ref(), event, disk_len),
                expected,
                "evento={event:?} bytes={disk_len:?}"
            );
        }
        assert!(!same_saved_size(None, &saved_path, Some(8_192)));
    }

    #[test]
    fn renaming_a_loaded_slot_updates_the_path_used_by_its_next_save() {
        let old = PathBuf::from("old.png");
        let new = PathBuf::from("renamed.png");
        let mut state = editor::EditorState::new_blank_image(20.0, 20.0);
        state.doc.source_path = Some(old.clone());
        let mut deck = deck::Deck::single(old);
        deck.slots[0].content = deck::SlotContent::Ready(Box::new(state.take_slot()));
        rename_slot(&mut deck.slots[0], &new);
        let deck::SlotContent::Ready(doc) = &deck.slots[0].content else {
            panic!("loaded slot");
        };
        assert_eq!(doc.doc.source_path.as_ref(), Some(&new));
        assert_eq!(deck.slots[0].path, new);
    }
}
