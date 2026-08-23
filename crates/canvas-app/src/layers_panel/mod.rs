//! Panel de capas: lista de arriba abajo (la cima de la pila primero),
//! arrastrar para reordenar/reagrupar, renombrar con doble clic, ojo de
//! visibilidad y candado, y una barra de Agrupar/Desagrupar/Borrar.

use canvas_core::{LayerContent, LayerId, Page};
use eframe::egui;

use crate::editor::EditorState;
use crate::sidebar;

mod ops;
mod row;

pub(crate) use ops::{group_selection, ungroup_selection};

use ops::{apply_reorder, toolbar_ui};
use row::row_ui;

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
    sidebar::compact(ui);
    sidebar::title(ui, "Layers");
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
