//! Pantalla de bienvenida cuando no hay ningún archivo abierto.

use std::path::PathBuf;

use eframe::egui;

use crate::app_icons::{
    draw_doc_icon, draw_folder_icon, draw_gear_icon, draw_sparkle_icon, icon_text_button_ui,
};

mod rows;
use rows::{folder_display_name, recent_folder_ui};

const BUTTON_W: f32 = 220.0;
const BUTTON_H: f32 = 36.0;

pub enum WelcomeAction {
    NewProject,
    OpenFile,
    OpenFolder,
    OpenSettings,
    OpenRecent(PathBuf),
    RemoveRecent(PathBuf),
    PinRecent(PathBuf),
    UnpinRecent(PathBuf),
}

pub fn show(
    ui: &mut egui::Ui,
    error: Option<&str>,
    recents: &[PathBuf],
    pinned: &[PathBuf],
) -> Option<WelcomeAction> {
    let mut action = None;
    egui::CentralPanel::default().show(ui, |ui| {
        ui.vertical_centered(|ui| {
            draw_welcome_actions(ui, &mut action);
            draw_recent_folders(ui, recents, pinned, &mut action);
            ui.add_space(18.0);
            ui.weak("You can also drag an image or a folder onto this window.");
            ui.add_space(8.0);
            if icon_text_button_ui(ui, true, draw_gear_icon, "Settings", None, egui::Vec2::ZERO)
                .clicked()
            {
                action = Some(WelcomeAction::OpenSettings);
            }
            if let Some(error) = error {
                ui.add_space(18.0);
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
        });
    });
    action
}

fn draw_welcome_actions(ui: &mut egui::Ui, action: &mut Option<WelcomeAction>) {
    ui.add_space(ui.available_height() * 0.28);
    ui.heading(egui::RichText::new("Canvas Desktop").size(32.0));
    ui.add_space(6.0);
    ui.label("Edit images right on top of your files.");
    ui.add_space(24.0);
    if icon_text_button_ui(
        ui,
        true,
        draw_sparkle_icon,
        "New design",
        None,
        egui::vec2(BUTTON_W, BUTTON_H),
    )
    .clicked()
    {
        *action = Some(WelcomeAction::NewProject);
    }
    ui.add_space(8.0);
    if icon_text_button_ui(
        ui,
        true,
        draw_doc_icon,
        "Open file…",
        None,
        egui::vec2(BUTTON_W, BUTTON_H),
    )
    .clicked()
    {
        *action = Some(WelcomeAction::OpenFile);
    }
    ui.add_space(8.0);
    if icon_text_button_ui(
        ui,
        true,
        draw_folder_icon,
        "Open folder…",
        None,
        egui::vec2(BUTTON_W, BUTTON_H),
    )
    .clicked()
    {
        *action = Some(WelcomeAction::OpenFolder);
    }
    ui.add_space(8.0);
}

fn draw_recent_folders(
    ui: &mut egui::Ui,
    recents: &[PathBuf],
    pinned: &[PathBuf],
    action: &mut Option<WelcomeAction>,
) {
    let all_recents: Vec<_> = recents
        .iter()
        .filter(|p| p.is_dir() && !pinned.iter().any(|pin| pin == *p))
        .take(12)
        .cloned()
        .collect();
    let total_items = pinned.len() + all_recents.len();
    if total_items == 0 {
        return;
    }
    let scroll_h = (30.0 * total_items.min(5) as f32).max(90.0);
    let scroll_w = BUTTON_W + 18.0;
    ui.add_space(24.0);
    ui.label("Recent folders");
    ui.add_space(8.0);
    let ox = ((ui.available_width() - scroll_w) / 2.0).max(0.0);
    let rect = egui::Rect::from_min_size(
        ui.cursor().min + egui::vec2(ox, 0.0),
        egui::vec2(scroll_w, scroll_h),
    );
    let _ = ui.allocate_rect(rect, egui::Sense::hover());
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        egui::ScrollArea::vertical()
            .id_salt("welcome_recents_scroll")
            .max_height(scroll_h)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_min_width(BUTTON_W);
                draw_recent_rows(ui, pinned, &all_recents, action);
            });
    });
}

fn draw_recent_rows(
    ui: &mut egui::Ui,
    pinned: &[PathBuf],
    recents: &[PathBuf],
    action: &mut Option<WelcomeAction>,
) {
    for path in pinned {
        if let Some(a) = recent_folder_ui(ui, path, &folder_display_name(path), true) {
            *action = Some(a);
        }
    }
    if !pinned.is_empty() && !recents.is_empty() {
        ui.add_space(4.0);
    }
    for path in recents {
        if let Some(a) = recent_folder_ui(ui, path, &folder_display_name(path), false) {
            *action = Some(a);
        }
    }
}
