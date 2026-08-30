//! Guardar y «guardar como»: los botones del panel y los atajos de teclado,
//! el aviso de sobrescritura destructiva y la eleccion de rama (imagen o
//! diseno) segun la extension final de la ruta elegida.
//!
//! Movido tal cual desde `editor_view_ui`, en el mismo orden: `Ctrl+Shift+S`
//! se resuelve ANTES que `Ctrl+S`.

use eframe::egui;
use eframe::egui_wgpu::RenderState;

use crate::{editor, loader};

use super::super::super::frame::EditorFrame;
use super::super::super::persistence::{start_save, start_save_design, SaveContext};
use super::super::super::Nav;

pub(super) fn handle_save(
    state: &mut editor::EditorState,
    ctx: &egui::Context,
    rs: &RenderState,
    f: &mut EditorFrame<'_>,
    open_next: &mut Option<Nav>,
) {
    // Guardar / Guardar como: botones del panel o atajos de teclado (el
    // orden importa: Ctrl+Shift+S primero).
    let save_as = std::mem::take(&mut state.save_as_clicked)
        || ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                egui::Key::S,
            ))
        });
    let mut save = f.save.save_requested
        || std::mem::take(&mut state.save_clicked)
        || ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::COMMAND,
                egui::Key::S,
            ))
        });
    f.save.save_requested = false;

    if save_as {
        if state.is_design {
            loader::spawn_pick_design_path(Some(state.file_name()), f.tx.clone(), ctx.clone());
        } else {
            loader::spawn_pick_save_path(Some(state.file_name()), f.tx.clone(), ctx.clone());
        }
        save = false;
    }
    if save {
        if !state.is_dirty() {
            // Un guardado sin cambios no reescribe nada: en JPEG,
            // recomprimir sin motivo costaría calidad. Si veníamos de un
            // diálogo de cerrar/volver, su flujo continúa.
            tracing::info!("documento sin cambios: no se reescribe el archivo");
            if f.save.close_after_save {
                f.save.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else if let Some(nav) = f.save.after_save.take() {
                *open_next = Some(nav);
            }
        } else if state.is_design {
            // Un diseño no se rasteriza: no hay nada destructivo que
            // avisar, ni SVG/GIF que redirigir, así que se saltan ambos
            // modales.
            match state.doc.source_path.clone() {
                Some(path) => {
                    let mut sctx = SaveContext {
                        renderer: f.renderer,
                        rs,
                        tx: f.tx,
                        ctx,
                        ignore_fs_events_until: f.ignore_fs_events_until,
                        scope: f.deck.slots.get(f.deck.active).map_or(0, |s| s.scope),
                    };
                    start_save_design(state, &mut sctx, path, false);
                }
                None => loader::spawn_pick_design_path(
                    Some(state.file_name()),
                    f.tx.clone(),
                    ctx.clone(),
                ),
            }
        } else {
            match state.doc.source_path.clone() {
                // SVG/GIF: no se sobrescriben nunca; se explica y se
                // redirige a «Save as…».
                Some(path) if !canvas_io::can_overwrite(&path) => {
                    f.save.readonly_prompt = Some(path);
                }
                // El documento ya no tiene NINGUNA capa de imagen (el usuario
                // borró la última foto): sobrescribir el raster aplanado
                // descartaría la copia editable y destruiría la foto original
                // sin dejar rastro. Se pide confirmación ANTES de hornear
                // (se salta también el modal de sobrescritura: este aviso es
                // más específico). Un lienzo `born_blank` —creado por la app,
                // sin foto que pudo borrarse— no recibe el aviso.
                Some(path)
                    if !state.born_blank
                        && !f.save.discard_raster_confirmed
                        && !crate::app::persistence::has_raster_layers(&state.doc) =>
                {
                    f.save.discard_raster_prompt = Some(path);
                }
                Some(path) => {
                    // Aviso de sobrescritura destructiva: la primera vez de
                    // cada sesión (salvo que el usuario pidiera no volver a
                    // preguntar), y NUNCA para un lienzo `born_blank` — lo
                    // creó la propia app en blanco, no hay píxeles del
                    // usuario que este primer guardado pudiera destruir.
                    if !state.born_blank
                        && !f.settings.skip_overwrite_warning
                        && !f.save.overwrite_confirmed
                    {
                        f.save.overwrite_dont_ask = false;
                        f.save.overwrite_prompt = Some(path);
                    } else {
                        let mut sctx = SaveContext {
                            renderer: f.renderer,
                            rs,
                            tx: f.tx,
                            ctx,
                            ignore_fs_events_until: f.ignore_fs_events_until,
                            scope: f.deck.slots.get(f.deck.active).map_or(0, |s| s.scope),
                        };
                        start_save(
                            state,
                            &mut sctx,
                            f.deck,
                            path,
                            false,
                            f.settings.jpeg_quality,
                        );
                    }
                }
                // Sin origen en disco: cae a «Guardar como…».
                None => {
                    loader::spawn_pick_save_path(Some(state.file_name()), f.tx.clone(), ctx.clone())
                }
            }
        }
    }
    if let Some(path) = f.save.pending_save_as.take() {
        // La extensión final de la ruta elegida decide la rama, venga del
        // diálogo de diseño o del de imagen (ambos acaban en el mismo
        // `SaveAsPicked`).
        if canvas_io::is_canvas_file(&path) {
            let mut sctx = SaveContext {
                renderer: f.renderer,
                rs,
                tx: f.tx,
                ctx,
                ignore_fs_events_until: f.ignore_fs_events_until,
                scope: f.deck.slots.get(f.deck.active).map_or(0, |s| s.scope),
            };
            start_save_design(state, &mut sctx, path, true);
        } else {
            let mut sctx = SaveContext {
                renderer: f.renderer,
                rs,
                tx: f.tx,
                ctx,
                ignore_fs_events_until: f.ignore_fs_events_until,
                scope: f.deck.slots.get(f.deck.active).map_or(0, |s| s.scope),
            };
            start_save(
                state,
                &mut sctx,
                f.deck,
                path,
                true,
                f.settings.jpeg_quality,
            );
        }
    }
}
