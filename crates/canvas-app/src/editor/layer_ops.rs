//! Operaciones puras de orden (z-order) y alineación de capas: no tocan
//! egui, solo el documento a través de comandos deshacibles.

use canvas_core::{Command, LayerId, Reorder, SetTransform, Transform};

use super::EditorState;

/// Destino de `reorder_layer` — el submenú "Layers" del menú contextual.
pub(super) enum ZOrder {
    Front,
    Forward,
    Backward,
    Back,
}

/// Posición de `id` entre sus hermanos: (padre, índice actual, último
/// índice). Compartida por `reorder_layer` (para calcular el destino) y el
/// submenú "Layers" (para deshabilitar los botones que ya no tendrían
/// efecto — Bring to Front/Move Forward en el extremo del frente, Move
/// Backward/Send to Back en el del fondo).
pub(super) fn sibling_position(
    state: &EditorState,
    id: LayerId,
) -> Option<(Option<LayerId>, usize, usize)> {
    let page = state.doc.page().ok()?;
    let parent = page.layer(id)?.parent_id;
    let siblings = page.children_of(parent);
    let current = siblings.iter().position(|&s| s == id)?;
    Some((parent, current, siblings.len() - 1))
}

/// Mueve `id` dentro de su grupo de hermanos, un paso o hasta el extremo.
/// Mismo comando (`Reorder`) y convención de índice que ya usa
/// `layers_panel::apply_reorder` para el arrastre en el panel de capas:
/// índice 0 = fondo de la pila, el último = frente.
pub(super) fn reorder_layer(state: &mut EditorState, id: LayerId, to: ZOrder) {
    let Some((parent, current, last)) = sibling_position(state, id) else {
        return;
    };
    let target = match to {
        ZOrder::Front => last,
        ZOrder::Forward => (current + 1).min(last),
        ZOrder::Backward => current.saturating_sub(1),
        ZOrder::Back => 0,
    };
    if target == current {
        return;
    }
    let mut cmd = Reorder::new(id, parent, target);
    if cmd.apply(&mut state.doc).is_ok() {
        state.push_undo_step(Box::new(cmd));
    }
}

/// Aplica un `Transform` ya calculado (los botones de "Align to Page" del
/// menú contextual) como un commit inmediato contra el transform ACTUAL de
/// la capa — más simple que la reconciliación con `panel_edit` que usa el
/// panel de propiedades, porque un clic de menú no es una edición en curso
/// a medias que consolidar.
pub(super) fn apply_alignment(state: &mut EditorState, sel: LayerId, after: Transform) {
    let Ok(before) = state.doc.layer(sel).map(|l| l.transform) else {
        return;
    };
    if after == before {
        return;
    }
    if let Err(e) = state.apply_undo_step(Box::new(SetTransform {
        layer: sel,
        before,
        after,
    })) {
        tracing::error!("alinear falló: {e}");
    }
}
