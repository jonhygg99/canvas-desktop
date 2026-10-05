//! Timeline del archivo completo, con tiradores de trim y cursor independientes.
use super::{timecode, TrimEdge, VideoEdit};
use eframe::egui;

#[path = "timeline_input.rs"]
mod input;
#[path = "timeline_selection.rs"]
mod selection;

#[derive(Clone, Copy)]
pub(super) enum Gesture {
    Edge(TrimEdge),
    Seek,
}

pub(super) struct Geometry {
    pub track: egui::Rect,
    pub start: f64,
    pub end: f64,
}

impl Geometry {
    pub fn x(&self, time: f64) -> f32 {
        self.track.left()
            + ((time - self.start) / (self.end - self.start).max(0.001)) as f32 * self.track.width()
    }
    pub fn time(&self, x: f32) -> f64 {
        self.start
            + f64::from(((x - self.track.left()) / self.track.width()).clamp(0.0, 1.0))
                * (self.end - self.start)
    }
}

pub(super) fn show(edit: &mut VideoEdit, ui: &mut egui::Ui) -> egui::Response {
    toolbar(edit, ui);
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), 94.0),
        egui::Sense::click_and_drag(),
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Slider, true, "Video timeline")
    });
    let (start, end) = edit.timeline_view();
    let geo = Geometry {
        track: rect.shrink2(egui::vec2(10.0, 20.0)),
        start,
        end,
    };
    paint(edit, ui, &geo);
    input::handle(edit, ui, &geo, &response);
    response
}

fn toolbar(edit: &mut VideoEdit, ui: &mut egui::Ui) {
    ui.horizontal_wrapped(|ui| {
        if ui.button("Fit entire video").clicked() {
            edit.timeline_range = None;
        }
        if ui.button("Fit selection").clicked() {
            edit.fit_timeline_selection();
        }
        if ui.button("−").on_hover_text("Zoom out timeline").clicked() {
            edit.zoom_timeline(2.0);
        }
        if ui.button("+").on_hover_text("Zoom in timeline").clicked() {
            edit.zoom_timeline(0.5);
        }
        ui.checkbox(&mut edit.loop_selection, "Loop selection");
    });
}

fn paint(edit: &mut VideoEdit, ui: &egui::Ui, geo: &Geometry) {
    let p = ui.painter();
    let track = geo.track;
    p.rect_filled(track, 3.0, ui.visuals().extreme_bg_color);
    let count = (track.width() / 72.0).ceil().clamp(1.0, 20.0) as usize;
    for i in 0..count {
        let time = geo.start + (geo.end - geo.start) * (i as f64 + 0.5) / count as f64;
        let cell = egui::Rect::from_min_max(
            egui::pos2(
                track.left() + track.width() * i as f32 / count as f32,
                track.top(),
            ),
            egui::pos2(
                track.left() + track.width() * (i + 1) as f32 / count as f32,
                track.bottom(),
            ),
        );
        if let Some(index) = super::frame_index_at(edit, time) {
            if let Some(texture) = edit
                .thumbnails
                .texture(index, &edit.frames[index], ui.ctx())
            {
                p.image(
                    texture,
                    cell,
                    egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }
        }
    }
    selection::paint(edit, ui, geo);
    for (x, time, align) in [
        (track.left(), geo.start, egui::Align2::LEFT_TOP),
        (track.right(), geo.end, egui::Align2::RIGHT_TOP),
    ] {
        p.text(
            egui::pos2(x, track.bottom() + 4.0),
            align,
            timecode(time),
            egui::FontId::monospace(11.0),
            ui.visuals().weak_text_color(),
        );
    }
}
