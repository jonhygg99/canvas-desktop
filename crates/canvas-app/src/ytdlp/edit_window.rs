//! Ventana adaptable de edicion de video.
use super::*;

pub fn edit_window_ui(
    video: &mut Panel,
    settings: &mut AppSettings,
    ui: &mut egui::Ui,
) -> Option<VideoAccept> {
    let title = format!("Edit video — {}", video.edit.as_ref()?.title);
    let mut open = true;
    let mut close = false;
    let mut accept = None;
    egui::Window::new(title)
        .id(egui::Id::new("video-editor"))
        .order(egui::Order::Foreground)
        .open(&mut open)
        .resizable(true)
        .default_size(egui::vec2(1040.0, 760.0))
        .min_size(egui::vec2(320.0, 380.0))
        .show(ui.ctx(), |ui| {
            let edit = video.edit.as_mut().expect("checked above");
            advance_playhead(edit, &ui.ctx().clone());
            egui::ScrollArea::vertical()
                .max_height(
                    (ui.available_height() - 55.0)
                        .min(ui.ctx().content_rect().height() - 170.0)
                        .max(160.0),
                )
                .show(ui, |ui| body(edit, settings, ui));
            ui.separator();
            ui.horizontal(|ui| {
                let ready =
                    !edit.loading_frames && edit.frames_error.is_none() && !edit.frames.is_empty();
                let btn = ui.add_enabled(ready, egui::Button::new("Create canvas"));
                if btn.clicked() {
                    accept = Some(build_accept(edit));
                }
                if !ready {
                    btn.on_hover_text(crate::i18n::tr("Waiting for preview frames"));
                }
                if ui.button("Cancel").clicked() {
                    close = true;
                }
            });
        });
    if !open || close {
        stop_preview_audio(video);
        video.edit = None;
        return None;
    }
    if accept.is_some() {
        stop_preview_audio(video);
        video.edit = None;
    }
    accept
}

/// Corta el audio de la preview al cerrar/aceptar (el lienzo no autoplayea).
fn stop_preview_audio(video: &Panel) {
    if let Some(edit) = video.edit.as_ref() {
        crate::audio::pause_for(&edit.path);
    }
}

fn body(edit: &mut VideoEdit, settings: &mut AppSettings, ui: &mut egui::Ui) {
    if ui.available_width() >= 760.0 {
        let width = ui.available_width() - 340.0;
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(width, 420.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(width);
                    preview_ui(edit, ui);
                    transport_ui(edit, ui);
                },
            );
            ui.allocate_ui_with_layout(
                egui::vec2(320.0, 440.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(320.0);
                    params_ui(edit, ui, settings);
                },
            );
        });
    } else {
        preview_ui(edit, ui);
        transport_ui(edit, ui);
        params_ui(edit, ui, settings);
    }
    ui.separator();
    timeline_ui(edit, ui);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_play_click_keeps_the_video_editor_open_without_creating_a_canvas() {
        let dir = tempfile::tempdir().unwrap();
        let frame = dir.path().join("frame.png");
        image::RgbaImage::new(16, 9).save(&frame).unwrap();
        let ctx = egui::Context::default();
        let mut video = Panel::default();
        let mut edit = VideoEdit::open(
            "clip",
            "Clip".into(),
            dir.path().join("clip.mp4"),
            Some(30.0),
            (1920.0, 1080.0),
            &Document::new(1920.0, 1080.0),
        );
        edit.set_frames(vec![frame], 1.0, 30.0, Some((16.0, 9.0)));
        video.edit = Some(edit);
        let mut settings = AppSettings::default();
        let render = |video: &mut Panel, settings: &mut AppSettings, events| {
            ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1200.0, 900.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    assert!(edit_window_ui(video, settings, ui).is_none());
                },
            )
        };
        render(&mut video, &mut settings, vec![]);
        let output = render(&mut video, &mut settings, vec![]);
        let pos = output
            .shapes
            .iter()
            .find_map(|shape| {
                if let egui::Shape::Text(text) = &shape.shape {
                    (text.galley.job.text == "Play").then(|| text.pos + text.galley.size() / 2.0)
                } else {
                    None
                }
            })
            .expect("visible Play button");
        for pressed in [true, false] {
            render(
                &mut video,
                &mut settings,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        pressed,
                        button: egui::PointerButton::Primary,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        assert!(video.edit.as_ref().unwrap().playing);
    }

    #[test]
    fn editor_content_fits_narrow_and_wide_windows() {
        for width in [320.0, 460.0, 940.0] {
            let ctx = egui::Context::default();
            let mut edit = VideoEdit::open(
                "test",
                "Test".into(),
                PathBuf::new(),
                Some(30.0),
                (1920.0, 1080.0),
                &Document::new(1920.0, 1080.0),
            );
            let mut settings = AppSettings::default();
            let mut measured = 0.0;
            let _ = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 800.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    body(&mut edit, &mut settings, ui);
                    measured = ui.min_rect().width();
                },
            );
            assert!(
                measured <= width,
                "content {measured} exceeds window {width}"
            );
        }
    }
}
