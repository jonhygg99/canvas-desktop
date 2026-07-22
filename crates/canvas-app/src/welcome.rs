//! Pantalla de bienvenida cuando no hay ningún archivo abierto.

use eframe::egui;

pub enum WelcomeAction {
    NewProject,
    OpenFile,
    OpenFolder,
    OpenSettings,
}

pub fn show(ui: &mut egui::Ui, error: Option<&str>) -> Option<WelcomeAction> {
    let mut action = None;
    egui::CentralPanel::default().show(ui, |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() * 0.28);
            ui.heading(egui::RichText::new("Canvas Desktop").size(32.0));
            ui.add_space(6.0);
            ui.label("Edit images right on top of your files.");
            ui.add_space(24.0);

            if ui
                .add(
                    egui::Button::new("✨  New design (1920 × 1080)")
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
