//! Filas interactivas de carpetas recientes y ancladas.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use eframe::egui;

use crate::app_icons::{draw_delete_icon, draw_folder_icon, draw_pin_icon};

use super::{WelcomeAction, BUTTON_W};

pub(super) fn folder_display_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

pub(super) fn recent_folder_ui(
    ui: &mut egui::Ui,
    path: &Path,
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
    let side_area = 24.0;
    let galley = ui
        .painter()
        .layout_no_wrap(name.to_owned(), font.clone(), visuals.text_color());
    let height = (galley.size().y + pad_y * 2.0).max(28.0);
    let (row_rect, row_resp) =
        ui.allocate_exact_size(egui::vec2(BUTTON_W, height), egui::Sense::click());
    let hovered = row_resp.hovered();
    let hover_pos = ui.input(|i| i.pointer.hover_pos());
    if hovered {
        ui.painter()
            .rect_filled(row_rect, 6.0, visuals.widgets.hovered.weak_bg_fill);
    }
    let color = if hovered {
        visuals.widgets.active.text_color()
    } else {
        visuals.text_color()
    };
    let trash_area = egui::Rect::from_min_size(row_rect.left_top(), egui::vec2(side_area, height));
    let over_trash = hovered && hover_pos.is_some_and(|p| trash_area.contains(p));
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
    let content_w = icon_sz + gap + galley.size().x;
    let content_start_x = row_rect.left() + (BUTTON_W - content_w) / 2.0;
    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(content_start_x + icon_sz / 2.0, row_rect.center().y),
        egui::vec2(icon_sz, icon_sz),
    );
    draw_folder_icon(ui.painter(), icon_rect, color);
    ui.painter().text(
        egui::pos2(icon_rect.right() + gap, row_rect.center().y),
        egui::Align2::LEFT_CENTER,
        name,
        font,
        color,
    );
    let pin_area = egui::Rect::from_min_size(
        egui::pos2(row_rect.right() - side_area, row_rect.top()),
        egui::vec2(side_area, height),
    );
    let over_pin = hover_pos.is_some_and(|p| pin_area.contains(p));
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
    if over_trash {
        row_resp.clone().on_hover_text("Remove");
    } else if over_pin {
        row_resp
            .clone()
            .on_hover_text(if is_pinned { "Unpin" } else { "Pin" });
    } else {
        row_resp.clone().on_hover_text(path.display().to_string());
    }
    if row_resp.clicked() {
        if over_trash {
            return Some(WelcomeAction::RemoveRecent(path.to_owned()));
        }
        if over_pin {
            return Some(if is_pinned {
                WelcomeAction::UnpinRecent(path.to_owned())
            } else {
                WelcomeAction::PinRecent(path.to_owned())
            });
        }
        return Some(WelcomeAction::OpenRecent(path.to_owned()));
    }
    let context_action = Rc::new(RefCell::new(None));
    let pin_label = if is_pinned { "Unpin" } else { "Pin" };
    let path_clone = path.to_owned();
    {
        let context_action = context_action.clone();
        row_resp.context_menu(move |ui| {
            if ui.button(pin_label).clicked() {
                *context_action.borrow_mut() = Some(if is_pinned {
                    WelcomeAction::UnpinRecent(path_clone.clone())
                } else {
                    WelcomeAction::PinRecent(path_clone.clone())
                });
                ui.close();
            }
            if ui.button("Remove").clicked() {
                *context_action.borrow_mut() =
                    Some(WelcomeAction::RemoveRecent(path_clone.clone()));
                ui.close();
            }
        });
    }
    context_action.take()
}
