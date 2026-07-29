//! Panel de capas: lista de arriba abajo (la cima de la pila primero),
//! arrastrar para reordenar/reagrupar, renombrar con doble clic, ojo de
//! visibilidad y candado, y una barra de Agrupar/Desagrupar/Borrar.

use canvas_core::{
    Command, Composite, Group, LayerContent, LayerId, Page, RemoveLayer, Rename, Reorder,
    SetLocked, SetVisible, Ungroup,
};
use eframe::egui;

use crate::editor::EditorState;

/// Fila del panel, de ARRIBA abajo (la cima de la pila primero). La cabecera
/// de un grupo va antes que sus hijos aquí, aunque en `Page::layers` (la
/// invariante de preorden que usa el renderer) vaya justo por DEBAJO.
struct Row {
    id: LayerId,
    depth: usize,
    is_group: bool,
    collapsed: bool,
}

/// Construye la lista de filas recorriendo el árbol en profundidad, de
/// arriba abajo dentro de cada nivel de hermanos.
fn push_rows(page: &Page, parent: Option<LayerId>, depth: usize, out: &mut Vec<Row>) {
    if depth > 64 {
        return; // red de seguridad ante un árbol corrupto (normalize_tree ya protege, pero...)
    }
    for id in page.children_of(parent).into_iter().rev() {
        let Some(layer) = page.layer(id) else {
            continue;
        };
        let is_group = matches!(layer.content, LayerContent::Group(_));
        let collapsed = match &layer.content {
            LayerContent::Group(g) => g.collapsed,
            _ => false,
        };
        out.push(Row {
            id,
            depth,
            is_group,
            collapsed,
        });
        if is_group && !collapsed {
            push_rows(page, Some(id), depth + 1, out);
        }
    }
}

/// Payload de arrastre: los ids que se están moviendo (la fila tocada, o
/// toda la selección múltiple si la fila tocada ya formaba parte de ella).
#[derive(Debug, Clone)]
struct DragLayers(Vec<LayerId>);

/// Destino de una soltada, relativo a la fila sobre la que se suelta.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Drop {
    /// Justo encima en la pila (índice de hermano uno mayor).
    Above(LayerId),
    /// Justo debajo en la pila (mismo índice de hermano que el objetivo).
    Below(LayerId),
    /// Dentro del grupo, al tope.
    Into(LayerId),
}

pub fn layers_panel_ui(state: &mut EditorState, ui: &mut egui::Ui) {
    ui.heading("Layers");
    toolbar_ui(state, ui);
    ui.separator();

    let Ok(page) = state.doc.page() else {
        ui.weak("No document.");
        return;
    };
    let mut rows = Vec::new();
    push_rows(page, None, 0, &mut rows);
    let is_empty = rows.is_empty();

    let mut pending_drop: Option<(Vec<LayerId>, Drop)> = None;
    egui::ScrollArea::vertical().show(ui, |ui| {
        for row in &rows {
            if let Some(drop) = row_ui(state, ui, row) {
                pending_drop = Some(drop);
            }
        }
        if is_empty {
            ui.weak("No layers yet.");
        }
    });

    if let Some((ids, drop)) = pending_drop {
        apply_reorder(state, &ids, drop);
    }
}

fn toolbar_ui(state: &mut EditorState, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        let (can_group, can_ungroup, can_delete) = match state.doc.page() {
            Ok(page) => {
                let roots = groupable_roots(state, page);
                let ungroupable = state
                    .selection
                    .roots(page)
                    .into_iter()
                    .any(|id| page.is_group(id));
                (!roots.is_empty(), ungroupable, !state.selection.is_empty())
            }
            Err(_) => (false, false, false),
        };

        if ui
            .add_enabled(can_group, egui::Button::new("▣ Group"))
            .on_hover_text("Ctrl+G")
            .clicked()
        {
            group_selection(state);
        }
        if ui
            .add_enabled(can_ungroup, egui::Button::new("▤ Ungroup"))
            .on_hover_text("Ctrl+Shift+G")
            .clicked()
        {
            ungroup_selection(state);
        }
        if ui
            .add_enabled(can_delete, egui::Button::new("🗑 Delete"))
            .clicked()
        {
            delete_selection(state);
        }
    });
}

/// La selección, sin descendientes-de-otro-miembro (ver `Selection::roots`)
/// y sin la capa de fondo desenfocado (agruparla no tiene sentido: siempre
/// debe quedar como la más baja de la página).
fn groupable_roots(state: &EditorState, page: &Page) -> Vec<LayerId> {
    let background = state.background_layer;
    state
        .selection
        .roots(page)
        .into_iter()
        .filter(|&id| Some(id) != background)
        .collect()
}

/// `pub(crate)`: también la usa el atajo Ctrl+G en `EditorState::handle_shortcuts`.
pub(crate) fn group_selection(state: &mut EditorState) {
    let Ok(page) = state.doc.page() else { return };
    let roots = groupable_roots(state, page);
    if roots.is_empty() {
        return;
    }
    let group_id = state.doc.allocate_layer_id();
    let mut cmd = Group::new(roots, group_id, "Group");
    if cmd.apply(&mut state.doc).is_ok() {
        state.history.push_applied(Box::new(cmd));
        state.selection.set(Some(group_id));
    }
}

/// `pub(crate)`: también la usa el atajo Ctrl+Shift+G en `EditorState::handle_shortcuts`.
pub(crate) fn ungroup_selection(state: &mut EditorState) {
    let Ok(page) = state.doc.page() else { return };
    let groups: Vec<LayerId> = state
        .selection
        .roots(page)
        .into_iter()
        .filter(|&id| page.is_group(id))
        .collect();
    if groups.is_empty() {
        return;
    }
    let mut cmds: Vec<Box<dyn Command>> = Vec::new();
    for id in groups {
        let mut cmd = Ungroup::new(id);
        if cmd.apply(&mut state.doc).is_ok() {
            cmds.push(Box::new(cmd));
        }
    }
    if !cmds.is_empty() {
        state
            .history
            .push_applied(Box::new(Composite::new("Desagrupar", cmds)));
    }
    state.forget_deleted_selection();
}

fn delete_selection(state: &mut EditorState) {
    let Ok(page) = state.doc.page() else { return };
    let roots = state.selection.roots(page);
    if roots.is_empty() {
        return;
    }
    let mut cmds: Vec<Box<dyn Command>> = Vec::new();
    for id in roots {
        let mut cmd = RemoveLayer::new(id);
        if cmd.apply(&mut state.doc).is_ok() {
            cmds.push(Box::new(cmd));
        }
    }
    if !cmds.is_empty() {
        state
            .history
            .push_applied(Box::new(Composite::new("Quitar capas", cmds)));
    }
    state.selection.clear();
}

/// Traduce un destino de soltada a `(padre, índice de hermano)` para
/// `Reorder`, contando la lista de hermanos SIN la capa que se está moviendo.
fn reorder_for(page: &Page, moving: LayerId, drop: Drop) -> Option<(Option<LayerId>, usize)> {
    match drop {
        Drop::Into(group) => {
            let mut siblings = page.children_of(Some(group));
            siblings.retain(|&s| s != moving);
            Some((Some(group), siblings.len()))
        }
        Drop::Above(target) | Drop::Below(target) => {
            let parent = page.layer(target)?.parent_id;
            let mut siblings = page.children_of(parent);
            siblings.retain(|&s| s != moving);
            let at = siblings.iter().position(|&s| s == target)?;
            // "Above" en el panel (más arriba en la lista) = índice de
            // hermano mayor (más arriba en la pila); "Below" = el mismo
            // índice que el objetivo (la movida pasa a ocupar su lugar).
            let index = if matches!(drop, Drop::Above(_)) {
                at + 1
            } else {
                at
            };
            Some((parent, index))
        }
    }
}

/// Aplica el arrastre de `ids` (en orden de pila, para que conserven su
/// apilamiento relativo) al destino `drop`, como un solo paso de deshacer.
fn apply_reorder(state: &mut EditorState, ids: &[LayerId], drop: Drop) {
    let mut ordered = ids.to_vec();
    if let Ok(page) = state.doc.page() {
        ordered.sort_by_key(|&id| page.index_of(id).unwrap_or(usize::MAX));
    }
    let mut cmds: Vec<Box<dyn Command>> = Vec::new();
    for (k, &id) in ordered.iter().enumerate() {
        let target = state
            .doc
            .page()
            .ok()
            .and_then(|page| reorder_for(page, id, drop));
        let Some((parent, index)) = target else {
            continue;
        };
        let mut cmd = Reorder::new(id, parent, index + k);
        if cmd.apply(&mut state.doc).is_ok() {
            cmds.push(Box::new(cmd));
        }
    }
    if !cmds.is_empty() {
        state
            .history
            .push_applied(Box::new(Composite::new("Reordenar capas", cmds)));
    }
}

/// Dibuja una fila y gestiona su interacción (selección, arrastre, doble
/// clic para renombrar). Devuelve el destino de una soltada si esta fila
/// fue el objetivo de un arrastre soltado en este frame.
fn row_ui(state: &mut EditorState, ui: &mut egui::Ui, row: &Row) -> Option<(Vec<LayerId>, Drop)> {
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
        ui.add_space(row.depth as f32 * 14.0);

        if row.is_group {
            let arrow = if row.collapsed { "▸" } else { "▾" };
            if ui.small_button(arrow).clicked() {
                if let Ok(layer) = state.doc.layer_mut(row.id) {
                    if let LayerContent::Group(g) = &mut layer.content {
                        g.collapsed = !g.collapsed;
                    }
                }
            }
        } else {
            ui.add_space(20.0);
        }

        let eye = if visible { "👁" } else { "🚫" };
        if ui
            .small_button(eye)
            .on_hover_text("Toggle visibility")
            .clicked()
        {
            let mut cmd = SetVisible {
                layer: row.id,
                before: visible,
                after: !visible,
            };
            if cmd.apply(&mut state.doc).is_ok() {
                state.history.push_applied(Box::new(cmd));
            }
        }

        let lock = if locked { "🔒" } else { "🔓" };
        if ui.small_button(lock).on_hover_text("Toggle lock").clicked() {
            let mut cmd = SetLocked {
                layer: row.id,
                before: locked,
                after: !locked,
            };
            if cmd.apply(&mut state.doc).is_ok() {
                state.history.push_applied(Box::new(cmd));
            }
        }

        if is_background {
            ui.label("🌫").on_hover_text("Blurred background");
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
                egui::RichText::new(name).strong()
            } else {
                egui::RichText::new(name)
            };
            ui.add(egui::Label::new(text).selectable(false));
        }
    });
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
                    state.history.push_applied(Box::new(cmd));
                }
            }
        }
    }
}
