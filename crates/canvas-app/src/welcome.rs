//! Pantalla de bienvenida cuando no hay ningún archivo abierto.

use std::path::PathBuf;

use eframe::egui;

pub enum WelcomeAction {
    NewProject,
    OpenFile,
    OpenFolder,
    OpenSettings,
    OpenRecent(PathBuf),
}

pub fn show(
    ui: &mut egui::Ui,
    error: Option<&str>,
    recents: &[PathBuf],
    page_size: (f64, f64),
) -> Option<WelcomeAction> {
    let mut action = None;
    egui::CentralPanel::default().show(ui, |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() * 0.28);
            ui.heading(egui::RichText::new("Canvas Desktop").size(32.0));
            ui.add_space(6.0);
            ui.label("Edit images right on top of your files.");
            ui.add_space(24.0);

            let (w, h) = page_size;
            if ui
                .add(
                    egui::Button::new(format!("✨  New design ({} × {})", w as i64, h as i64))
                        .min_size(egui::vec2(220.0, 36.0)),
                )
                .clicked()
            {
                action = Some(WelcomeAction::NewProject);
            }
            ui.add_space(8.0);
            if ui
                .add(egui::Button::new("📄  Open file…").min_size(egui::vec2(220.0, 36.0)))
                .clicked()
            {
                action = Some(WelcomeAction::OpenFile);
            }
            ui.add_space(8.0);
            if ui
                .add(egui::Button::new("📁  Open folder…").min_size(egui::vec2(220.0, 36.0)))
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
                        .link(format!("📁 {name}"))
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
            if ui.small_button("⚙ Settings").clicked() {
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
