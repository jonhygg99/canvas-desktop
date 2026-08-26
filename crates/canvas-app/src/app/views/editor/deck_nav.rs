//! Resolucion de todo lo que quedo pendiente del frame: las acciones de la
//! tira y de la cabecera de un lienzo, el salto a otra ranura de la baraja, el
//! lote de «Save all», y los pasos de deshacer/rehacer GLOBAL que esperaban a
//! que su ranura fuese la activa.
//!
//! Movido tal cual desde `editor_view_ui`, en el mismo orden.

use std::time::Instant;

use eframe::egui;

use crate::{deck, deck_strip, editor, loader, menus};

use super::super::super::frame::EditorFrame;

pub(super) fn resolve(
    state: &mut editor::EditorState,
    ctx: &egui::Context,
    f: &mut EditorFrame<'_>,
    strip_action: Option<deck_strip::StripAction>,
    canvas_action: Option<editor::CanvasAction>,
    pending_menu_action: &mut Option<menus::MenuAction>,
) {
    let mut deck_target = state.deck_nav.take().and_then(|nav| match nav {
        editor::DeckNav::Next => f.deck.next_path(),
        editor::DeckNav::Prev => f.deck.prev_path(),
        editor::DeckNav::First => f.deck.first_path(),
        editor::DeckNav::Last => f.deck.last_path(),
    });
    match strip_action {
        Some(deck_strip::StripAction::Open(path)) => {
            deck_target = deck_target.or(Some(path));
        }
        Some(deck_strip::StripAction::ToggleAxis) => {
            f.deck.axis = f.deck.axis.toggled();
            f.deck.layout_dirty = true;
            f.settings.deck_axis = f.deck.axis;
            f.settings.save_in_background();
        }
        Some(deck_strip::StripAction::CycleSide) => {
            f.deck.strip_side = f.deck.strip_side.cycled();
            f.settings.deck_strip_side = f.deck.strip_side;
            f.settings.save_in_background();
        }
        Some(deck_strip::StripAction::AddCanvas) => {
            if let Some(idx) = f.deck.push_placeholder(
                f.settings.last_page_size,
                f.settings.new_canvas_format.extension(),
            ) {
                f.deck.jump_to = Some(idx);
                f.deck.jump_reframe = true;
            }
        }
        None => {}
    }
    match canvas_action {
        Some(editor::CanvasAction::Rename(id, new_stem)) => {
            let is_active = f.deck.slots.get(f.deck.active).map(|s| s.id) == Some(id);
            let is_placeholder = f
                .deck
                .find_by_id(id)
                .and_then(|index| f.deck.slots.get(index))
                .is_some_and(|slot| slot.is_placeholder);
            if is_placeholder {
                f.deck.discard_placeholder(id, state);
            } else if is_active {
                state.file_rename_requested = Some(new_stem);
            } else if let Some(path) = f
                .deck
                .find_by_id(id)
                .and_then(|i| f.deck.slots.get(i))
                .map(|s| s.path.clone())
            {
                *f.ignore_fs_events_until =
                    Some(Instant::now() + std::time::Duration::from_secs(2));
                *f.watcher = None;
                loader::spawn_document_rename(path, new_stem, f.tx.clone(), ctx.clone());
            }
        }
        Some(editor::CanvasAction::Duplicate(id)) => {
            let source = f
                .deck
                .find_by_id(id)
                .and_then(|i| f.deck.slots.get(i))
                .map(|slot| (slot.is_placeholder, slot.page, slot.path.clone()));
            if let Some((true, page, path)) = source {
                let ext = path
                    .extension()
                    .and_then(|value| value.to_str())
                    .unwrap_or(f.settings.new_canvas_format.extension());
                f.deck
                    .push_placeholder(page.unwrap_or(f.settings.last_page_size), ext);
            } else if let Some((false, _, path)) = source {
                loader::spawn_gallery_op(
                    loader::GalleryOp::Duplicate { path },
                    false,
                    f.tx.clone(),
                    ctx.clone(),
                );
            }
        }
        Some(editor::CanvasAction::Delete(id)) => {
            let is_active = f.deck.slots.get(f.deck.active).map(|s| s.id) == Some(id);
            let is_placeholder = f
                .deck
                .find_by_id(id)
                .and_then(|index| f.deck.slots.get(index))
                .is_some_and(|slot| slot.is_placeholder);
            if is_placeholder {
                f.deck.discard_placeholder(id, state);
            } else if is_active {
                state.delete_requested = true;
            } else if let Some(path) = f
                .deck
                .find_by_id(id)
                .and_then(|i| f.deck.slots.get(i))
                .map(|s| s.path.clone())
            {
                *f.ignore_fs_events_until =
                    Some(Instant::now() + std::time::Duration::from_secs(2));
                *f.watcher = None;
                let sidecar = canvas_io::find_sidecar(&path);
                f.deck_ops.undoable_deletes.insert(path.clone(), sidecar);
                loader::spawn_document_delete(path, f.tx.clone(), ctx.clone());
            }
        }
        Some(editor::CanvasAction::ReplaceFromLocal(layer)) => {
            loader::spawn_pick_replacement_image(layer, None, f.tx.clone(), ctx.clone());
        }
        Some(editor::CanvasAction::ReplaceFromUrl(layer, url)) => {
            loader::spawn_load_replacement_image_from_url(layer, url, f.tx.clone(), ctx.clone());
        }
        Some(editor::CanvasAction::Menu(action)) => {
            *pending_menu_action = Some(action);
        }
        None => {}
    }
    // Si ya hay un salto pendiente (`jump_to` — zona «+» de la baraja,
    // botón «+» de la tira, o uno anterior esperando a que el editor se
    // quedara ocioso), NINGUNA petición del resto del frame puede pisarlo:
    // el «+» es la interacción más reciente y el trazo que el usuario dibuje
    // a continuación debe caer en el lienzo recién creado. Antes este bloque
    // podía sobreescribir `jump_to` (Save all, PageUp/PageDown, deshacer
    // global) y el lienzo nuevo quedaba sin activar — el dibujo se iba al
    // lienzo equivocado y el nuevo se quedaba en blanco.
    if f.deck.jump_to.is_none() {
        if let Some(target) = deck_target {
            if let Some(idx) = f.deck.find_by_path(&target) {
                f.deck.jump_to = Some(idx);
                f.deck.jump_reframe = true;
            }
        } else if let Some(&next_id) = f.save.save_all_queue.first() {
            if f.deck.slots.get(f.deck.active).map(|s| s.id) != Some(next_id) {
                match f.deck.find_by_id(next_id) {
                    Some(idx) => {
                        f.deck.jump_to = Some(idx);
                        f.deck.jump_reframe = true;
                    }
                    None => {
                        f.save.save_all_queue.remove(0);
                    }
                }
            }
        } else if let Some(id) = state
            .pending_global_undo
            .as_ref()
            .or(state.pending_global_redo.as_ref())
            .map(editor::GlobalStep::slot_id)
        {
            match f.deck.find_by_id(id) {
                Some(idx) => {
                    f.deck.jump_to = Some(idx);
                    f.deck.jump_reframe = true;
                }
                None => {
                    state.discard_pending_global_undo();
                    state.discard_pending_global_redo();
                }
            }
        }
    }
    if let Some(folder) = f.deck.folder.clone() {
        let gen = f.deck.generation();
        for path in f.deck.request_loads(&[]) {
            loader::spawn_load_slot(
                folder.clone(),
                path,
                gen,
                f.settings.sidecar_default,
                f.tx.clone(),
                ctx.clone(),
            );
        }
    }
    let save_modal_pending = f.save.overwrite_prompt.is_some()
        || f.save.readonly_prompt.is_some()
        || f.deck_ops.materializing.is_some();
    if !save_modal_pending
        && deck::apply_jump(f.deck, state)
        && std::mem::take(&mut f.deck.jump_reframe)
    {
        state.viewport.request_fit();
    }
    if let Some(&next_id) = f.save.save_all_queue.first() {
        if f.deck.slots.get(f.deck.active).map(|s| s.id) == Some(next_id) {
            let waiting_on_modal =
                f.save.overwrite_prompt.is_some() || f.save.readonly_prompt.is_some();
            if !state.is_dirty() {
                // Ya se guardó (`AppMsg::Saved` la sacó de la cola).
            } else if state.saving || waiting_on_modal {
                // En curso, o esperando la respuesta del usuario.
            } else if f.save.save_all_attempted {
                tracing::warn!(
                    "Save all: se detiene en un lienzo de fondo (guardado fallido o cancelado)"
                );
                f.save.save_all_queue.clear();
                f.save.save_all_attempted = false;
            } else {
                f.save.save_all_attempted = true;
                state.save_clicked = true;
            }
        }
    }
    if state
        .pending_global_undo
        .as_ref()
        .is_some_and(|step| f.deck.slots.get(f.deck.active).map(|s| s.id) == Some(step.slot_id()))
    {
        state.finish_pending_global_undo();
    }
    if state
        .pending_global_redo
        .as_ref()
        .is_some_and(|step| f.deck.slots.get(f.deck.active).map(|s| s.id) == Some(step.slot_id()))
    {
        state.finish_pending_global_redo();
    }
    if let Some(record) = state.pending_restore.take() {
        loader::spawn_restore_from_trash(record.path, record.sidecar, f.tx.clone(), ctx.clone());
    }
}
