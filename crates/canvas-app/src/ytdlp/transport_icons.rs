//! Avance por fotograma: símbolos vectoriales sin depender de la fuente.
use eframe::egui;

pub(super) fn frame_button(ui: &mut egui::Ui, next: bool) -> egui::Response {
    let label = if next { "Next frame" } else { "Previous frame" };
    crate::app_icons::icon_button_ui(ui, 30.0, true, label, |p, rect, color| {
        let c = rect.center();
        let direction = if next { 1.0 } else { -1.0 };
        let points = vec![
            egui::pos2(c.x - direction * 5.0, c.y - 5.0),
            egui::pos2(c.x + direction * 3.0, c.y),
            egui::pos2(c.x - direction * 5.0, c.y + 5.0),
        ];
        p.add(egui::Shape::convex_polygon(
            points,
            color,
            egui::Stroke::NONE,
        ));
        let x = c.x + direction * 6.0;
        p.line_segment(
            [egui::pos2(x, c.y - 5.0), egui::pos2(x, c.y + 5.0)],
            egui::Stroke::new(1.5, color),
        );
    })
    .on_hover_text(if next {
        "Next frame (→)"
    } else {
        "Previous frame (←)"
    })
}
