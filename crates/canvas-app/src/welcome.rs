//! Pantalla de bienvenida cuando no hay ningún archivo abierto.

use std::path::PathBuf;

use eframe::egui;

use crate::app_icons::{
    draw_doc_icon, draw_folder_icon, draw_gear_icon, draw_sparkle_icon, icon_text_button_ui,
};

pub enum WelcomeAction {
    NewProject,
    OpenFile,
    OpenFolder,
    OpenSettings,
    OpenRecent(PathBuf),
}

pub fn show(ui: &mut egui::Ui, error: Option<&str>, recents: &[PathBuf]) -> Option<WelcomeAction> {
    let mut action = None;
    egui::CentralPanel::default().show(ui, |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() * 0.28);
            ui.heading(egui::RichText::new("Canvas Desktop").size(32.0));
            ui.add_space(6.0);
            ui.label("Edit images right on top of your files.");
            ui.add_space(24.0);

            if icon_text_button_ui(
                ui,
                true,
                |p, r, c| draw_sparkle_icon(p, r, c),
                "New design",
                None,
                egui::vec2(220.0, 36.0),
            )
            .clicked()
            {
                action = Some(WelcomeAction::NewProject);
            }
            ui.add_space(8.0);
            if icon_text_button_ui(
                ui,
                true,
                |p, r, c| draw_doc_icon(p, r, c),
                "Open file…",
                None,
                egui::vec2(220.0, 36.0),
            )
            .clicked()
            {
                action = Some(WelcomeAction::OpenFile);
            }
            ui.add_space(8.0);
            if icon_text_button_ui(
                ui,
                true,
                |p, r, c| draw_folder_icon(p, r, c),
                "Open folder…",
                None,
                egui::vec2(220.0, 36.0),
            )
            .clicked()
            {
                action = Some(WelcomeAction::OpenFolder);
            }

            let folder_recents: Vec<_> = recents
                .iter()
                .filter(|p| p.is_dir())
                .take(6)
                .cloned()
                .collect();
            if !folder_recents.is_empty() {
                ui.add_space(18.0);
                ui.label("Recent folders");
                for path in &folder_recents {
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| path.display().to_string());
                    if ui
                        .link(name)
                        .on_hover_text(path.display().to_string())
                        .clicked()
                    {
                        action = Some(WelcomeAction::OpenRecent(path.clone()));
                    }
                }
            }

            ui.add_space(18.0);
            ui.weak("You can also drag an image or a folder onto this window.");
            ui.add_space(8.0);
            if icon_text_button_ui(
                ui,
                true,
                |p, r, c| draw_gear_icon(p, r, c),
                "Settings",
                None,
                egui::Vec2::ZERO,
            )
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
