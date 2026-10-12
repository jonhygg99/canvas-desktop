//! Respuestas de las operaciones sobre el archivo abierto en el editor:
//! renombrar, borrar, restaurar de la papelera, y el aviso del watcher de que
//! cambio en disco.

use std::path::PathBuf;

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
        let own_save = ws
            .ignore_fs_events_until
            .is_some_and(|t| std::time::Instant::now() < t);
        if !own_save {
            if let View::Editor(state) = &mut ws.view {
                if state.doc.source_path.as_deref() == Some(path.as_path()) {
                    state.external_change = true;
                }
            }
        }
    }
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
