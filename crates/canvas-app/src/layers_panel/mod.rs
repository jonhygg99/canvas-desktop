//! Panel lateral izquierdo del editor: pestañas Page y Layers en
//! disposición vertical solo con icono (el nombre es la tooltip y el
//! título del menú que hay debajo), barra de iconos en estado
//! colapsado. Las pestañas son arrastrables entre sí
//! para reordenarlas y el orden queda persistido en los ajustes.
//!
//! Reparto: `tab_strip` (la tira vertical de pestañas: clic, arrastre e
//! intercambio animado), `tab_draw` (pintado de una pestaña), `insert`
//! (pestaña Insert) y `row`/`ops` (lista de capas y sus operaciones).

use canvas_core::{LayerContent, LayerId, Page};
use eframe::egui;

use crate::editor::properties_panel::page::page_ui;
use crate::editor::state::LeftTab;
use crate::editor::EditorState;
use crate::settings::LayersTabOrder;
use crate::sidebar;

mod insert;
mod ops;
mod row;
mod tab_draw;
mod tab_strip;

pub(crate) use ops::{group_selection, ungroup_selection};

use insert::insert_tab_ui;
use ops::{apply_reorder, toolbar_ui};
use row::row_ui;
pub(crate) use tab_strip::vertical_tab_strip_ui;

// Nombres que solo usan los tests (glob `use super::*` en `tests.rs`).
#[cfg(test)]
use insert::{insert_item, INSERT_ITEMS, INSERT_TILE_H};
#[cfg(test)]
use tab_strip::ordered_tabs;

struct Row {
    id: LayerId,
    depth: usize,
    is_group: bool,
    collapsed: bool,
}

fn push_rows(page: &Page, parent: Option<LayerId>, depth: usize, out: &mut Vec<Row>) {
    if depth > 64 {
        return;
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

#[derive(Debug, Clone)]
struct DragLayers(Vec<LayerId>);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Drop {
    Above(LayerId),
    Below(LayerId),
    Into(LayerId),
}

/// Devuelve el nuevo orden si una pestaña se soltó sobre la otra; el
/// llamador persiste el cambio en los ajustes.
pub fn left_panel_ui(
    state: &mut EditorState,
    ui: &mut egui::Ui,
    layers_collapsed: &mut bool,
    order: LayersTabOrder,
    tx: &std::sync::mpsc::Sender<crate::loader::AppMsg>,
) -> Option<LayersTabOrder> {
    sidebar::compact(ui);
    let mut new_order = None;
    // `ui.horizontal` arranca con altura = `interact_size.y` (22pt) y limita
    // a sus hijos a esa altura inicial: la tira de pestañas se dibuja
    // desbordando (el clic es geométrico, no de layout) pero el contenido
    // del panel —listas y scroll— quedaba aplastado a ~0pt. Con
    // `allocate_ui_with_layout` el layout horizontal recibe TODA la altura
    // disponible del panel y el contenido ocupa el vertical completo.
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), ui.available_height()),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            new_order = vertical_tab_strip_ui(
                ui,
                &mut state.active_left_tab,
                layers_collapsed,
                order,
                false,
            );
            ui.separator();
            // El nombre de la pestaña activa es también el TÍTULO del menú:
            // la tira quedó solo con iconos, y el título vuelve aquí, encima
            // de los elementos, con algo de aire arriba.
            ui.vertical(|ui| {
                ui.add_space(8.0);
                let tab_name = match state.active_left_tab {
                    LeftTab::Page => "Page",
                    LeftTab::Layers => "Layers",
                    LeftTab::Insert => "Insert",
                    LeftTab::Images => "Images",
                };
                sidebar::title(ui, tab_name);
                ui.add_space(6.0);
                match state.active_left_tab {
                    LeftTab::Page => {
                        page_ui(state, ui);
                    }
                    LeftTab::Images => crate::unsplash::panel_ui(state, ui, tx),
                    LeftTab::Layers => {
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
                    LeftTab::Insert => insert_tab_ui(state, ui),
                }
            });
        },
    );
    new_order
}

#[cfg(test)]
mod tests;
