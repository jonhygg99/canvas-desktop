//! Menu contextual del area de edicion (clic derecho). Solo las acciones que
//! de verdad se usan desde un clic derecho: no una copia entera del menu Edit,
//! que ya esta a un atajo de teclado o al menu superior de distancia.
//!
//! Movido tal cual desde el closure de `canvas_ui`: el cuerpo es identico,
//! solo cambian `state` y `action` de capturas a parametros.

use canvas_core::{LayerContent, LayerId, Transform};
use eframe::egui;

use super::super::layer_ops::{apply_alignment, reorder_layer, sibling_position, ZOrder};
use super::super::EditorState;
use super::CanvasAction;

pub(super) fn canvas_context_menu(
    ui: &mut egui::Ui,
    state: &mut EditorState,
    action: &mut Option<CanvasAction>,
) {
    use crate::menus::MenuAction;
    let mut item = |ui: &mut egui::Ui, label: &str, enabled: bool, a: MenuAction| {
        if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
            *action = Some(CanvasAction::Menu(a));
            ui.close();
        }
    };
    item(ui, "Undo", state.can_undo(), MenuAction::Undo);
    item(ui, "Redo", state.can_redo(), MenuAction::Redo);
    ui.separator();
    item(ui, "Cut", true, MenuAction::Cut);
    item(ui, "Copy", true, MenuAction::Copy);
    item(ui, "Paste", true, MenuAction::Paste);
    item(ui, "Duplicate", true, MenuAction::Duplicate);
    item(ui, "Delete", true, MenuAction::Delete);
    ui.separator();
    item(ui, "Select All", true, MenuAction::SelectAll);
    item(ui, "Group", true, MenuAction::Group);
    item(ui, "Ungroup", true, MenuAction::Ungroup);
    let selected_image = state.selection.primary().filter(|id| {
        state
            .doc
            .layer(*id)
            .ok()
            .is_some_and(|l| matches!(l.content, LayerContent::Image(_)))
    });
    let design_sources: Vec<(LayerId, String)> = selected_image
        .and_then(|target| {
            state.doc.page().ok().map(|page| {
                page.layers
                    .iter()
                    .filter(|layer| {
                        layer.id != target
                            && matches!(layer.content, LayerContent::Image(_))
                            && state.images.contains_key(&layer.id)
                    })
                    .map(|layer| (layer.id, layer.name.clone()))
                    .collect()
            })
        })
        .unwrap_or_default();
    ui.add_enabled_ui(selected_image.is_some(), |ui| {
        ui.menu_button("Replace", |ui| {
            let Some(target) = selected_image else {
                return;
            };

            ui.menu_button("From this design", |ui| {
                if design_sources.is_empty() {
                    ui.add_enabled(false, egui::Button::new("No other images"));
                }
                for (source, name) in &design_sources {
                    if ui.button(name).clicked() {
                        if let Err(e) = state.replace_image_from_layer(target, *source) {
                            state.save_error = Some(e);
                        }
                        ui.close();
                    }
                }
            });

            if ui.button("From local file").clicked() {
                *action = Some(CanvasAction::ReplaceFromLocal(target));
                ui.close();
            }
            if ui.button("From internet URL").clicked() {
                state.replace_url_popup = Some((target, String::new()));
                ui.close();
            }
        });
    });
    ui.separator();
    // Orden y alineación de la capa PRIMARIA seleccionada — deshabilitados
    // enteros (el propio botón del submenú) sin selección, en vez de
    // mostrar el submenú vacío o con todo gris dentro.
    let sel = state.selection.primary();
    ui.add_enabled_ui(sel.is_some(), |ui| {
        ui.menu_button("Layers", |ui| {
            let Some(id) = sel else {
                return;
            };
            // Bring to Front/Move Forward no tendrían efecto si ya está
            // en el extremo del frente (`current == last`); Move
            // Backward/Send to Back igual en el del fondo (`current ==
            // 0`) — se deshabilitan en vez de dejarlos ahí sin más,
            // para que el diseño lo demuestre en vez de solo no-opear.
            let range = sibling_position(state, id);
            let can_go_forward = range.is_some_and(|(_, current, last)| current < last);
            let can_go_backward = range.is_some_and(|(_, current, _)| current > 0);
            let mut z = |ui: &mut egui::Ui, label: &str, enabled: bool, to: ZOrder| {
                if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
                    reorder_layer(state, id, to);
                    ui.close();
                }
            };
            z(ui, "Bring to Front", can_go_forward, ZOrder::Front);
            z(ui, "Move Forward", can_go_forward, ZOrder::Forward);
            z(ui, "Move Backward", can_go_backward, ZOrder::Backward);
            z(ui, "Send to Back", can_go_backward, ZOrder::Back);
        });
    });
    ui.add_enabled_ui(sel.is_some(), |ui| {
        ui.menu_button("Align to Page", |ui| {
            let Some(id) = sel else {
                return;
            };
            let Ok(page) = state.doc.page() else {
                return;
            };
            let (page_w, page_h) = (page.width, page.height);
            let Some(t) = state.doc.layer(id).ok().map(|l| l.transform) else {
                return;
            };
            // `selectable_label`, no `button`: resalta la opción que YA
            // coincide con la posición actual de la capa — mismo widget
            // que ya usa este archivo para "elegido entre varias
            // opciones" (alineación de texto, más abajo en este mismo
            // módulo).
            let mut a = |ui: &mut egui::Ui, label: &str, after: Transform| {
                if ui.selectable_label(after == t, label).clicked() {
                    apply_alignment(state, id, after);
                    ui.close();
                }
            };
            a(
                ui,
                "Left",
                canvas_core::align_horizontal(&t, page_w, canvas_core::HAlign::Left),
            );
            a(
                ui,
                "Center",
                canvas_core::align_horizontal(&t, page_w, canvas_core::HAlign::Center),
            );
            a(
                ui,
                "Right",
                canvas_core::align_horizontal(&t, page_w, canvas_core::HAlign::Right),
            );
            ui.separator();
            a(
                ui,
                "Top",
                canvas_core::align_vertical(&t, page_h, canvas_core::VAlign::Top),
            );
            a(
                ui,
                "Middle",
                canvas_core::align_vertical(&t, page_h, canvas_core::VAlign::Middle),
            );
            a(
                ui,
                "Bottom",
                canvas_core::align_vertical(&t, page_h, canvas_core::VAlign::Bottom),
            );
            ui.separator();
            let centered_h = canvas_core::align_horizontal(&t, page_w, canvas_core::HAlign::Center);
            let centered =
                canvas_core::align_vertical(&centered_h, page_h, canvas_core::VAlign::Middle);
            a(ui, "Center on page", centered);
        });
    });
    ui.separator();
    // Estos tres, a diferencia de los de arriba, se resuelven AQUÍ
    // MISMO con `state` directamente — no tocan disco ni el resto de
    // `App`, así que no hace falta pasarlos por `CanvasAction`/`main.rs`.
    let bg_active = state.background_active();
    let bg_can_toggle = bg_active || state.background_source().is_some();
    let mut bg_on = bg_active;
    if ui
        .add_enabled(
            bg_can_toggle,
            egui::Checkbox::new(&mut bg_on, "Blurred background"),
        )
        .clicked()
    {
        state.set_blurred_background(bg_on);
        ui.close();
    }
    let crop_eligible = state
        .selection
        .primary()
        .and_then(|id| state.doc.layer(id).ok())
        .is_some_and(|l| matches!(l.content, LayerContent::Image(_)));
    let mut crop_on = state.crop_mode;
    if ui
        .add_enabled(crop_eligible, egui::Checkbox::new(&mut crop_on, "Crop"))
        .clicked()
    {
        state.crop_mode = crop_on;
        ui.close();
    }
    if ui.button("Size").clicked() {
        state.size_popup = state.doc.page().ok().map(|p| (p.width, p.height));
        ui.close();
    }
}
