use super::*;

fn render(
    ctx: &egui::Context,
    video: &mut Panel,
    settings: &mut AppSettings,
    events: Vec<egui::Event>,
) -> (egui::FullOutput, Option<Vec<VideoAccept>>) {
    let mut accepts = None;
    let output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1400.0, 1200.0),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            accepts = edit_window_ui(video, settings, ui);
        },
    );
    (output, accepts)
}

fn text_center(output: &egui::FullOutput, label: &str) -> egui::Pos2 {
    output
        .shapes
        .iter()
        .find_map(|shape| {
            if let egui::Shape::Text(text) = &shape.shape {
                (text.galley.job.text == label).then(|| text.pos + text.galley.size() / 2.0)
            } else {
                None
            }
        })
        .unwrap_or_else(|| panic!("missing {label}"))
}

fn click(pos: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(pos),
        egui::Event::PointerButton {
            pos,
            pressed,
            button: egui::PointerButton::Primary,
            modifiers: egui::Modifiers::NONE,
        },
    ]
}

#[test]
fn add_trim_and_create_canvases_buttons_return_every_independent_range() {
    let dir = tempfile::tempdir().unwrap();
    let frame = dir.path().join("frame.png");
    image::RgbaImage::new(16, 9).save(&frame).unwrap();
    let mut edit = VideoEdit::open(
        "clip",
        "Clip".into(),
        dir.path().join("clip.mp4"),
        Some(30.0),
        (1920.0, 1080.0),
        &Document::new(1920.0, 1080.0),
    );
    edit.set_frames(vec![frame], 1.0, 30.0, Some((16.0, 9.0)));
    edit.set_trim_duration(7.0);
    let mut video = Panel::default();
    video.edit = Some(edit);
    let mut settings = AppSettings::default();
    let ctx = egui::Context::default();
    render(&ctx, &mut video, &mut settings, vec![]);
    let (output, _) = render(&ctx, &mut video, &mut settings, vec![]);
    let add = text_center(&output, "Add trim");
    render(&ctx, &mut video, &mut settings, click(add, true));
    render(&ctx, &mut video, &mut settings, click(add, false));
    assert_eq!(
        video.edit.as_ref().unwrap().trim_ranges(),
        vec![(0.0, 7.0), (7.0, 14.0)]
    );
    let (output, _) = render(&ctx, &mut video, &mut settings, vec![]);
    let first = text_center(&output, "Trim 1 · 0.00–7.00 s");
    render(&ctx, &mut video, &mut settings, click(first, true));
    render(&ctx, &mut video, &mut settings, click(first, false));
    assert_eq!(video.edit.as_ref().unwrap().active_trim, 0);
    assert_eq!(video.edit.as_ref().unwrap().trim_end, 7.0);
    let (output, _) = render(&ctx, &mut video, &mut settings, vec![]);
    let create = text_center(&output, "Create 2 canvases");
    render(&ctx, &mut video, &mut settings, click(create, true));
    let (_, accepts) = render(&ctx, &mut video, &mut settings, click(create, false));
    let accepts = accepts.expect("create action");
    assert_eq!(accepts.len(), 2);
    assert_eq!(accepts[0].trim_start, 0.0);
    assert_eq!(accepts[1].trim_start, 7.0);
    assert!(video.edit.is_none());
}
