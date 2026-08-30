//! El panel de propiedades: archivo, página, insertar, y todos los
//! controles de la capa seleccionada (transform, blur, color, sombra,
//! contenido de texto/forma) — la UI que vive en el panel lateral derecho,
//! sin nada del lienzo en sí.
//!
//! Dividido por responsabilidad: `page` (resolución de página + ventanita
//! "Size"), `effects` (blur/color/sombra), `layer_common` (posición/tamaño/
//! recorte/alineación, comunes a cualquier capa) y `content`/`content_text`/
//! `content_shape` (controles propios de texto o forma).

mod content;
mod content_shape;
mod content_text;
mod effects;

mod layer_common;
#[cfg(test)]
#[path = "opacity_tests.rs"]
mod opacity_tests;
pub(crate) mod page;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
use eframe::egui;

use crate::app_icons::{
    draw_gear_icon, draw_pencil_icon, draw_triangle_icon, icon_button_ui, icon_text_button_ui,
    IconDir,
};

use super::EditorState;
use crate::sidebar;

pub(in crate::editor) use page::size_popup_ui;

use layer_common::layer_properties_ui;

/// Panel derecho: propiedades de la capa seleccionada.
pub fn properties_ui(state: &mut EditorState, ui: &mut egui::Ui) {
    sidebar::compact(ui);
    sidebar::title(ui, "Properties");
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            properties_ui_inner(state, ui);
        });
}

/// Consolida cualquier edición de panel a medias (`panel_edit`/`blur_edit`/
/// `color_edit`/`content_edit`/`shadow_edit`) cuya capa ya no es la
/// seleccionada. Esos campos solo se limpian solos cuando el control que los
/// arma detecta `lost_focus()`/`drag_stopped()` — pero ese control solo se
/// dibuja mientras su capa sigue siendo `selection.primary()`
/// (`layer_properties_ui`/`content_properties_ui`). Cambiar de capa (o
/// deseleccionar) a mitad de una edición hace que ese control desaparezca
/// del árbol de UI sin soltar nunca el foco, dejando el campo pegado en
/// `Some(...)` para siempre: la edición se pierde como paso de deshacer y,
/// para `content_edit`, además bloquea Ctrl+Z/Ctrl+Y de TODO el editor
/// (`handle_shortcuts` se los cede a un `TextEdit` con foco propio mientras
/// `content_edit.is_some()`).
fn commit_stale_panel_edits(state: &mut EditorState) {
    let current = state.selection.primary();

    if matches!(&state.panel_edit, Some((id, _)) if Some(*id) != current) {
        if let Some((id, before)) = state.panel_edit.take() {
            if let Ok(l) = state.doc.layer(id) {
                let after = l.transform;
                if after != before {
                    state.push_undo_step(Box::new(canvas_core::SetTransform {
                        layer: id,
                        before,
                        after,
                    }));
                }
            }
        }
    }
    if matches!(&state.opacity_edit, Some((id, _)) if Some(*id) != current) {
        effects::commit_opacity(state);
    }
    if matches!(&state.blur_edit, Some((id, _)) if Some(*id) != current) {
        if let Some((id, before)) = state.blur_edit.take() {
            let after = state
                .doc
                .layer(id)
                .map(|l| l.effects.blur_radius)
                .unwrap_or(before);
            if (after - before).abs() > f32::EPSILON {
                state.push_undo_step(Box::new(canvas_core::SetBlur {
                    layer: id,
                    before,
                    after,
                }));
            }
        }
    }
    if matches!(&state.color_edit, Some((id, _)) if Some(*id) != current) {
        if let Some((id, before)) = state.color_edit.take() {
            let after = state.doc.layer(id).map(|l| l.effects).unwrap_or(before);
            if after != before {
                state.push_undo_step(Box::new(canvas_core::SetEffects {
                    layer: id,
                    before,
                    after,
                }));
            }
        }
    }
    if matches!(&state.content_edit, Some((id, _)) if Some(*id) != current) {
        if let Some((id, before)) = state.content_edit.take() {
            let after = state
                .doc
                .layer(id)
                .map(|l| l.content.clone())
                .unwrap_or_else(|_| before.clone());
            if after != before {
                state.push_undo_step(Box::new(canvas_core::SetContent {
                    layer: id,
                    before,
                    after,
                }));
            }
        }
    }
    if matches!(&state.shadow_edit, Some((id, _)) if Some(*id) != current) {
        if let Some((id, before)) = state.shadow_edit.take() {
            let after = state.doc.layer(id).ok().and_then(|l| l.effects.shadow);
            if after != before {
                state.push_undo_step(Box::new(canvas_core::SetShadow {
                    layer: id,
                    before,
                    after,
                }));
            }
        }
    }
}

fn properties_ui_inner(state: &mut EditorState, ui: &mut egui::Ui) {
    commit_stale_panel_edits(state);
    ui.add_space(8.0);

    // Banner: el archivo cambió en disco fuera de la app.
    if state.external_change {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            "⚠ This file changed on disk outside Canvas Desktop.",
        );
        ui.horizontal(|ui| {
            if ui.button("Reload").clicked() {
                state.reload_requested = true;
            }
            if ui.button("Keep mine").clicked() {
                state.external_change = false;
            }
        });
        ui.separator();
    }

    // Banner de error de guardado/operación: el guard anti-blanco/incompleto
    // (Save/Export rechazados), un fallo del hilo de guardado, un pegado
    // vacío… Se descarta con el botón; la siguiente operación con éxito
    // también lo limpia (`save_error = None` en `start_save`/`start_export`).
    let _ = save_error_banner(state, ui);

    if state.from_gallery.is_some()
        && icon_text_button_ui(
            ui,
            true,
            |p, r, c| draw_triangle_icon(p, r, IconDir::Left, c),
            "Back to gallery",
            None,
            egui::Vec2::ZERO,
        )
        .clicked()
    {
        state.return_requested = true;
    }
    file_name_ui(state, ui);
    let page_dims = match state.doc.page() {
        Ok(p) => (p.width, p.height),
        Err(_) => (0.0, 0.0),
    };
    ui.weak(format!(
        "{} × {} px",
        page_dims.0 as i64, page_dims.1 as i64
    ));

    // Capa seleccionada: cada propiedad en su propio desplegable.
    if let Some(sel) = state.selection.primary() {
        if state.doc.layer(sel).is_ok() {
            layer_properties_ui(state, ui, sel, page_dims);
        } else {
            ui.weak("No layer selected.");
            ui.weak("Click the image to select it.");
        }
    } else {
        ui.weak("No layer selected.");
        ui.weak("Click the image to select it.");
    }

    // Fondo desenfocado: copia «cover» de la imagen, con blur 50 por defecto.
    sidebar::section(ui, "Blurred background", true, |ui| {
        let active = state.background_active();
        let can_toggle = active || state.background_source().is_some();
        let mut bg_on = active;
        let response = ui.add_enabled(
            can_toggle,
            egui::Checkbox::new(&mut bg_on, "Blurred background"),
        );
        if response.changed() && bg_on != active {
            state.set_blurred_background(bg_on);
        }
        if active {
            if let Some(id) = state.background_layer {
                effects::blur_control(state, ui, id);
            }
        }
    });
    ui.label(format!("Zoom: {:.0} %", state.viewport.zoom * 100.0));
    ui.weak("Wheel: pan · Shift+wheel: pan the other axis · Ctrl+wheel: zoom");
    ui.weak("Space/middle button: pan · Ctrl+0: fit");
    ui.weak("Ctrl+S: save · Ctrl+Shift+S: save as");
    ui.weak("Ctrl+C / Ctrl+V: copy layers, even between designs");
    ui.add_space(4.0);
    if icon_text_button_ui(ui, true, draw_gear_icon, "Settings", None, egui::Vec2::ZERO).clicked() {
        state.settings_clicked = true;
    }
}

/// Nombre del archivo abierto, arriba del panel: un lápiz lo vuelve editable
/// in-place (mismo patrón que el renombrado de la galería —
/// `gallery::gallery_cell` — y el de capas — `rename_edit_ui` más abajo)
/// cuando el documento ya tiene archivo en disco. Un diseño nuevo sin
/// guardar (`source_path` en `None`) no ofrece el lápiz: no hay nada que
/// renombrar todavía.
fn file_name_ui(state: &mut EditorState, ui: &mut egui::Ui) {
    let id = egui::Id::new("editor_file_rename");
    if state.file_rename_edit.is_some() {
        // Mismo patrón que `gallery::gallery_cell`: Escape cancela, perder
        // el foco confirma (comprobado antes que `lost_focus` para que el
        // propio Escape no dispare un commit).
        let mut cancel = false;
        let mut commit = false;
        if let Some(text) = state.file_rename_edit.as_mut() {
            let resp = ui.add(egui::TextEdit::singleline(text).id(id));
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                cancel = true;
            } else if resp.lost_focus() {
                commit = true;
            }
        }
        if cancel {
            state.file_rename_edit = None;
        } else if commit {
            if let Some(text) = state.file_rename_edit.take() {
                let new_stem = text.trim().to_owned();
                let original_stem = state
                    .doc
                    .source_path
                    .as_deref()
                    .and_then(|p| p.file_stem())
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                if !new_stem.is_empty() && new_stem != original_stem {
                    state.file_rename_requested = Some(new_stem);
                }
            }
        }
    } else {
        ui.horizontal(|ui| {
            ui.heading(state.file_name());
            if state.doc.source_path.is_some()
                && icon_button_ui(ui, 16.0, true, draw_pencil_icon).clicked()
            {
                let stem = state
                    .doc
                    .source_path
                    .as_deref()
                    .and_then(|p| p.file_stem())
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                state.file_rename_edit = Some(stem);
                ui.memory_mut(|m| m.request_focus(id));
            }
        });
    }
}

/// El banner de error de guardado/operación, aislado del resto del panel
/// para poder probarlo con el harness headless de egui de `tests.rs` sin
/// montar todo el panel: dispara con cualquier `save_error` pendiente y su
/// botón «Dismiss» lo descarta. Devuelve la `Response` del botón (su rect
/// permite al test clicar sobre él sin adivinar posiciones). `None` sin
/// error pendiente.
pub(super) fn save_error_banner(
    state: &mut EditorState,
    ui: &mut egui::Ui,
) -> Option<egui::Response> {
    if let Some(error) = &state.save_error {
        ui.colored_label(ui.visuals().error_fg_color, format!("⚠ {error}"));
        let resp = ui.button("Dismiss");
        if resp.clicked() {
            state.save_error = None;
        }
        ui.separator();
        Some(resp)
    } else {
        None
    }
}
