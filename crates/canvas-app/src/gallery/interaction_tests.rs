use super::{GalleryAction, GalleryItem, ItemKind};
use eframe::egui;
use std::path::PathBuf;

fn frame(
    ctx: &egui::Context,
    time: f64,
    pressed: Option<bool>,
    selected: &mut Option<PathBuf>,
) -> Option<GalleryAction> {
    let pos = egui::pos2(60.0, 60.0);
    let mut events = vec![egui::Event::PointerMoved(pos)];
    if let Some(pressed) = pressed {
        events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
    }
    let mut action = None;
    let _ = ctx.run_ui(
        egui::RawInput {
            time: Some(time),
            events,
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(500.0, 400.0),
            )),
            ..Default::default()
        },
        |ui| {
            let item = GalleryItem {
                path: "photo.png".into(),
                name: "photo.png".into(),
                kind: ItemKind::Image,
                mtime: None,
                tex: None,
                failed: false,
            };
            egui::CentralPanel::default().show(ui, |ui| {
                action = super::ui::gallery_cell(
                    ui,
                    &item,
                    egui::vec2(180.0, 140.0),
                    selected,
                    &mut None,
                );
            });
        },
    );
    action
}

#[test]
fn single_click_selects_without_opening() {
    let ctx = egui::Context::default();
    let mut selected = None;
    frame(&ctx, 0.0, None, &mut selected);
    frame(&ctx, 0.1, Some(true), &mut selected);
    assert!(frame(&ctx, 0.12, Some(false), &mut selected).is_none());
    assert_eq!(selected, Some(PathBuf::from("photo.png")));
}

#[test]
fn double_click_opens_selected_design() {
    let ctx = egui::Context::default();
    let mut selected = None;
    frame(&ctx, 0.0, None, &mut selected);
    frame(&ctx, 0.1, Some(true), &mut selected);
    frame(&ctx, 0.12, Some(false), &mut selected);
    frame(&ctx, 0.2, Some(true), &mut selected);
    assert!(
        matches!(frame(&ctx, 0.22, Some(false), &mut selected), Some(GalleryAction::Open(path)) if path == PathBuf::from("photo.png"))
    );
}
