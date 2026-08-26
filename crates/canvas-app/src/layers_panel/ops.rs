//! Las operaciones de la barra del panel (agrupar, desagrupar, borrar) y el
//! reordenado por arrastre, ya traducidos a comandos de `canvas_core`.

use canvas_core::{Command, Composite, Group, LayerId, Page, Reorder, Ungroup};
use eframe::egui;

use crate::app_icons::{
    draw_delete_icon, draw_group_icon, draw_ungroup_icon, icon_button_ui, icon_text_button_ui,
};
use crate::editor::EditorState;

use super::Drop;

pub(super) fn toolbar_ui(state: &mut EditorState, ui: &mut egui::Ui) {
    ui.horizontal_wrapped(|ui| {
        let (can_group, can_ungroup, can_delete) = match state.doc.page() {
            Ok(page) => {
                let roots = groupable_roots(state, page);
                let ungroupable = state
                    .selection
                    .roots(page)
                    .into_iter()
                    .any(|id| page.is_group(id));
                (
                    !roots.is_empty(),
                    ungroupable,
                    crate::editor::has_deletable_selection(state),
                )
            }
            Err(_) => (false, false, false),
        };

        if icon_button_ui(ui, 18.0, can_group, draw_group_icon)
            .on_hover_text("Group (Ctrl+G)")
            .clicked()
            && can_group
        {
            group_selection(state);
        }
        if icon_button_ui(ui, 18.0, can_ungroup, draw_ungroup_icon)
            .on_hover_text("Ungroup (Ctrl+Shift+G)")
            .clicked()
            && can_ungroup
        {
            ungroup_selection(state);
        }
        // El rojo de «Delete» es deliberado (destructivo con confirmación
        // implícita a la Papelera); el resto de botones usa el color del
        // estado.
        let del_c = egui::Color32::from_rgb(220, 70, 70);
        if icon_text_button_ui(
            ui,
            can_delete,
            move |p, r, _| draw_delete_icon(p, r, del_c),
            "Delete",
            Some(del_c),
            egui::Vec2::ZERO,
        )
        .clicked()
            && can_delete
        {
            crate::editor::delete_selected(state);
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
        state.push_undo_step(Box::new(cmd));
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
        state.push_undo_step(Box::new(Composite::new("Desagrupar", cmds)));
    }
    state.forget_deleted_selection();
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
pub(super) fn apply_reorder(state: &mut EditorState, ids: &[LayerId], drop: Drop) {
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
        state.push_undo_step(Box::new(Composite::new("Reordenar capas", cmds)));
    }
}
