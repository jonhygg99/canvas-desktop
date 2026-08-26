//! Vista del editor. `editor_view_ui` es una funcion libre, no un metodo de
//! la app: se llama mientras `state` sigue prestado de `ws.view`, asi que
//! recibe el resto del estado de la ventana en un `EditorFrame`.
//!
//! Este archivo es SOLO orquestacion: llama a los submodulos EN EL MISMO orden
//! en que estaban sus bloques dentro de la funcion original. Ese orden es
//! significativo (los comentarios del codigo movido lo dicen explicitamente) y
//! no debe cambiarse al tocar este archivo.

use eframe::egui;
use eframe::egui_wgpu::RenderState;

use crate::{deck, editor, loader, menus};

use super::super::frame::EditorFrame;
use super::super::Nav;

mod deck_nav;
mod file_ops;
mod modals;
mod panels;
mod save_flow;

/// Vista de editor: baraja + panel de capas/propiedades + lienzo, y toda la
/// orquestación de guardado, exportación, navegación de la baraja y
/// deshacer/rehacer global de ese frame. `rs` ya viene resuelto por el
/// llamador (si `frame.wgpu_render_state()` fuera `None`, el llamador corta
/// el frame entero antes de entrar aquí, no solo esta vista).
pub(in crate::app) fn editor_view_ui(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    rs: &RenderState,
    state: &mut editor::EditorState,
    paste_requested: bool,
    f: &mut EditorFrame<'_>,
) -> (Option<Nav>, Option<menus::MenuAction>) {
    let mut open_next: Option<Nav> = None;
    // Acción del menú contextual del lienzo (clic derecho): se resuelve por
    // el llamador, una vez liberado el préstamo de `state`.
    let mut pending_menu_action: Option<menus::MenuAction> = None;

    // El deshacer/rehacer global (`push_undo_step`/`undo`/`redo` en
    // `editor.rs`) etiqueta cada paso con esta id: hay que tenerla al día
    // ANTES de `handle_shortcuts` (que puede disparar un Ctrl+Z ese mismo
    // frame) y de cualquier edición que ocurra más abajo en `canvas_ui`.
    // Barato de refrescar cada frame; más simple que perseguir cada sitio
    // donde `f.deck.active`/`ws.view` pueden cambiar.
    state.active_slot_id = f.deck.slots.get(f.deck.active).map_or(0, |s| s.id);
    // Modo estrés de desarrollo (`CANVAS_DEBUG_EDIT_LOOP=1`): simula
    // ediciones continuas en el documento activo (oscila el desenfoque de
    // las capas) y navegación entre lienzos, para reproducir el escenario
    // de dos ventanas editando en paralelo sin clics. Puramente visual:
    // muta `state.doc` sin pasos de deshacer, así que no marca nada sucio.
    simulate_edits(state, f, ctx);
    state.handle_shortcuts(ctx, paste_requested, f.deck.rename_edit.is_some());

    // Recarga pedida desde el banner de «cambió en disco».
    if std::mem::take(&mut state.reload_requested) {
        match state.doc.source_path.clone() {
            Some(path) => open_next = Some(Nav::Open(path)),
            None => state.external_change = false,
        }
    }

    // Volver a la galería (preguntando si hay cambios sin guardar).
    if state.return_requested {
        state.return_requested = false;
        if let Some(folder) = state.from_gallery.clone() {
            if !state.is_dirty() {
                open_next = Some(Nav::Open(folder));
            } else {
                let choice = rfd::MessageDialog::new()
                    .set_level(rfd::MessageLevel::Warning)
                    .set_title("Unsaved changes")
                    .set_description(format!(
                        "\"{}\" has unsaved changes.\nSave them before going back to the gallery? (\"No\" discards them.)",
                        state.file_name()
                    ))
                    .set_buttons(rfd::MessageButtons::YesNoCancelCustom(
                        "Save".to_owned(),
                        "Discard".to_owned(),
                        "Cancel".to_owned(),
                    ))
                    .show();
                // Igual que en confirm_window_close: en Windows el resultado llega
                // como Yes/No/Cancel, no Custom.
                match choice {
                    rfd::MessageDialogResult::Yes => {
                        f.save.save_requested = true;
                        f.save.after_save = Some(Nav::Open(folder));
                    }
                    rfd::MessageDialogResult::Custom(c) if c == "Save" => {
                        f.save.save_requested = true;
                        f.save.after_save = Some(Nav::Open(folder));
                    }
                    rfd::MessageDialogResult::No => {
                        open_next = Some(Nav::Open(folder));
                    }
                    rfd::MessageDialogResult::Custom(c) if c == "Discard" => {
                        open_next = Some(Nav::Open(folder));
                    }
                    _ => {}
                }
            }
        }
    }

    file_ops::handle_file_ops(state, ctx, f);
    save_flow::handle_save(state, ctx, rs, f, &mut open_next);
    modals::show_modals(state, ctx, rs, f);
    let (strip_action, canvas_action) = panels::show_panels(state, ui, rs, f);
    deck_nav::resolve(
        state,
        ctx,
        f,
        strip_action,
        canvas_action,
        &mut pending_menu_action,
    );

    if std::mem::take(&mut state.settings_clicked) {
        *f.show_settings = true;
    }
    if std::mem::take(&mut state.layers_panel_toggle) {
        f.settings.layers_collapsed = !f.settings.layers_collapsed;
        f.settings.save_in_background();
    }
    // El checkbox del sidecar en el editor ES el valor por defecto
    // persistido: cambiarlo ahí lo recuerda para el futuro. En un diseño el
    // checkbox ni se muestra: no debe tocar el ajuste.
    if !state.is_design && state.sidecar_enabled != f.settings.sidecar_default {
        f.settings.sidecar_default = state.sidecar_enabled;
        f.settings.save_in_background();
    }

    (open_next, pending_menu_action)
}
/// Modo estrés de desarrollo para reproducir el crash de «dos ventanas
/// editando a la vez» sin interacción, con el escenario REAL de varias
/// imágenes pesadas en una carpeta:
///
/// 1. Al entrar, siembra la baraja con TODOS los archivos de imagen/diseño
///    del padre del archivo abierto (como haría la galería al abrir desde
///    ella), así que cada ventana carga las N imágenes pesadas.
/// 2. Cada ~15 frames (≈4/s) oscila el desenfoque de todas las capas de
///    imagen del documento activo — un re-horneado real en
///    `sync_layer_effects`, el mismo camino que arrastrar el slider de blur.
/// 3. Cada ~120 frames (~2 s) avanza a la siguiente ranura de la baraja,
///    desfasado por ventana: ambas editan lienzos distintos en paralelo,
///    navegando entre las imágenes (cargas/descargas, evicción por
///    presupuesto, `forget_scope`).
///
/// Solo actúa con `CANVAS_DEBUG_EDIT_LOOP=1`; nunca en la app empaquetada.
fn simulate_edits(state: &mut editor::EditorState, f: &mut EditorFrame<'_>, ctx: &egui::Context) {
    use std::sync::atomic::{AtomicU64, Ordering};

    if std::env::var_os("CANVAS_DEBUG_EDIT_LOOP").is_none() {
        return;
    }
    seed_stress_deck(state, f, ctx);

    static FRAME: AtomicU64 = AtomicU64::new(0);
    let frame = FRAME.fetch_add(1, Ordering::Relaxed);
    // Fase por ventana: `generation()` es única por baraja (contador global),
    // así que las dos ventanas editan y navegan desfasadas, en paralelo.
    let phase = f.deck.generation() % 97;

    // Navegación: avanza una ranura cada ~120 frames si hay más de una.
    if frame % 120 == 0 && f.deck.slots.len() > 1 {
        state.deck_nav = Some(editor::DeckNav::Next);
    }

    if frame % 15 != 0 {
        return;
    }
    // Onda triangular: 0 → 24 → 0 en 12 pasos (cada paso 15 frames).
    let step = ((frame / 15) + phase) % 12;
    let wobble = (step as f32 - 6.0).abs() * 4.0;
    // Se copian los ids primero: `page` presta `state.doc` inmutablemente y
    // `layer_mut` pide un préstamo mutable — no pueden coexistir.
    let layer_ids: Vec<_> = match state.doc.page() {
        Ok(page) => page.layers.iter().rev().take(8).map(|l| l.id).collect(),
        Err(_) => return,
    };
    let mut edited = 0u32;
    for (i, id) in layer_ids.into_iter().enumerate() {
        if let Ok(l) = state.doc.layer_mut(id) {
            l.effects.blur_radius = 20.0 + i as f32 * 30.0 + wobble;
            edited += 1;
        }
    }
    tracing::info!(
        "stress: edición simulada (ventana={phase}, paso={step}, capas={edited}, ranuras={})",
        f.deck.slots.len()
    );
}

/// Siembra la baraja del editor con los hermanos del archivo abierto — la
/// misma baraja que construiría la galería al abrir desde ella — para que
/// el modo estrés cargue VARIAS imágenes pesadas de la carpeta en cada
/// ventana. No-op si la baraja ya tiene carpeta (ya sembrada o navegada).
fn seed_stress_deck(state: &editor::EditorState, f: &mut EditorFrame<'_>, ctx: &egui::Context) {
    if f.deck.folder.is_some() {
        return;
    }
    let Some(src) = state.doc.source_path.clone() else {
        return;
    };
    let Some(folder) = src.parent().map(|p| p.to_path_buf()) else {
        return;
    };
    if !folder.is_dir() {
        return;
    }
    let mut entries: Vec<_> = std::fs::read_dir(&folder)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| !n.starts_with('.'))
        })
        .filter(|p| canvas_io::is_canvas_file(p) || canvas_io::is_image_file(p))
        .collect();
    entries.sort();
    if entries.is_empty() {
        return;
    }
    let items = entries
        .into_iter()
        .map(|path| {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let kind = if canvas_io::is_canvas_file(&path) {
                crate::gallery::ItemKind::Design
            } else {
                crate::gallery::ItemKind::Image
            };
            let mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
            deck::SeedItem {
                path,
                name,
                kind,
                mtime,
                thumb: None,
                thumb_failed: false,
            }
        })
        .collect();
    let seed = deck::DeckSeed {
        folder: folder.clone(),
        sort: f.settings.gallery_sort,
        items,
    };
    *f.deck = deck::Deck::from_seed(seed, &src);
    f.deck.axis = f.settings.deck_axis;
    f.deck.strip_visible = f.settings.deck_strip_visible;
    f.deck.strip_side = f.settings.deck_strip_side;
    f.deck.layout_dirty = true;
    // Precarga los hermanos en segundo plano (como `start_seeded_deck_preload`).
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
    tracing::info!(
        "stress: baraja sembrada con {} ranuras de {}",
        f.deck.slots.len(),
        folder.display()
    );
}
