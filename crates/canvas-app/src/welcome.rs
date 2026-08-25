//! Pantalla de bienvenida cuando no hay ningún archivo abierto.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use eframe::egui;

use crate::app_icons::{
    draw_delete_icon, draw_doc_icon, draw_folder_icon, draw_gear_icon, draw_pin_icon,
    draw_sparkle_icon, icon_text_button_ui,
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

            // ── Carpetas ancladas (siempre visibles, primero) ──
            let show_pinned = !pinned.is_empty();
            let max_visible = 5;
            // Altura estimada por fila: 28 px de altura + ~2 px de espaciado
            // implícito de egui = redondeamos a 30.
            let row_h = 30.0;

            let all_recents: Vec<_> = recents
                .iter()
                .filter(|p| p.is_dir() && !pinned.iter().any(|pin| pin == *p))
                .take(12)
                .cloned()
                .collect();

            let total_items = pinned.len() + all_recents.len();
            if total_items > 0 {
                let scroll_h = (row_h * (total_items.min(max_visible)) as f32)
                    .max(row_h * 3.0);

                ui.add_space(24.0);
                ui.label("Recent folders");
                ui.add_space(8.0);

                egui::ScrollArea::vertical()
                    .id_salt("welcome_recents_scroll")
                    .max_height(scroll_h)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_min_width(BUTTON_W);
                        if show_pinned {
                            for path in pinned {
                                let name = folder_display_name(path);
                                if let Some(a) =
                                    recent_folder_ui(ui, path, &name, true)
                                {
                                    action = Some(a);
                                }
                            }
                            if !all_recents.is_empty() {
                                ui.add_space(4.0);
                            }
                        }
                        for path in &all_recents {
                            let name = folder_display_name(path);
                            if let Some(a) =
                                recent_folder_ui(ui, path, &name, false)
                            {
                                action = Some(a);
                            }
                        }
                    });
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

fn folder_display_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// Una fila clicable para una carpeta: icono de borrar a la izquierda,
/// icono de carpeta + nombre centrados, chincheta a la derecha. Los
/// iconos de borrar y chincheta solo se ven al hover (salvo pin fijo).
/// Cada zona de clic se decide por `hover_pos()` al hacer clic.
/// Una sola asignación `Sense::click()` para que `vertical_centered`
/// centre la fila correctamente.
fn recent_folder_ui(
    ui: &mut egui::Ui,
    path: &std::path::Path,
    name: &str,
    is_pinned: bool,
) -> Option<WelcomeAction> {
    let visuals = ui.visuals().clone();
    let font = egui::FontId::proportional(13.0);
    let icon_sz = 14.0;
    let pin_sz = 12.0;
    let trash_sz = 13.0;
    let pad_y = 6.0;
    let gap = 8.0;
    let side_area = 24.0; // ancho de cada zona de icono lateral

    let galley = ui
        .painter()
        .layout_no_wrap(name.to_owned(), font.clone(), visuals.text_color());
    let height = (galley.size().y + pad_y * 2.0).max(28.0);
    let width = BUTTON_W;

    let (row_rect, row_resp) =
        ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());
    let hovered = row_resp.hovered();
    let hover_pos = ui.input(|i| i.pointer.hover_pos());

    // Fondo al hover
    if hovered {
        ui.painter()
            .rect_filled(row_rect, 6.0, visuals.widgets.hovered.weak_bg_fill);
    }

    let color = if hovered {
        visuals.widgets.active.text_color()
    } else {
        visuals.text_color()
    };

    // ── Zona borrar (izquierda) ──
    let trash_area = egui::Rect::from_min_size(
        row_rect.left_top(),
        egui::vec2(side_area, height),
    );
    let over_trash = hovered && hover_pos.map_or(false, |p| trash_area.contains(p));
    if over_trash {
        ui.painter()
            .rect_filled(trash_area, 4.0, visuals.widgets.hovered.weak_bg_fill);
    }
    if hovered {
        let trash_c = if over_trash {
            visuals.widgets.active.text_color()
        } else {
            color
        };
        draw_delete_icon(
            ui.painter(),
            egui::Rect::from_center_size(trash_area.center(), egui::vec2(trash_sz, trash_sz)),
            trash_c,
        );
    }

    // ── Contenido centrado (icono carpeta + nombre) ──
    let content_w = icon_sz + gap + galley.size().x;
    let content_start_x = row_rect.left() + (width - content_w) / 2.0;
    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(content_start_x + icon_sz / 2.0, row_rect.center().y),
        egui::vec2(icon_sz, icon_sz),
    );
    draw_folder_icon(ui.painter(), icon_rect, color);
    let text_origin = egui::pos2(icon_rect.right() + gap, row_rect.center().y);
    ui.painter().text(text_origin, egui::Align2::LEFT_CENTER, name, font, color);

    // ── Zona chincheta (derecha) ──
    let pin_area = egui::Rect::from_min_size(
        egui::pos2(row_rect.right() - side_area, row_rect.top()),
        egui::vec2(side_area, height),
    );
    let over_pin = hover_pos.map_or(false, |p| pin_area.contains(p));

    if over_pin {
        ui.painter()
            .rect_filled(pin_area, 4.0, visuals.widgets.hovered.weak_bg_fill);
        draw_pin_icon(
            ui.painter(),
            egui::Rect::from_center_size(pin_area.center(), egui::vec2(pin_sz, pin_sz)),
            visuals.widgets.active.text_color(),
            is_pinned,
        );
    } else if is_pinned {
        draw_pin_icon(
            ui.painter(),
            egui::Rect::from_center_size(pin_area.center(), egui::vec2(pin_sz, pin_sz)),
            visuals.widgets.inactive.text_color(),
            true,
        );
    } else if hovered {
        draw_pin_icon(
            ui.painter(),
            egui::Rect::from_center_size(pin_area.center(), egui::vec2(pin_sz, pin_sz)),
            color,
            false,
        );
    }

    // ── Tooltip ──
    if over_trash {
        row_resp.clone().on_hover_text("Remove");
    } else if over_pin {
        let tip = if is_pinned { "Unpin" } else { "Pin" };
        row_resp.clone().on_hover_text(tip);
    } else {
        row_resp.clone().on_hover_text(path.display().to_string());
    }

    // ── Clic ──
    if row_resp.clicked() {
        if hover_pos.map_or(false, |p| trash_area.contains(p)) {
            return Some(WelcomeAction::RemoveRecent(path.to_owned()));
        }
        if hover_pos.map_or(false, |p| pin_area.contains(p)) {
            return if is_pinned {
                Some(WelcomeAction::UnpinRecent(path.to_owned()))
            } else {
                Some(WelcomeAction::PinRecent(path.to_owned()))
            };
        }
        return Some(WelcomeAction::OpenRecent(path.to_owned()));
    }

    // ── Menú contextual ──
    let context_action = Rc::new(RefCell::new(None));
    let pin_label = if is_pinned { "Unpin" } else { "Pin" };
    let path_clone = path.to_owned();
    {
        let context_action = context_action.clone();
        row_resp.context_menu(move |ui| {
            if ui.button(pin_label).clicked() {
                *context_action.borrow_mut() = if is_pinned {
                    Some(WelcomeAction::UnpinRecent(path_clone.clone()))
                } else {
                    Some(WelcomeAction::PinRecent(path_clone.clone()))
                };
                ui.close();
            }
            if ui.button("Remove").clicked() {
                *context_action.borrow_mut() =
                    Some(WelcomeAction::RemoveRecent(path_clone.clone()));
                ui.close();
            }
        });
    }

    if let Some(a) = context_action.take() {
        return Some(a);
    }
    None
}