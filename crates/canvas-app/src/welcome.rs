//! Pantalla de bienvenida cuando no hay ningún archivo abierto.

use std::path::PathBuf;

use eframe::egui;

use crate::app_icons::{
    draw_doc_icon, draw_folder_icon, draw_gear_icon, draw_sparkle_icon, icon_text_button_ui,
};

const BUTTON_W: f32 = 220.0;
const BUTTON_H: f32 = 36.0;

pub enum WelcomeAction {
    NewProject,
    OpenFile,
    OpenFolder,
    OpenSettings,
    OpenRecent(PathBuf),
    RemoveRecent(PathBuf),
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
                egui::vec2(BUTTON_W, BUTTON_H),
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
                egui::vec2(BUTTON_W, BUTTON_H),
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
                egui::vec2(BUTTON_W, BUTTON_H),
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
                ui.add_space(24.0);
                ui.label("Recent folders");
                ui.add_space(8.0);
                for path in &folder_recents {
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| path.display().to_string());
                    let action_out = recent_folder_ui(ui, path, &name);
                    if let Some(a) = action_out {
                        action = Some(a);
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

/// Una fila clicable para una carpeta reciente: icono de carpeta a la
/// izquierda, nombre a la derecha, fondo suave al hover, esquinas
/// redondeadas, y tooltip con la ruta completa. Mismo ancho que los
/// botones principales para quedar alineado visualmente.
fn recent_folder_ui(
    ui: &mut egui::Ui,
    path: &std::path::Path,
    name: &str,
) -> Option<WelcomeAction> {
    let visuals = ui.visuals().clone();
    let font = egui::FontId::proportional(13.0);
    let icon_sz = 14.0;
    let pad_y = 6.0;
    let gap = 8.0;

    let galley = ui
        .painter()
        .layout_no_wrap(name.to_owned(), font.clone(), visuals.text_color());
    let height = (galley.size().y + pad_y * 2.0).max(28.0);
    let text_w = galley.size().x;
    let content_w = icon_sz + gap + text_w;
    let width = BUTTON_W;
    let rect = egui::Rect::from_min_size(
        ui.cursor().min,
        egui::vec2(width, height),
    );
    let (rect, mut resp) = ui.allocate_exact_size(rect.size(), egui::Sense::click());
    let hovered = resp.hovered();
    let clicked = resp.clicked();

    if hovered {
        ui.painter()
            .rect_filled(rect, 6.0, visuals.widgets.hovered.weak_bg_fill);
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    let color = if hovered {
        visuals.widgets.active.text_color()
    } else {
        visuals.text_color()
    };

    let start_x = rect.left() + (width - content_w) / 2.0;

    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(start_x + icon_sz / 2.0, rect.center().y),
        egui::vec2(icon_sz, icon_sz),
    );
    draw_folder_icon(ui.painter(), icon_rect, color);

    ui.painter().text(
        egui::pos2(start_x + icon_sz + gap, rect.center().y),
        egui::Align2::LEFT_CENTER,
        name,
        font,
        color,
    );

    resp = resp.on_hover_text(path.display().to_string());

    // ── Menú contextual: eliminar de recientes ──
    let mut context_action = None;
    resp.context_menu(|ui| {
        if ui.button("Remove from recents").clicked() {
            context_action = Some(WelcomeAction::RemoveRecent(path.to_owned()));
            ui.close();
        }
    });

    if let Some(a) = context_action {
        return Some(a);
    }
    if clicked {
        return Some(WelcomeAction::OpenRecent(path.to_owned()));
    }
    None
}
