//! Una fila del panel: sangria por profundidad, ojo de visibilidad, candado,
//! nombre (con renombrado in situ) y las zonas de soltar del arrastre.

use canvas_core::{Command, LayerContent, LayerId, Rename, SetLocked, SetVisible};
use eframe::egui;

use crate::app_icons::{
    draw_blur_icon, draw_eye_icon, draw_lock_icon, draw_triangle_icon, icon_button_ui,
    icon_label_ui, IconDir,
};
use crate::editor::EditorState;

use super::{DragLayers, Drop, Row};

/// Ancho del botón de plegado de un grupo; también el hueco que deja una
/// fila sin grupo, para que la columna de la flecha quede alineada.
const GROUP_ARROW_W: f32 = 18.0;

/// Dibuja una fila y gestiona su interacción (selección, arrastre, doble
/// clic para renombrar). Devuelve el destino de una soltada si esta fila
/// fue el objetivo de un arrastre soltado en este frame.
pub(super) fn row_ui(
    state: &mut EditorState,
    ui: &mut egui::Ui,
    row: &Row,
) -> Option<(Vec<LayerId>, Drop)> {
    let Ok(layer) = state.doc.layer(row.id) else {
        return None;
    };
    let name = layer.name.clone();
    let visible = layer.visible;
    let locked = layer.locked;
    let is_background = Some(row.id) == state.background_layer;
    let renaming = state
        .rename_edit
        .as_ref()
        .is_some_and(|(id, ..)| *id == row.id);

    let row_id = egui::Id::new(("layer_row", row.id.raw()));
    let dragged: Vec<LayerId> = if state.selection.contains(row.id) && state.selection.len() > 1 {
        state.selection.ids().to_vec()
    } else {
        vec![row.id]
    };

    // Los botones van FUERA del `dnd_drag_source`: usan `Sense::click()`
    // normal y no compiten con el drag. Solo la etiqueta del nombre es
    // arrastrable. Medimos el cursor antes y después del prefijo para
    // calcular el rect que ocupan los botones; luego unimos ese rect con
    // el del drag source para el clic de selección y el drop hint.
    let drag_response = ui.horizontal(|ui| {
        let cursor_before = ui.cursor().min;

        // ── Botones del prefijo (fuera del drag) ──
        row_prefix_buttons(state, ui, row, visible, locked, is_background);

        let cursor_after = ui.cursor().min;
        let prefix_rect =
            egui::Rect::from_min_max(cursor_before, cursor_after);

        // ── Etiqueta (dentro del drag source, salvo fondo) ──
        let drag_resp = if is_background {
            ui.scope(|ui| {
                row_label_ui(state, ui, row, &name, renaming);
            })
            .response
        } else {
            ui.dnd_drag_source(row_id, DragLayers(dragged), |ui| {
                row_label_ui(state, ui, row, &name, renaming);
            })
            .response
        };

        let full_rect = prefix_rect.union(drag_resp.rect);

        // ── Drop (arrastrar otra fila sobre esta) ──
        let mut drop_result = None;
        if let Some(payload) = drag_resp.dnd_hover_payload::<DragLayers>() {
            if !payload.0.contains(&row.id) {
                if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
                    let frac = ((pos.y - full_rect.top()) / full_rect.height().max(1.0))
                        .clamp(0.0, 1.0);
                    let drop = if is_background {
                        Drop::Above(row.id)
                    } else if frac < 0.25 {
                        Drop::Above(row.id)
                    } else if frac > 0.75 {
                        Drop::Below(row.id)
                    } else if row.is_group {
                        Drop::Into(row.id)
                    } else if frac < 0.5 {
                        Drop::Above(row.id)
                    } else {
                        Drop::Below(row.id)
                    };
                    paint_drop_hint(ui, full_rect, drop);
                    if ui.input(|i| i.pointer.any_released()) {
                        drop_result = Some(((*payload).clone(), drop));
                    }
                }
            }
        }

        // ── Clic / doble clic de selección ──
        if !renaming {
            let click =
                ui.interact(full_rect, row_id.with("click"), egui::Sense::click());
            if click.double_clicked() {
                state.rename_edit = Some((row.id, name.clone(), name));
                ui.memory_mut(|m| m.request_focus(row_id.with("rename")));
            } else if click.clicked() {
                let mods = ui.input(|i| i.modifiers);
                if mods.command {
                    state.selection.toggle(row.id);
                } else if mods.shift {
                    if let Ok(page) = state.doc.page() {
                        state.selection.extend_range(page, row.id);
                    }
                } else {
                    state.selection.set(Some(row.id));
                }
            }
        }

        drop_result
    });

    drag_response.inner.map(|(payload, drop)| (payload.0, drop))
}

/// Botones del prefijo de la fila (fuera del drag source): indentación,
/// flecha de plegado, ojo, candado, marcador de fondo.
fn row_prefix_buttons(
    state: &mut EditorState,
    ui: &mut egui::Ui,
    row: &Row,
    visible: bool,
    locked: bool,
    is_background: bool,
) {
    ui.add_space(row.depth as f32 * 10.0);

    if row.is_group {
        if group_arrow_ui(ui, row.collapsed) {
            if let Ok(layer) = state.doc.layer_mut(row.id) {
                if let LayerContent::Group(g) = &mut layer.content {
                    g.collapsed = !g.collapsed;
                }
            }
        }
    } else {
        ui.add_space(GROUP_ARROW_W);
    }

    if icon_button_ui(ui, GROUP_ARROW_W, true, move |p, r, c| {
        draw_eye_icon(p, r, visible, c)
    })
    .on_hover_text("Toggle visibility")
    .clicked()
    {
        let mut cmd = SetVisible {
            layer: row.id,
            before: visible,
            after: !visible,
        };
        if cmd.apply(&mut state.doc).is_ok() {
            state.push_undo_step(Box::new(cmd));
        }
    }

    if icon_button_ui(ui, GROUP_ARROW_W, true, move |p, r, c| {
        draw_lock_icon(p, r, locked, c)
    })
    .on_hover_text("Toggle lock")
    .clicked()
    {
        let mut cmd = SetLocked {
            layer: row.id,
            before: locked,
            after: !locked,
        };
        if cmd.apply(&mut state.doc).is_ok() {
            state.push_undo_step(Box::new(cmd));
        }
    }

    if is_background {
        icon_label_ui(ui, GROUP_ARROW_W, |p, r, c| draw_blur_icon(p, r, c))
            .on_hover_text("Blurred background");
    }
}

/// Etiqueta del nombre (o el `TextEdit` de renombrado). Renderizada dentro
/// del `dnd_drag_source`, que solo cubre esta zona.
fn row_label_ui(
    state: &mut EditorState,
    ui: &mut egui::Ui,
    row: &Row,
    name: &str,
    renaming: bool,
) {
    if renaming {
        rename_edit_ui(state, ui, row.id);
    } else {
        let selected = state.selection.contains(row.id);
        let text = if selected {
            egui::RichText::new(name)
                .strong()
                .color(egui::Color32::from_rgb(0, 122, 255))
        } else {
            egui::RichText::new(name)
        };
        ui.add(egui::Label::new(text).selectable(false));
    }
}

fn paint_drop_hint(ui: &egui::Ui, rect: egui::Rect, drop: Drop) {
    let stroke = egui::Stroke::new(2.0, egui::Color32::from_rgb(0, 122, 255));
    match drop {
        Drop::Above(_) => {
            ui.painter().hline(rect.x_range(), rect.top(), stroke);
        }
        Drop::Below(_) => {
            ui.painter().hline(rect.x_range(), rect.bottom(), stroke);
        }
        Drop::Into(_) => {
            ui.painter()
                .rect_stroke(rect, 2.0, stroke, egui::StrokeKind::Inside);
        }
    }
}

/// Botón de plegado de grupo: un triángulo relleno de `app_icons` — el
/// MISMO que usan las cabeceras de los lienzos y el resto de la app —
/// apuntando a la derecha cuando el grupo está plegado y hacia abajo
/// cuando está desplegado.
fn group_arrow_ui(ui: &mut egui::Ui, collapsed: bool) -> bool {
    let dir = if collapsed { IconDir::Right } else { IconDir::Down };
    icon_button_ui(ui, GROUP_ARROW_W, true, move |p, r, c| {
        draw_triangle_icon(p, r, dir, c)
    })
    .clicked()
}

fn rename_edit_ui(state: &mut EditorState, ui: &mut egui::Ui, id: LayerId) {
    let text_id = egui::Id::new(("layer_row", id.raw())).with("rename");
    let mut cancel = false;
    let mut commit = false;
    if let Some((_, text, _)) = state.rename_edit.as_mut() {
        let resp = ui.add(
            egui::TextEdit::singleline(text)
                .id(text_id)
                .desired_width(140.0),
        );
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            cancel = true;
        } else if resp.lost_focus() {
            commit = true;
        }
    }
    if cancel {
        state.rename_edit = None;
    } else if commit {
        if let Some((layer, text, original)) = state.rename_edit.take() {
            let text = text.trim().to_owned();
            if !text.is_empty() && text != original {
                let mut cmd = Rename {
                    layer,
                    before: original,
                    after: text,
                };
                if cmd.apply(&mut state.doc).is_ok() {
                    state.push_undo_step(Box::new(cmd));
                }
            }
        }
    }
}