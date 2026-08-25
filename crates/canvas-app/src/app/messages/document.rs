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
        result: Result<PathBuf, String>,
    ) {
        let is_active = matches!(&ws.view, View::Editor(state)
        if state.doc.source_path.as_deref() == Some(old_path.as_path()));
        if is_active {
            if let View::Editor(state) = &mut ws.view {
                match result {
                    Ok(new_path) => {
                        if let Some(slot) = ws.deck.slots.get_mut(ws.deck.active) {
                            slot.path = new_path.clone();
                            slot.name = new_path
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_default();
                        }
                        state.doc.source_path = Some(new_path);
                    }
                    Err(e) => state.save_error = Some(e),
                }
            }
        } else {
            match result {
                Ok(new_path) => {
                    if let Some(slot) = ws.deck.slots.iter_mut().find(|s| s.path == old_path) {
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
        ws: &mut Workspace,
        path: PathBuf,
        result: Result<(), String>,
        open_after: &mut Option<Nav>,
    ) {
        let mut go_to_welcome = false;
        let undoable_delete = ws.deck_ops.undoable_deletes.remove(&path);
        if result.is_ok() {
            if let (Some(sidecar), View::Editor(state)) = (undoable_delete, &mut ws.view) {
                state.record_delete(editor::DeleteRecord {
                    path: path.clone(),
                    sidecar,
                });
            }
        }
        let is_active = matches!(&ws.view, View::Editor(state)
        if state.doc.source_path.as_deref() == Some(path.as_path()));
        if is_active {
            if let View::Editor(state) = &mut ws.view {
                match result {
                    Ok(()) => {
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
                    Err(e) => state.save_error = Some(e),
                }
            }
        } else {
            match result {
                Ok(()) => {
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
            ws.view = View::Welcome { error: None };
        }
    }

    pub(super) fn on_document_restored(
        &mut self,
        ws: &mut Workspace,
        path: PathBuf,
        result: Result<(), String>,
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
