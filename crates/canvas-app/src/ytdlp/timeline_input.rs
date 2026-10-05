//! Interacción de timeline con ratón y teclado; el foco delimita los atajos.
use super::{Geometry, Gesture, TrimEdge, VideoEdit};
use eframe::egui;

pub(super) fn handle(
    edit: &mut VideoEdit,
    ui: &mut egui::Ui,
    geo: &Geometry,
    response: &egui::Response,
) {
    if response.is_pointer_button_down_on() || response.clicked() || response.drag_stopped() {
        if let Some(pos) = response.interact_pointer_pos() {
            if edit.timeline_gesture.is_none() {
                let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(pos);
                let a = (origin.x - geo.x(edit.trim_start)).abs();
                let b = (origin.x - geo.x(edit.trim_end)).abs();
                edit.timeline_gesture = Some(if a.min(b) <= 12.0 {
                    edit.begin_trim_gesture();
                    Gesture::Edge(if a <= b {
                        TrimEdge::Start
                    } else {
                        TrimEdge::End
                    })
                } else {
                    Gesture::Seek
                });
                response.request_focus();
            }
            let time = geo.time(pos.x);
            match edit.timeline_gesture {
                Some(Gesture::Edge(edge)) => edit.set_trim_edge(edge, time),
                Some(Gesture::Seek) => edit.seek(time),
                None => {}
            }
        }
    }
    if !ui.input(|i| i.pointer.primary_down()) {
        edit.finish_trim_gesture();
        edit.timeline_gesture = None;
    }
    if let Some(pos) = response.hover_pos() {
        let near_edge = (pos.x - geo.x(edit.trim_start))
            .abs()
            .min((pos.x - geo.x(edit.trim_end)).abs())
            <= 12.0;
        ui.ctx().set_cursor_icon(if near_edge {
            egui::CursorIcon::ResizeHorizontal
        } else {
            egui::CursorIcon::Crosshair
        });
        response.clone().on_hover_text(format!(
            "{} · I: start · O: end · ←/→: frame · Space: play/pause",
            super::timecode(geo.time(pos.x))
        ));
    }
    if response.has_focus() {
        keyboard(edit, ui);
    }
}

fn keyboard(edit: &mut VideoEdit, ui: &mut egui::Ui) {
    let take = |ui: &mut egui::Ui, modifiers, key| ui.input_mut(|i| i.consume_key(modifiers, key));
    if take(ui, egui::Modifiers::NONE, egui::Key::I) {
        edit.set_trim_edge(TrimEdge::Start, edit.playhead);
    }
    if take(ui, egui::Modifiers::NONE, egui::Key::O) {
        edit.set_trim_edge(TrimEdge::End, edit.playhead + 1.0 / edit.source_fps);
    }
    if take(ui, egui::Modifiers::COMMAND, egui::Key::Z) {
        edit.undo_trim();
    }
    if take(ui, egui::Modifiers::COMMAND, egui::Key::Y)
        || take(
            ui,
            egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
            egui::Key::Z,
        )
    {
        edit.redo_trim();
    }
    for (key, direction) in [(egui::Key::ArrowLeft, -1.0), (egui::Key::ArrowRight, 1.0)] {
        if take(ui, egui::Modifiers::NONE, key) {
            edit.seek(edit.playhead + direction / edit.source_fps);
        }
    }
    if take(ui, egui::Modifiers::NONE, egui::Key::Space) && !edit.frames.is_empty() {
        if edit.playing {
            edit.seek(edit.playhead);
        } else {
            edit.prepare_playback_start();
            edit.playing = true;
            edit.last_tick = Some(std::time::Instant::now());
            ui.ctx().request_repaint();
        }
    }
}
