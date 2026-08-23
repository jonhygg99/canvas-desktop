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
use super::super::super::persistence::{start_save, start_save_design};
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
    let mut save = *f.save_requested
        || std::mem::take(&mut state.save_clicked)
        || ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::COMMAND,
                egui::Key::S,
            ))
        });
    *f.save_requested = false;

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
            if *f.close_after_save {
                *f.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else if let Some(nav) = f.after_save.take() {
                *open_next = Some(nav);
            }
        } else if state.is_design {
            // Un diseño no se rasteriza: no hay nada destructivo que
            // avisar, ni SVG/GIF que redirigir, así que se saltan ambos
            // modales.
            match state.doc.source_path.clone() {
                Some(path) => start_save_design(
                    state,
                    f.renderer,
                    rs,
                    f.tx,
                    ctx,
                    path,
                    false,
                    f.ignore_fs_events_until,
                ),
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
                    *f.readonly_prompt = Some(path);
                }
                Some(path) => {
                    // Aviso de sobrescritura destructiva: la primera vez de
                    // cada sesión (salvo que el usuario pidiera no volver a
                    // preguntar), y NUNCA para un lienzo `born_blank` — lo
                    // creó la propia app en blanco, no hay píxeles del
                    // usuario que este primer guardado pudiera destruir.
                    if !state.born_blank
                        && !f.settings.skip_overwrite_warning
                        && !*f.overwrite_confirmed
                    {
                        *f.overwrite_dont_ask = false;
                        *f.overwrite_prompt = Some(path);
                    } else {
                        start_save(
                            state,
                            f.renderer,
                            rs,
                            f.tx,
                            ctx,
                            path,
                            false,
                            f.settings.jpeg_quality,
                            f.ignore_fs_events_until,
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
    if let Some(path) = f.pending_save_as.take() {
        // La extensión final de la ruta elegida decide la rama, venga del
        // diálogo de diseño o del de imagen (ambos acaban en el mismo
        // `SaveAsPicked`).
        if canvas_io::is_canvas_file(&path) {
            start_save_design(
                state,
                f.renderer,
                rs,
                f.tx,
                ctx,
                path,
                true,
                f.ignore_fs_events_until,
            );
        } else {
            start_save(
                state,
                f.renderer,
                rs,
                f.tx,
                ctx,
                path,
                true,
                f.settings.jpeg_quality,
                f.ignore_fs_events_until,
            );
        }
    }
}
