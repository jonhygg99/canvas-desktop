//! Indicación visible del tramo conservado, los extremos y el cursor.
use super::{Geometry, VideoEdit};
use eframe::egui;

pub(super) fn paint(edit: &VideoEdit, ui: &egui::Ui, geo: &Geometry) {
    let track = geo.track;
    let p = ui.painter().with_clip_rect(track.expand(12.0));
    let (start, end) = (geo.x(edit.trim_start), geo.x(edit.trim_end));
    let shade = egui::Color32::from_black_alpha(180);
    for (a, b) in [
        (track.left(), start.clamp(track.left(), track.right())),
        (end.clamp(track.left(), track.right()), track.right()),
    ] {
        p.rect_filled(
            egui::Rect::from_min_max(egui::pos2(a, track.top()), egui::pos2(b, track.bottom())),
            0.0,
            shade,
        );
    }
    let selected = egui::Rect::from_min_max(
        egui::pos2(start.max(track.left()), track.top()),
        egui::pos2(end.min(track.right()), track.bottom()),
    );
    if selected.is_positive() {
        p.rect_stroke(
            selected,
            2.0,
            egui::Stroke::new(2.0, ui.visuals().selection.bg_fill),
            egui::StrokeKind::Inside,
        );
    }
    for x in [start, end] {
        if x >= track.left() && x <= track.right() {
            let handle = egui::Rect::from_center_size(
                egui::pos2(x, track.center().y),
                egui::vec2(14.0, track.height() + 8.0),
            );
            p.rect_filled(handle, 3.0, ui.visuals().selection.bg_fill);
            for dx in [-2.0, 2.0] {
                p.line_segment(
                    [
                        egui::pos2(x + dx, track.center().y - 8.0),
                        egui::pos2(x + dx, track.center().y + 8.0),
                    ],
                    egui::Stroke::new(1.0, ui.visuals().selection.stroke.color),
                );
            }
        }
    }
    let cursor = geo.x(edit.playhead);
    if cursor >= track.left() && cursor <= track.right() {
        p.line_segment(
            [
                egui::pos2(cursor, track.top() - 9.0),
                egui::pos2(cursor, track.bottom()),
            ],
            egui::Stroke::new(2.0, egui::Color32::WHITE),
        );
        p.circle_filled(
            egui::pos2(cursor, track.top() - 9.0),
            4.0,
            egui::Color32::WHITE,
        );
    }
}
