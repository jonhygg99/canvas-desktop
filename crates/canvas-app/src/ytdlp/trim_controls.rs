//! Campos de tiempo y comandos de trim; todas las acciones son pulsables.
use super::{timecode, TrimEdge, VideoEdit};
use eframe::egui;

pub(super) fn trim_controls(edit: &mut VideoEdit, ui: &mut egui::Ui) {
    let Some(duration) = edit.duration else {
        return;
    };
    ui.horizontal_wrapped(|ui| {
        edge_field(edit, ui, TrimEdge::Start, duration);
        edge_field(edit, ui, TrimEdge::End, duration);
    });
    ui.horizontal_wrapped(|ui| {
        if ui.button("Set start here (I)").clicked() {
            edit.set_trim_edge(TrimEdge::Start, edit.playhead);
        }
        if ui.button("Set end here (O)").clicked() {
            edit.set_trim_edge(TrimEdge::End, edit.playhead + 1.0 / edit.source_fps);
        }
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
    ui.label(format!(
        "Start {}  ·  End {}  ·  Duration {}",
        timecode(edit.trim_start),
        timecode(edit.trim_end),
        timecode(edit.trim_end - edit.trim_start)
    ));
}

fn edge_field(edit: &mut VideoEdit, ui: &mut egui::Ui, edge: TrimEdge, duration: f64) {
    let start = edge == TrimEdge::Start;
    ui.label(if start { "Start" } else { "End" });
    let mut value = if start {
        edit.trim_start
    } else {
        edit.trim_end
    };
    let response = ui.add(
        egui::DragValue::new(&mut value)
            .range(0.0..=duration)
            .speed(0.001)
            .custom_formatter(|value, _| timecode(value))
            .custom_parser(|text| crate::ytdlp::api::parse_time(text).filter(|v| v.is_finite())),
    );
    if response.changed() {
        edit.begin_trim_gesture();
        edit.set_trim_edge(edge, value);
    }
    if response.drag_stopped() || response.lost_focus() {
        edit.finish_trim_gesture();
    }
    response.on_hover_text("Type HH:MM:SS.mmm or seconds; drag for a fine adjustment");
}
