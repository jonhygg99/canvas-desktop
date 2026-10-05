//! Transporte de vídeo con símbolos vectoriales sin depender de la fuente.
use eframe::egui;

pub(super) fn play_button(ui: &mut egui::Ui, playing: bool, enabled: bool) -> egui::Response {
    let label = if playing { "Pause" } else { "Play" };
    crate::app_icons::icon_button_ui(ui, 30.0, enabled, label, |p, rect, color| {
        let c = rect.center();
        if playing {
            for x in [c.x - 4.0, c.x + 4.0] {
                p.line_segment(
                    [egui::pos2(x, c.y - 8.0), egui::pos2(x, c.y + 8.0)],
                    egui::Stroke::new(3.0, color),
                );
            }
        } else {
            p.add(egui::Shape::convex_polygon(
                vec![
                    c + egui::vec2(-7.0, -8.0),
                    c + egui::vec2(7.0, 0.0),
                    c + egui::vec2(-7.0, 8.0),
                ],
                color,
                egui::Stroke::NONE,
            ));
        }
    })
    .on_hover_text(label)
}

pub(super) fn restart_button(ui: &mut egui::Ui, enabled: bool, label: &str) -> egui::Response {
    crate::app_icons::icon_button_ui(ui, 30.0, enabled, label, |p, rect, color| {
        let c = rect.center();
        let points = (0..=24)
            .map(|i| {
                let angle =
                    -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::TAU * 0.85 / 24.0;
                c + egui::vec2(angle.cos(), angle.sin()) * 8.0
            })
            .collect();
        p.add(egui::Shape::line(points, egui::Stroke::new(1.5, color)));
        p.line_segment(
            [c + egui::vec2(-4.0, -12.0), c + egui::vec2(0.0, -8.0)],
            egui::Stroke::new(1.5, color),
        );
        p.line_segment(
            [c + egui::vec2(-4.0, -4.0), c + egui::vec2(0.0, -8.0)],
            egui::Stroke::new(1.5, color),
        );
    })
    .on_hover_text(label)
}

pub(super) fn mute_button(ui: &mut egui::Ui, mute: bool, enabled: bool) -> egui::Response {
    let label = if mute {
        "Unmute preview"
    } else {
        "Mute preview"
    };
    crate::app_icons::icon_button_ui(ui, 30.0, enabled, label, |p, rect, color| {
        let c = rect.center();
        p.rect_filled(
            egui::Rect::from_min_size(c + egui::vec2(-9.0, -3.0), egui::vec2(5.0, 6.0)),
            0.0,
            color,
        );
        p.add(egui::Shape::convex_polygon(
            vec![
                c + egui::vec2(-5.0, -3.0),
                c + egui::vec2(0.0, -8.0),
                c + egui::vec2(0.0, 8.0),
                c + egui::vec2(-5.0, 3.0),
            ],
            color,
            egui::Stroke::NONE,
        ));
        let stroke = egui::Stroke::new(1.5, color);
        if mute {
            for direction in [-1.0, 1.0] {
                p.line_segment(
                    [
                        c + egui::vec2(4.0, direction * 4.0),
                        c + egui::vec2(10.0, -direction * 4.0),
                    ],
                    stroke,
                );
            }
        } else {
            p.add(egui::Shape::line(
                vec![
                    c + egui::vec2(4.0, -6.0),
                    c + egui::vec2(8.0, 0.0),
                    c + egui::vec2(4.0, 6.0),
                ],
                stroke,
            ));
        }
    })
    .on_hover_text(label)
}

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
