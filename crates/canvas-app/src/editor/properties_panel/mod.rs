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
mod page;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

use canvas_core::LayerContent;
use eframe::egui;

use super::EditorState;
use crate::sidebar;

pub(in crate::editor) use page::size_popup_ui;

use layer_common::layer_properties_ui;
use page::page_ui;

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

    if state.from_gallery.is_some() && ui.button("⏴ Back to gallery").clicked() {
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
    ui.separator();

    sidebar::section(ui, "Page", true, |ui| {
        page_ui(state, ui);
    });

    sidebar::section(ui, "Insert", false, |ui| {
        ui.horizontal_wrapped(|ui| {
            if ui.button("T Text").clicked() {
                state.insert_layer_centered(
                    "Text",
                    500.0,
                    120.0,
                    LayerContent::Text(canvas_core::TextContent::default()),
                );
            }
            if ui.small_button("R").on_hover_text("Rectangle").clicked() {
                state.insert_layer_centered(
                    "Rectangle",
                    320.0,
                    220.0,
                    LayerContent::Shape(canvas_core::ShapeContent::default()),
                );
            }
            if ui.small_button("O").on_hover_text("Ellipse").clicked() {
                state.insert_layer_centered(
                    "Ellipse",
                    280.0,
                    280.0,
                    LayerContent::Shape(canvas_core::ShapeContent {
                        kind: canvas_core::ShapeKind::Ellipse,
                        ..Default::default()
                    }),
                );
            }
            if ui.small_button("L").on_hover_text("Line").clicked() {
                state.insert_layer_centered(
                    "Line",
                    400.0,
                    24.0,
                    LayerContent::Shape(canvas_core::ShapeContent {
                        kind: canvas_core::ShapeKind::Line,
                        stroke: [30, 30, 30, 255],
                        stroke_width: 6.0,
                        ..Default::default()
                    }),
                );
            }
        });
    });

    sidebar::section(ui, "Layer", true, |ui| {
        if let Some(sel) = state.selection.primary() {
            if state.doc.layer(sel).is_ok() {
                layer_properties_ui(state, ui, sel, page_dims);
            }
        } else {
            ui.weak("No layer selected.");
            ui.weak("Click the image to select it.");
        }
    });

    sidebar::section(ui, "File actions", false, |ui| {
        ui.horizontal_wrapped(|ui| {
            let dirty_mark = if state.is_dirty() { " •" } else { "" };
            if ui
                .add_enabled(
                    !state.saving,
                    egui::Button::new(format!("💾 Save{dirty_mark}")),
                )
                .clicked()
            {
                state.save_clicked = true;
            }
            if ui
                .add_enabled(!state.saving, egui::Button::new("Save as…"))
                .clicked()
            {
                state.save_as_clicked = true;
            }
            // Va a la Papelera de reciclaje (`trash::delete`), no borrado
            // permanente: recuperable si el usuario se equivoca, así que no
            // hace falta pedir confirmación aparte.
            if ui
                .add_enabled(
                    !state.saving,
                    egui::Button::new(
                        egui::RichText::new("Delete").color(egui::Color32::from_rgb(220, 70, 70)),
                    ),
                )
                .clicked()
            {
                state.delete_requested = true;
            }
        });
        if state.is_design {
            ui.weak("Design file (.canvas) — layers are always kept.");
        } else {
            ui.checkbox(&mut state.sidecar_enabled, "Editable sidecar (.canvas)")
                .on_hover_text(
                    "Writes a .canvas file next to the image so the layers stay \
                     editable when you reopen it. Turn it off if you don't want \
                     extra files in your folders.",
                );
        }
        if state.saving {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new());
                ui.label("Saving…");
            });
        }
        if let Some(error) = state.save_error.clone() {
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(ui.visuals().error_fg_color, &error);
                if ui.small_button("✕").clicked() {
                    state.save_error = None;
                }
            });
        }
    });
    ui.label(format!("Zoom: {:.0} %", state.viewport.zoom * 100.0));
    ui.weak("Wheel: pan · Shift+wheel: pan the other axis · Ctrl+wheel: zoom");
    ui.weak("Space/middle button: pan · Ctrl+0: fit");
    ui.weak("Ctrl+S: save · Ctrl+Shift+S: save as");
    ui.weak("Ctrl+C / Ctrl+V: copy layers, even between designs");
    ui.add_space(4.0);
    if ui.small_button("⚙ Settings").clicked() {
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
            if state.doc.source_path.is_some() && ui.small_button("✏").clicked() {
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
