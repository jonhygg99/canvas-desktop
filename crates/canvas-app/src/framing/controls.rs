use super::{jobs, Session};
use canvas_core::framing::Framing;
use eframe::egui;

pub(super) fn show(session: &mut Session, ui: &mut egui::Ui) {
    session.poll(ui.ctx());
    ui.heading("Framing 9:16");
    ui.weak("Drag the composition to frame it.");
    if let Some(error) = &session.error {
        ui.colored_label(ui.visuals().error_fg_color, error);
    }
    if let Some(status) = &session.status {
        ui.small(status);
    }
    let enabled = session.ready() && !session.busy();
    ui.add_enabled_ui(enabled, |ui| {
        for index in 0..3 {
            let before = session.value;
            let response = match index {
                0 => ui.add(egui::Slider::new(&mut session.value.x_pct, -100.0..=100.0).text("X")),
                1 => ui.add(egui::Slider::new(&mut session.value.y_pct, -100.0..=100.0).text("Y")),
                _ => ui
                    .add(egui::Slider::new(&mut session.value.scale_pct, 50..=200).text("Scale %")),
            };
            if response.drag_started() {
                session.gesture = Some(before);
            }
            if response.drag_stopped() {
                if let Some(before) = session.gesture.take() {
                    session.commit(before);
                }
            } else if response.changed() && !response.dragged() {
                session.commit(before);
            }
        }
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!session.undo.is_empty(), egui::Button::new("Undo"))
                .clicked()
            {
                session.undo();
            }
            if ui
                .add_enabled(!session.redo.is_empty(), egui::Button::new("Redo"))
                .clicked()
            {
                session.redo();
            }
            if ui.button("Reset").clicked() {
                let before = session.value;
                session.value = Framing::default();
                session.commit(before);
            }
        });
        if ui
            .add_enabled(session.path.is_some(), egui::Button::new("Save framing"))
            .clicked()
        {
            if let Some(path) = &session.path {
                session.receiver = Some(jobs::save(path.clone(), session.value, ui.ctx().clone()));
            }
        }
        if session.exportable && ui.button("Export for Flashcut-Auto").clicked() {
            if let Some(source) = &session.source {
                session.receiver = Some(jobs::export(
                    canvas_io::LoadedImage {
                        rgba: source.rgba.clone(),
                        width: source.width,
                        height: source.height,
                    },
                    session.value,
                    session.path.clone(),
                    ui.ctx().clone(),
                ));
            }
        }
        ui.weak(if session.saved == Some(session.value) {
            "Framing saved"
        } else {
            "Framing not saved"
        });
    });
    if session.busy() {
        ui.spinner();
    }
    if ui
        .add_enabled(!session.busy(), egui::Button::new("Back to normal view"))
        .clicked()
    {
        session.closed = true;
    }
    if enabled {
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z)) {
            session.undo();
        }
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Y)) {
            session.redo();
        }
    }
}
