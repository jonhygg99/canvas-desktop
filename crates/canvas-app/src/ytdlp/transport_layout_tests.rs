use super::*;

#[test]
fn playback_is_centered_independently_of_the_right_aligned_canvas_size() {
    for width in [320.0, 700.0] {
        let mut edit = VideoEdit::open(
            "clip",
            "Clip".into(),
            PathBuf::new(),
            Some(30.0),
            (1920.0, 1080.0),
            &Document::new(1920.0, 1080.0),
        );
        let mut settings = AppSettings::default();
        let ctx = egui::Context::default();
        let mut play = egui::Rect::NOTHING;
        let mut center = 0.0;
        let mut group_width = 0.0;
        let output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 200.0),
                )),
                ..Default::default()
            },
            |ui| {
                center = ui.available_rect_before_wrap().center().x;
                group_width = 150.0 + 4.0 * ui.spacing().item_spacing.x;
                play = transport_ui(&mut edit, ui, &mut settings)[0].rect;
            },
        );
        assert!((play.left() + group_width / 2.0 - center).abs() < 0.1);
        let size_text = output
            .shapes
            .iter()
            .find_map(|shape| {
                if let egui::Shape::Text(text) = &shape.shape {
                    (text.galley.job.text == "Full HD 1920 × 1080").then_some(text.pos)
                } else {
                    None
                }
            })
            .expect("canvas size selector");
        if width >= 700.0 {
            assert!(size_text.x > play.left() + group_width);
            assert!((size_text.y - play.top()).abs() < 15.0);
        } else {
            assert!(size_text.y >= play.bottom());
        }
    }
}
