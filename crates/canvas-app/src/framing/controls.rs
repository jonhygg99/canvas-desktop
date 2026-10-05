use super::{jobs, Session};
use canvas_core::framing::Framing;
use eframe::egui;

pub(super) fn show(session: &mut Session, ui: &mut egui::Ui) {
    session.poll(ui.ctx());
    ui.heading("Framing 9:16");
    ui.weak("Drag the composition to frame it.");
    ui.weak("Framing changes position and scale without changing the original design or its dimensions.");
    if session.exportable {
        ui.weak("This preview is a snapshot. Return to Edit to change layers, then enter framing again.");
    }
    if let Some(error) = &session.error {
        ui.colored_label(ui.visuals().error_fg_color, error);
    }
    if let Some(status) = &session.status {
        ui.small(status);
    }
    let enabled = session.ready() && !session.busy();
    ui.add_enabled_ui(enabled, |ui| {
        if let Some(video) = &mut session.video {
            video.controls(ui);
        }
        adjustments(session, ui);
        ui.checkbox(&mut session.show_guides, "Composition guides (10% inset)");
        if ui.button("Center composition").clicked() {
            let before = session.value;
            session.value.x_pct = 0.0;
            session.value.y_pct = 0.0;
            session.commit(before);
        }
        if ui
            .push_id("framing-save", |ui| {
                ui.add_enabled(session.path.is_some(), egui::Button::new("Save framing"))
            })
            .inner
            .clicked()
        {
            session.save(ui.ctx());
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
        ui.weak(if !session.is_dirty() {
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
        session.request_close();
    }
    if enabled {
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::S)) {
            session.save(ui.ctx());
        }
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z)) {
            session.undo();
        }
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Y)) {
            session.redo();
        }
    }
    close_dialog(session, ui.ctx());
}

fn close_dialog(session: &mut Session, ctx: &egui::Context) {
    if !session.close_requested {
        return;
    }
    egui::Modal::new(egui::Id::new("framing-unsaved")).show(ctx, |ui| {
        ui.heading("Unsaved framing changes");
        ui.label("Save your framing before returning to Edit?");
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(session.path.is_some() && !session.busy(), egui::Button::new("Save and return")).clicked() {
                session.close_after_save = true;
                session.save(ctx);
                session.close_requested = false;
            }
            if ui.button("Discard framing changes").clicked() {
                session.closed = true;
                session.close_requested = false;
            }
            if ui.button("Keep editing").clicked() {
                session.close_requested = false;
            }
        });
    });
}

fn adjustments(session: &mut Session, ui: &mut egui::Ui) {
    for index in 0..3 {
        let before = session.value;
        let response = match index {
            0 => ui.add(egui::Slider::new(&mut session.value.x_pct, -100.0..=100.0).text("X")),
            1 => ui.add(egui::Slider::new(&mut session.value.y_pct, -100.0..=100.0).text("Y")),
            _ => ui.add(egui::Slider::new(&mut session.value.scale_pct, 50..=200).text("Scale %")),
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
}
