//! Ventana adaptable de edicion de video.
use super::*;

pub fn edit_window_ui(
    video: &mut Panel,
    settings: &mut AppSettings,
    ui: &mut egui::Ui,
) -> Option<Vec<VideoAccept>> {
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
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let ready =
                    !edit.loading_frames && edit.frames_error.is_none() && !edit.frames.is_empty();
                let count = edit.trim_ranges().len();
                let label = if count == 1 {
                    "Create canvas".to_owned()
                } else {
                    format!("Create {count} canvases")
                };
                let btn = ui.add_enabled(ready, egui::Button::new(label));
                if btn.clicked() {
                    accept = Some(edit.build_accepts());
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
        let mut image_height = 0.0;
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(width, 0.0),
                egui::Layout::top_down(egui::Align::Center),
                |ui| {
                    ui.set_width(width);
                    image_height = preview::preview_size(edit, ui).y;
                    preview_ui(edit, ui);
                    transport_ui(edit, ui, settings);
                },
            );
            ui.allocate_ui_with_layout(
                egui::vec2(320.0, image_height),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(320.0);
                    egui::ScrollArea::vertical()
                        .id_salt("video-settings-scroll")
                        .max_height(image_height)
                        .auto_shrink([false, false])
                        .show(ui, |ui| params_ui(edit, ui));
                },
            );
        });
    } else {
        ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
            preview_ui(edit, ui)
        });
        transport_ui(edit, ui, settings);
        params_ui(edit, ui);
    }
    ui.separator();
    timeline_ui(edit, ui);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn individual_reset_icons_restore_appearance_and_trim() {
        let mut edit = VideoEdit::open(
            "clip",
            "Clip".into(),
            PathBuf::new(),
            Some(30.0),
            (1920.0, 1080.0),
            &Document::new(1920.0, 1080.0),
        );
        edit.zoom = 2.0;
        edit.position = (120.0, -45.0);
        edit.blur = 40.0;
        edit.trim_start = 2.0;
        edit.trim_end = 9.0;
        let ctx = egui::Context::default();
        let render = |edit: &mut VideoEdit, events| {
            ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(400.0, 900.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| params_ui(edit, ui),
            )
        };
        for index in 0..7 {
            if index == 6 {
                edit.trim_start = 2.0;
                edit.trim_end = 9.0;
            }
            let output = render(&mut edit, vec![]);
            let icons: Vec<_> = output
                .shapes
                .iter()
                .filter_map(|shape| {
                    if let egui::Shape::Path(path) = &shape.shape {
                        (path.points.len() == 25)
                            .then(|| egui::Rect::from_points(&path.points).center())
                    } else {
                        None
                    }
                })
                .collect();
            assert_eq!(icons.len(), 7);
            let pos = icons[index];
            for pressed in [true, false] {
                render(
                    &mut edit,
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
            match index {
                0 => assert_eq!(edit.zoom, 1.0),
                1 => assert_eq!(edit.position.0, 0.0),
                2 => assert_eq!(edit.position.1, 0.0),
                3 => assert_eq!(edit.blur, 100.0),
                4 => assert_eq!((edit.trim_start, edit.trim_end), (0.0, 9.0)),
                5 => assert_eq!(edit.trim_end, 30.0),
                6 => assert_eq!((edit.trim_start, edit.trim_end), (2.0, 30.0)),
                _ => unreachable!(),
            }
        }
    }

    #[test]
    fn vertical_canvas_is_centered_in_the_preview_column() {
        let mut edit = VideoEdit::open(
            "clip",
            "Clip".into(),
            PathBuf::new(),
            Some(30.0),
            (1080.0, 1920.0),
            &Document::new(1080.0, 1920.0),
        );
        let mut settings = AppSettings {
            ytdlp_canvas_size: (1080.0, 1920.0),
            ..Default::default()
        };
        let ctx = egui::Context::default();
        let mut expected_center = 0.0;
        let output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1040.0, 900.0),
                )),
                ..Default::default()
            },
            |ui| {
                expected_center = ui.max_rect().left() + (ui.available_width() - 340.0) / 2.0;
                body(&mut edit, &mut settings, ui);
            },
        );
        let preview = output
            .shapes
            .iter()
            .find_map(|shape| {
                if let egui::Shape::Rect(rect) = &shape.shape {
                    (rect.fill == egui::Color32::WHITE && rect.rect.height() > rect.rect.width())
                        .then_some(rect.rect)
                } else {
                    None
                }
            })
            .expect("vertical preview");
        assert!((preview.center().x - expected_center).abs() < 0.01);
    }

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
        let text_pos = |label| {
            output
                .shapes
                .iter()
                .find_map(|shape| {
                    if let egui::Shape::Text(text) = &shape.shape {
                        (text.galley.job.text == label).then_some(text.pos)
                    } else {
                        None
                    }
                })
                .unwrap()
        };
        assert!(text_pos("Create canvas").x > text_pos("Cancel").x);
        let pos = output
            .shapes
            .iter()
            .find_map(|shape| {
                if let egui::Shape::Path(path) = &shape.shape {
                    let rect = egui::Rect::from_points(&path.points);
                    (path.points.len() == 3
                        && (rect.width() - 14.0).abs() < 0.01
                        && (rect.height() - 16.0).abs() < 0.01)
                        .then(|| rect.center())
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
            edit.position = (1234.0, -567.0);
            let mut measured = 0.0;
            let output = ctx.run_ui(
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
            let label_y = |label| {
                output
                    .shapes
                    .iter()
                    .find_map(|shape| {
                        if let egui::Shape::Text(text) = &shape.shape {
                            (text.galley.job.text == label).then_some(text.pos.y)
                        } else {
                            None
                        }
                    })
                    .unwrap()
            };
            assert!(label_y("Zoom") < label_y("Background blur"));
            assert!(label_y("Background blur") < label_y("Trim"));
        }
    }
}
