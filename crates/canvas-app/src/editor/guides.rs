//! Guías persistentes: edición numérica y bloqueo, fuera del contenido exportado.
use super::{viewport::page_to_screen, EditorState};
use canvas_core::{Guide, SetGuides};
use eframe::egui;

pub(super) fn is_open(state: &EditorState, ctx: &egui::Context) -> bool {
    ctx.data(|d| {
        d.get_temp::<Vec<Guide>>(egui::Id::new(("guide_editor", state.active_slot_id)))
            .is_some()
    })
}

pub(super) fn controls(state: &mut EditorState, ui: &mut egui::Ui) {
    ui.menu_button(crate::i18n::tr("Guides"), |ui| {
        ui.checkbox(&mut state.show_rulers, crate::i18n::tr("Show rulers"));
        if ui
            .add_enabled(
                state.is_idle(),
                egui::Button::new(crate::i18n::tr("Edit guides…")),
            )
            .clicked()
        {
            let key = egui::Id::new(("guide_editor", state.active_slot_id));
            let guides = state
                .doc
                .page()
                .map(|p| p.guides.clone())
                .unwrap_or_default();
            ui.data_mut(|d| d.insert_temp(key, guides));
            ui.close();
        }
    });
}

pub(super) fn window(state: &mut EditorState, ctx: &egui::Context) {
    let key = egui::Id::new(("guide_editor", state.active_slot_id));
    let Some(mut guides) = ctx.data(|d| d.get_temp::<Vec<Guide>>(key)) else {
        return;
    };
    let mut open = true;
    let mut apply = false;
    let mut cancel = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
    egui::Window::new(crate::i18n::tr("Guides"))
        .id(key)
        .open(&mut open)
        .collapsible(false)
        .default_width(380.0)
        .show(ctx, |ui| {
            ui.checkbox(&mut state.show_rulers, crate::i18n::tr("Show rulers"));
            ui.horizontal(|ui| {
                for (vertical, label) in [
                    (true, "Add vertical guide"),
                    (false, "Add horizontal guide"),
                ] {
                    if ui.button(crate::i18n::tr(label)).clicked() {
                        let position = state
                            .doc
                            .page()
                            .map(|p| if vertical { p.width } else { p.height })
                            .unwrap_or(0.0)
                            / 2.0;
                        guides.push(Guide {
                            vertical,
                            position,
                            locked: false,
                        });
                    }
                }
            });
            egui::ScrollArea::vertical()
                .max_height(280.0)
                .show(ui, |ui| rows(state, ui, &mut guides));
            ui.separator();
            ui.horizontal(|ui| {
                apply = ui
                    .add_enabled(state.is_idle(), egui::Button::new(crate::i18n::tr("Apply")))
                    .clicked();
                cancel |= ui.button(crate::i18n::tr("Cancel")).clicked();
            });
        });
    if apply {
        let before = state
            .doc
            .page()
            .map(|p| p.guides.clone())
            .unwrap_or_default();
        if before != guides {
            let _ = state.apply_undo_step(Box::new(SetGuides {
                before,
                after: guides,
            }));
        }
    } else if open && !cancel {
        ctx.data_mut(|d| d.insert_temp(key, guides));
        return;
    }
    ctx.data_mut(|d| d.remove::<Vec<Guide>>(key));
}

fn rows(state: &EditorState, ui: &mut egui::Ui, guides: &mut Vec<Guide>) {
    let mut remove = None;
    for (index, guide) in guides.iter_mut().enumerate() {
        ui.push_id(index, |ui| {
            ui.horizontal(|ui| {
                ui.label(if guide.vertical { "X" } else { "Y" });
                let limit = state
                    .doc
                    .page()
                    .map(|p| if guide.vertical { p.width } else { p.height })
                    .unwrap_or(0.0)
                    .max(0.0);
                ui.add_enabled(
                    !guide.locked,
                    egui::DragValue::new(&mut guide.position)
                        .range(0.0..=limit)
                        .suffix(" px"),
                );
                ui.checkbox(&mut guide.locked, crate::i18n::tr("Locked"));
                if ui.button(crate::i18n::tr("Remove")).clicked() {
                    remove = Some(index);
                }
            })
        });
    }
    if let Some(index) = remove {
        guides.remove(index);
    }
    if guides.is_empty() {
        ui.weak(crate::i18n::tr("No guides yet"));
    }
}

pub(super) fn draw(state: &EditorState, ui: &egui::Ui, coord: egui::Rect, clip: egui::Rect) {
    let Ok(page) = state.doc.page() else { return };
    let painter = ui.painter_at(clip);
    for guide in &page.guides {
        if !guide.position.is_finite() {
            continue;
        }
        let pos = page_to_screen(&state.viewport, coord, guide.position, guide.position);
        let (a, b) = if guide.vertical {
            (
                egui::pos2(pos.x, clip.top()),
                egui::pos2(pos.x, clip.bottom()),
            )
        } else {
            (
                egui::pos2(clip.left(), pos.y),
                egui::pos2(clip.right(), pos.y),
            )
        };
        if !a.is_finite() || !b.is_finite() {
            continue;
        }
        let color = if guide.locked {
            egui::Color32::from_rgb(155, 125, 230)
        } else {
            egui::Color32::from_rgb(0, 180, 220)
        };
        painter.line_segment([a, b], egui::Stroke::new(1.0, color));
    }
}
