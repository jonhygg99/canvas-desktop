//! Una fila del panel: sangria por profundidad, ojo de visibilidad, candado,
//! nombre (con renombrado in situ) y las zonas de soltar del arrastre.

use canvas_core::{Command, LayerContent, LayerId, Rename, SetLocked, SetVisible};
use eframe::egui;

use crate::app_icons::{
    draw_blur_icon, draw_eye_icon, draw_lock_icon, draw_triangle_icon, icon_label_ui, IconDir,
};
use crate::editor::EditorState;

use super::{DragLayers, Drop, Row};

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

    let response = if is_background {
        // Fila fijada: no arrastrable (una capa de fondo por encima de otras
        // capas no tiene sentido).
        ui.scope(|ui| row_contents(state, ui, row, &name, visible, locked, is_background))
            .response
    } else {
        ui.dnd_drag_source(row_id, DragLayers(dragged), |ui| {
            row_contents(state, ui, row, &name, visible, locked, is_background)
        })
        .response
    };

    let mut drop_result = None;
    if let Some(payload) = response.dnd_hover_payload::<DragLayers>() {
        // No dejar que la propia fila arrastrada (o alguna de las
        // seleccionadas con ella) sea su propio destino.
        if !payload.0.contains(&row.id) {
            if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
                let frac = ((pos.y - response.rect.top()) / response.rect.height().max(1.0))
                    .clamp(0.0, 1.0);
                let drop = if is_background {
                    // Nada se suelta DENTRO ni DEBAJO del fondo: como mucho,
                    // justo encima de él.
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
                paint_drop_hint(ui, response.rect, drop);
                if ui.input(|i| i.pointer.any_released()) {
                    drop_result = Some(((*payload).clone(), drop));
                }
            }
        }
    }

    if !renaming {
        let click = ui.interact(response.rect, row_id.with("click"), egui::Sense::click());
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

    drop_result.map(|(payload, drop)| (payload.0, drop))
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

/// Contenido de una fila: indentación, flecha de plegado (grupos), ojo,
/// candado, marcador de fondo, y el nombre (o su `TextEdit` mientras se
/// renombra).
fn row_contents(
    state: &mut EditorState,
    ui: &mut egui::Ui,
    row: &Row,
    name: &str,
    visible: bool,
    locked: bool,
    is_background: bool,
) {
    ui.horizontal(|ui| {
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

        let (eye_clicked, eye_resp) = hover_button_ui(ui, GROUP_ARROW_W, true, move |p, r, c| {
            draw_eye_icon(p, r, visible, c)
        });
        eye_resp.on_hover_text("Toggle visibility");
        if eye_clicked {
            let mut cmd = SetVisible {
                layer: row.id,
                before: visible,
                after: !visible,
            };
            if cmd.apply(&mut state.doc).is_ok() {
                state.push_undo_step(Box::new(cmd));
            }
        }

        let (lock_clicked, lock_resp) = hover_button_ui(ui, GROUP_ARROW_W, true, move |p, r, c| {
            draw_lock_icon(p, r, locked, c)
        });
        lock_resp.on_hover_text("Toggle lock");
        if lock_clicked {
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

        let renaming = state
            .rename_edit
            .as_ref()
            .is_some_and(|(id, ..)| *id == row.id);
        if renaming {
            rename_edit_ui(state, ui, row.id);
        } else {
            // Etiqueta simple sin sentido de clic propio: la selección la
            // gestiona el `ui.interact` sobre toda la fila, en `row_ui`.
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
    });
}

/// Ancho del botón de plegado de un grupo; también el hueco que deja una
/// fila sin grupo, para que la columna de la flecha quede alineada.
const GROUP_ARROW_W: f32 = 18.0;

/// Botón de icono con `Sense::hover()` y detección manual del clic
/// (comprobando que `press_origin` esté dentro del rect). Funciona
/// incluso dentro de `dnd_drag_source`, donde `Sense::click()` no
/// recibe el evento porque el drag lo consume.
/// Devuelve `(clicked, response)` para que el llamador pueda encadenar
/// `.on_hover_text()` sobre el response.
fn hover_button_ui(
    ui: &mut egui::Ui,
    size: f32,
    enabled: bool,
    draw: impl FnOnce(&egui::Painter, egui::Rect, egui::Color32),
) -> (bool, egui::Response) {
    let visuals = ui.visuals().clone();
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let hovered = enabled && resp.hovered();
    if hovered {
        ui.painter()
            .rect_filled(rect, 4.0, visuals.widgets.hovered.weak_bg_fill);
    }
    let color = if !enabled {
        visuals.weak_text_color()
    } else if hovered {
        visuals.widgets.active.text_color()
    } else {
        visuals.widgets.inactive.text_color()
    };
    draw(&ui.painter(), rect, color);
    let clicked = ui.input(|i| {
        i.pointer.primary_clicked()
            && i.pointer.press_origin().map_or(false, |o| rect.contains(o))
    });
    (clicked, resp)
}

/// Botón de plegado de grupo: un triángulo relleno de `app_icons` — el
/// MISMO que usan las cabeceras de los lienzos y el resto de la app —
/// apuntando a la derecha cuando el grupo está plegado y hacia abajo
/// cuando está desplegado, con la mecánica estándar de los botones de
/// icono (fondo suave al hover).
fn group_arrow_ui(ui: &mut egui::Ui, collapsed: bool) -> bool {
    let dir = if collapsed { IconDir::Right } else { IconDir::Down };
    hover_button_ui(ui, GROUP_ARROW_W, true, move |p, r, c| {
        draw_triangle_icon(p, r, dir, c)
    })
    .0
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
