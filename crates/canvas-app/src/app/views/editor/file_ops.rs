//! Operaciones sobre el ARCHIVO del lienzo activo, pedidas desde el panel:
//! renombrar, borrar, y materializar una ranura provisional (un lienzo nuevo
//! que todavia no existe en disco) en cuanto el usuario la edita.

use std::time::Instant;

use eframe::egui;

use crate::{editor, loader};

use super::super::super::frame::EditorFrame;

pub(super) fn handle_file_ops(
    state: &mut editor::EditorState,
    ctx: &egui::Context,
    f: &mut EditorFrame<'_>,
) {
    // Renombrar/borrar el archivo abierto (lápiz junto al nombre / botón
    // «Delete» del panel).
    if let Some(new_stem) = state.file_rename_requested.take() {
        if let Some(path) = state.doc.source_path.clone() {
            // Misma ventana de gracia que un guardado (`Saved` de éxito, más
            // abajo): se abre ANTES de lanzar la operación, no solo al
            // recibir la respuesta. Renombrar hace que el f.watcher (que
            // sigue mirando la ruta vieja) vea el archivo "desaparecer" —
            // un evento mucho más inmediato que el de un guardado en el
            // sitio — y sin esto el banner de «cambió por fuera» podía
            // saltar en la carrera entre ese evento y el mensaje
            // `DocumentRenamed` que actualiza `source_path`.
            *f.ignore_fs_events_until = Some(Instant::now() + std::time::Duration::from_secs(2));
            *f.watcher = None;
            loader::spawn_document_rename(path, new_stem, f.tx.clone(), ctx.clone());
        }
    }
    if std::mem::take(&mut state.delete_requested) {
        // Si viene de deshacer un `Create` (ver `pending_delete_from_undo`),
        // este borrado en concreto no debe poder deshacerse a su vez — se
        // consume aquí, ANTES de decidir si la ruta entra en
        // `f.deck_ops.undoable_deletes`.
        let from_undo = std::mem::take(&mut state.pending_delete_from_undo);
        let placeholder_id = f
            .deck
            .slots
            .get(f.deck.active)
            .filter(|slot| slot.is_placeholder)
            .map(|slot| slot.id);
        if let Some(id) = placeholder_id {
            f.deck.discard_placeholder(id, state);
        } else if let Some(path) = state.doc.source_path.clone() {
            *f.ignore_fs_events_until = Some(Instant::now() + std::time::Duration::from_secs(2));
            *f.watcher = None;
            if !from_undo {
                let sidecar = canvas_io::find_sidecar(&path);
                f.deck_ops.undoable_deletes.insert(path.clone(), sidecar);
            }
            loader::spawn_document_delete(path, f.tx.clone(), ctx.clone());
        }
    }
    // Ranura PROVISIONAL que se convierte en archivo de verdad en cuanto el
    // usuario la edita — sin diálogo. El usuario pidió «un lienzo nuevo», no
    // «guardar como»; preguntarle un nombre justo después de su primer
    // trazo rompería el flujo. Va DESPUÉS de `handle_messages` (una
    // respuesta de este mismo frame ya está aplicada) y ANTES del bloque de
    // guardado (el `save_clicked` que la respuesta deja preparado se
    // consume ese mismo frame, más abajo).
    let placeholder = f
        .deck
        .slots
        .get(f.deck.active)
        .filter(|s| s.is_placeholder)
        .map(|s| {
            (
                s.id,
                s.path
                    .extension()
                    .map(|e| e.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            )
        });
    if let Some((id, ext)) = placeholder {
        let has_canvas_content = state.doc.page().is_ok_and(|page| !page.layers.is_empty());
        if has_canvas_content
            && state.is_dirty()
            && !state.saving
            && f.deck_ops.materializing.is_none()
            && f.deck_ops.materialize_blocked != Some(id)
        {
            if let Some(folder) = f.deck.folder.clone() {
                f.deck_ops.materializing = Some(id);
                loader::spawn_reserve_canvas_path(folder, id, ext, f.tx.clone(), ctx.clone());
            }
        }
    }
}
