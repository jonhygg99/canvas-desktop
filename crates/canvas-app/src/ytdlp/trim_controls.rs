//! Ajustes de trim junto al vídeo: extremos, duración y accesos rápidos.
use super::{timecode, TrimEdge, VideoEdit};
use eframe::egui;

pub(super) fn trim_controls(edit: &mut VideoEdit, ui: &mut egui::Ui) {
    ui.strong("Trim");
    let Some(duration) = edit.duration else {
        ui.weak("Reading video duration…");
        return;
    };
    edge_field(edit, ui, TrimEdge::Start, duration);
    edge_field(edit, ui, TrimEdge::End, duration);
    duration_field(edit, ui, duration);
    ui.horizontal_wrapped(|ui| {
        if ui
            .button("Start at playhead")
            .on_hover_text("Set start here (I)")
            .clicked()
        {
            edit.set_trim_edge(TrimEdge::Start, edit.playhead);
        }
        if ui
            .button("End at playhead")
            .on_hover_text("Set end here (O)")
            .clicked()
        {
            edit.set_trim_edge(TrimEdge::End, edit.playhead + 1.0 / edit.source_fps);
        }
    });
    ui.horizontal_wrapped(|ui| {
        if ui
            .add_enabled(
                !edit.trim_history.undo.is_empty(),
                egui::Button::new("Undo trim"),
            )
            .clicked()
        {
            edit.undo_trim();
        }
        if ui
            .add_enabled(
                !edit.trim_history.redo.is_empty(),
                egui::Button::new("Redo trim"),
            )
            .clicked()
        {
            edit.redo_trim();
        }
        if ui.button("Reset trim").clicked() {
            edit.reset_trim();
        }
    });
}

fn edge_field(edit: &mut VideoEdit, ui: &mut egui::Ui, edge: TrimEdge, duration: f64) {
    let start = edge == TrimEdge::Start;
    let mut value = if start {
        edit.trim_start
    } else {
        edit.trim_end
    };
    let response = ui
        .horizontal(|ui| {
            ui.label(if start { "Start" } else { "End" });
            let response = ui.add(
                egui::DragValue::new(&mut value)
                    .range(0.0..=duration)
                    .speed(0.05)
                    .custom_formatter(|value, _| timecode(value))
                    .custom_parser(|text| {
                        crate::ytdlp::api::parse_time(text).filter(|v| v.is_finite())
                    }),
            );
            if super::transport_icons::restart_button(
                ui,
                true,
                if start { "Reset start" } else { "Reset end" },
            )
            .clicked()
            {
                edit.finish_trim_gesture();
                edit.set_trim_edge(edge, if start { 0.0 } else { duration });
                value = if start {
                    edit.trim_start
                } else {
                    edit.trim_end
                };
            }
            response
        })
        .inner;
    update_edge(edit, edge, value, &response);
    response.on_hover_text("Drag to adjust, or type HH:MM:SS.mmm or seconds");
    // El campo puede haber cruzado el otro extremo: usa el valor validado.
    value = if start {
        edit.trim_start
    } else {
        edit.trim_end
    };
    let response = ui.add(
        egui::Slider::new(&mut value, 0.0..=duration)
            .clamping(egui::SliderClamping::Edits)
            .step_by(0.01)
            .show_value(false),
    );
    update_edge(edit, edge, value, &response);
}

fn update_edge(edit: &mut VideoEdit, edge: TrimEdge, value: f64, response: &egui::Response) {
    if response.changed() {
        edit.begin_trim_gesture();
        edit.set_trim_edge(edge, value);
    }
    if response.drag_stopped() || response.lost_focus() || response.clicked() {
        edit.finish_trim_gesture();
    }
}

fn duration_field(edit: &mut VideoEdit, ui: &mut egui::Ui, duration: f64) {
    let mut seconds = edit.trim_end - edit.trim_start;
    let remaining = duration - edit.trim_start;
    let response = ui
        .horizontal(|ui| {
            ui.label("Duration");
            let response = ui.add(
                egui::DragValue::new(&mut seconds)
                    .range(0.1_f64.min(remaining)..=remaining)
                    .speed(0.1)
                    .suffix(" s")
                    .max_decimals(3),
            );
            if super::transport_icons::restart_button(ui, true, "Reset duration").clicked() {
                edit.finish_trim_gesture();
                edit.set_trim_duration(remaining);
                seconds = edit.trim_end - edit.trim_start;
            }
            response
        })
        .inner;
    if response.changed() {
        edit.begin_trim_gesture();
        edit.set_trim_duration(seconds);
    }
    if response.drag_stopped() || response.lost_focus() {
        edit.finish_trim_gesture();
    }
    ui.horizontal_wrapped(|ui| {
        for seconds in [5.0, 7.0, 10.0, 15.0] {
            if ui
                .add_enabled(
                    seconds <= remaining,
                    egui::Button::new(format!("{seconds} s")),
                )
                .clicked()
            {
                edit.finish_trim_gesture();
                edit.set_trim_duration(seconds);
            }
        }
    });
}
