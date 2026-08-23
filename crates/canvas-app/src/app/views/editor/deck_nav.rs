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
    // Saltar a otro lienzo de la baraja: clic en el propio lienzo (ya deja
    // `self.deck.jump_to` listo, dentro de `canvas_ui`), tira lateral, o
    // teclado (PageUp/PageDown/Home/End). El intercambio es SIN PÉRDIDA —
    // el lienzo saliente queda guardado en su propia ranura con su
    // historial de deshacer intacto — así que, a diferencia de «Back to
    // gallery» (que sí sale del editor), no hace falta preguntar por
    // cambios sin guardar para saltar aquí dentro.
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
        // Inline, no `self.toggle_deck_axis()`: el borrow checker no ve que
        // ese método solo toca campos disjuntos de `state`.
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
            // Sin `layout_dirty = true`: mover el panel no cambia la
            // geometría de la baraja, solo el rect del panel central — que
            // `Viewport::note_size` ya detecta y reajusta.
        }
        Some(deck_strip::StripAction::AddCanvas) => {
            if let Some(idx) = f.deck.push_placeholder(
                f.settings.last_page_size,
                f.settings.new_canvas_format.extension(),
            ) {
                f.deck.jump_to = Some(idx);
                f.deck.jump_center = true;
            }
        }
        None => {}
    }
    // Renombrar/duplicar/borrar desde la cabecera de un lienzo (activo o de
    // fondo) en el área central — mismas operaciones que ya existían para
    // la ranura activa (lápiz junto al nombre, botón «Delete» del panel) o
    // desde la galería (duplicar), generalizadas por id/ruta en vez de
    // asumir "la activa".
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
                // Reutiliza el camino ya existente (lápiz junto al
                // nombre): se recoge y se lanza más arriba, en el próximo
                // frame.
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
                // Reutiliza el camino ya existente (botón «Delete» del
                // panel de propiedades).
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
                f.undoable_deletes.insert(path.clone(), sidecar);
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
    if let Some(target) = deck_target {
        if let Some(idx) = f.deck.find_by_path(&target) {
            f.deck.jump_to = Some(idx);
            // Por la tira o el teclado: el destino puede no estar a la
            // vista, así que sí hace falta recentrar (a diferencia de un
            // clic directo sobre el propio lienzo, que ya deja `jump_to`
            // sin esto).
            f.deck.jump_center = true;
        }
    } else if let Some(&next_id) = f.save_all_queue.first() {
        // «Save all»: sin una navegación más prioritaria este frame, salta
        // a la próxima ranura pendiente de la cola.
        if f.deck.slots.get(f.deck.active).map(|s| s.id) != Some(next_id) {
            match f.deck.find_by_id(next_id) {
                Some(idx) => {
                    f.deck.jump_to = Some(idx);
                    f.deck.jump_center = true;
                }
                // Desapareció (renombrada/borrada) mientras esperaba turno:
                // se salta sin más.
                None => {
                    f.save_all_queue.remove(0);
                }
            }
        }
    } else if let Some(id) = state
        .pending_global_undo
        .as_ref()
        .or(state.pending_global_redo.as_ref())
        .map(editor::GlobalStep::slot_id)
    {
        // Deshacer/rehacer global: el paso más reciente de toda la sesión
        // le tocaba a OTRO diseño de la baraja — salta a él para mostrarlo
        // (mismo patrón que «Save all», arriba). `request_loads`, más abajo
        // en `canvas_ui`, dispara la recarga de disco si esa ranura ya no
        // está `Ready` (fue descartada por presupuesto).
        match f.deck.find_by_id(id) {
            Some(idx) => {
                f.deck.jump_to = Some(idx);
                f.deck.jump_center = true;
            }
            // El diseño desapareció (archivo borrado) mientras esperaba
            // turno: se descarta ese paso, sin encadenar automáticamente
            // con el siguiente.
            None => {
                state.discard_pending_global_undo();
                state.discard_pending_global_redo();
            }
        }
    }
    // Aplica el salto si el destino ya está listo y el editor está ocioso;
    // si no, la petición queda pendiente y se reintenta en los próximos
    // frames — llamar aquí siempre, no solo cuando `deck_target` trae algo
    // nuevo, es lo que reintenta un salto que aún esperaba a que su carga
    // terminase. Recentra la vista SOLO si quien pidió el salto lo marcó
    // (`jump_center`): un clic directo sobre el propio lienzo ya se ve,
    // recentrar ahí sería mover la cámara sin que el usuario lo pidiera.
    //
    // NUNCA mientras haya un modal de guardado pendiente
    // (`f.overwrite_prompt`/`f.readonly_prompt`): `is_idle()` ya cubre
    // `saving`, pero esos modales aparecen ANTES de que `start_save` los
    // ponga a `true` — sin este freno, saltar en ese hueco dejaría el modal
    // hablando de un archivo mientras `state` pasa a ser otro documento, y
    // al confirmarlo se guardarían los píxeles del documento EQUIVOCADO en
    // la ruta del modal. Igual con `f.materializing`: la reserva de nombre de
    // una provisional tampoco pone `saving` a `true` todavía, y saltar a
    // mitad de esa reserva dejaría la respuesta actuando sobre el lienzo
    // equivocado.
    let save_modal_pending =
        f.overwrite_prompt.is_some() || f.readonly_prompt.is_some() || f.materializing.is_some();
    if !save_modal_pending
        && deck::apply_jump(f.deck, state)
        && std::mem::take(&mut f.deck.jump_center)
    {
        state.viewport.request_center(f.deck.active_rect());
    }
    // «Save all»: si la activa ya es la ranura que tocaba, dispara su
    // guardado — mismo camino que Ctrl+S, un frame más tarde (el bloque de
    // guardado de este frame ya corrió antes de que se dibujaran los
    // paneles).
    if let Some(&next_id) = f.save_all_queue.first() {
        if f.deck.slots.get(f.deck.active).map(|s| s.id) == Some(next_id) {
            // El aviso de sobrescritura (primer lienzo raster del lote) o
            // el redirect de SVG/GIF cuentan como "en curso", no como
            // fallo: sin este freno, el intento ya marcado se leería como
            // fallido mientras el usuario todavía no ha respondido al
            // modal.
            let waiting_on_modal = f.overwrite_prompt.is_some() || f.readonly_prompt.is_some();
            if !state.is_dirty() {
                // Ya se guardó (`AppMsg::Saved` la sacó de la cola) o nunca
                // hizo falta: nada que hacer aquí.
            } else if state.saving || waiting_on_modal {
                // En curso, o esperando la respuesta del usuario.
            } else if *f.save_all_attempted {
                // Se pulsó "Guardar", no hay guardado en curso ni modal
                // pendiente, y sigue sucia: ese intento falló de verdad (o
                // el usuario canceló el modal). Se aborta el lote en vez de
                // reintentar sin fin sobre el mismo lienzo.
                tracing::warn!(
                    "Save all: se detiene en un lienzo de fondo (guardado fallido o cancelado)"
                );
                f.save_all_queue.clear();
                *f.save_all_attempted = false;
            } else {
                *f.save_all_attempted = true;
                state.save_clicked = true;
            }
        }
    }
    // Deshacer/rehacer global: si la activa ya es la ranura que le tocaba a
    // la petición pendiente (el salto de arriba se aplicó, esta misma
    // vuelta o en una anterior), ejecuta el paso local ahora que es la
    // activa y limpia la petición.
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
    // Deshacer un borrado (`GlobalStep::Delete`): no pertenece a ninguna
    // ranura, así que `undo()` ya lo resolvió sin esperar ningún salto —
    // solo queda lanzar la restauración.
    if let Some(record) = state.pending_restore.take() {
        loader::spawn_restore_from_trash(record.path, record.sidecar, f.tx.clone(), ctx.clone());
    }
}
