//! Pantalla de bienvenida cuando no hay ningún archivo abierto.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use eframe::egui;

use crate::app_icons::{
    draw_doc_icon, draw_folder_icon, draw_gear_icon, draw_pin_icon, draw_sparkle_icon,
    icon_text_button_ui,
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
            if show_pinned {
                ui.add_space(24.0);
                ui.label("Pinned");
                ui.add_space(8.0);
                for path in pinned {
                    let name = folder_display_name(path);
                    if let Some(a) = recent_folder_ui(ui, path, &name, true) {
                        action = Some(a);
                    }
                }
            }

            // ── Carpetas recientes (no ancladas) ──
            let folder_recents: Vec<_> = recents
                .iter()
                .filter(|p| p.is_dir() && !pinned.iter().any(|pin| pin == *p))
                .take(6)
                .cloned()
                .collect();
            if !folder_recents.is_empty() {
                ui.add_space(if show_pinned { 16.0 } else { 24.0 });
                ui.label("Recent folders");
                ui.add_space(8.0);
                for path in &folder_recents {
                    let name = folder_display_name(path);
                    if let Some(a) = recent_folder_ui(ui, path, &name, false) {
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

fn folder_display_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// Una fila clicable para una carpeta: icono de carpeta a la izquierda,
/// nombre centrado, botón de chincheta a la derecha (solo visible al
/// hover), fondo suave al hover, tooltip con la ruta completa.
/// La chincheta de la derecha es un botón independiente (no incluido
/// en el área del clic principal): así recibe sus propios clics y
/// su propio fondo al hover. Mismo ancho que los botones principales.
///
/// `is_pinned` controla si el icono de chincheta aparece lleno
/// (anclada) o vacío (no anclada).
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
    let pad_y = 6.0;
    let gap = 8.0;
    let pin_hit_area = 24.0; // área clicable generosa para la chincheta

    let galley = ui
        .painter()
        .layout_no_wrap(name.to_owned(), font.clone(), visuals.text_color());
    let height = (galley.size().y + pad_y * 2.0).max(28.0);
    let width = BUTTON_W;
    let body_w = width - pin_hit_area;

    // ── Layout horizontal: cuerpo + pin ──
    let row_rect = egui::Rect::from_min_size(
        ui.cursor().min,
        egui::vec2(width, height),
    );
    let (_row_rect, _row_resp) = ui.allocate_exact_size(row_rect.size(), egui::Sense::hover());

    // Fondo al hover de toda la fila
    let hovered = _row_resp.hovered();
    if hovered {
        ui.painter()
            .rect_filled(row_rect, 6.0, visuals.widgets.hovered.weak_bg_fill);
    }

    // ── Cuerpo (icono + nombre, clicable para abrir) ──
    let body_rect = egui::Rect::from_min_size(
        row_rect.left_top(),
        egui::vec2(body_w, height),
    );
    let body_resp = ui.interact(body_rect, ui.next_auto_id(), egui::Sense::click());
    let body_hovered = body_resp.hovered();
    let body_clicked = body_resp.clicked();

    let color = if body_hovered {
        visuals.widgets.active.text_color()
    } else {
        visuals.text_color()
    };

    // Icono de carpeta
    let left_pad = 14.0;
    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(body_rect.left() + left_pad + icon_sz / 2.0, body_rect.center().y),
        egui::vec2(icon_sz, icon_sz),
    );
    draw_folder_icon(ui.painter(), icon_rect, color);

    // Nombre
    let text_origin = egui::pos2(icon_rect.right() + gap, body_rect.center().y);
    ui.painter().text(
        text_origin,
        egui::Align2::LEFT_CENTER,
        name,
        font,
        color,
    );

    // ── Chincheta (widget propio con fondo al hover) ──
    let pin_area = egui::Rect::from_min_size(
        egui::pos2(row_rect.left() + body_w, row_rect.top()),
        egui::vec2(pin_hit_area, height),
    );
    let pin_resp = ui.interact(pin_area, ui.next_auto_id(), egui::Sense::click());
    let pin_hovered = pin_resp.hovered();
    let pin_clicked = pin_resp.clicked();

    let pin_center = egui::Rect::from_center_size(
        pin_area.center(),
        egui::vec2(pin_sz, pin_sz),
    );

    if pin_hovered {
        // Fondo suave propio
        ui.painter()
            .rect_filled(pin_area, 4.0, visuals.widgets.hovered.weak_bg_fill);
        draw_pin_icon(
            ui.painter(),
            pin_center,
            visuals.widgets.active.text_color(),
            is_pinned,
        );
    } else if is_pinned {
        // Siempre visible, apagado
        draw_pin_icon(
            ui.painter(),
            pin_center,
            visuals.widgets.inactive.text_color(),
            true,
        );
    } else if hovered {
        // Fila en hover pero no el pin: pin sutil
        draw_pin_icon(ui.painter(), pin_center, color, false);
    }

    if pin_hovered {
        let tip = if is_pinned { "Unpin" } else { "Pin" };
        pin_resp.on_hover_text(tip);
    }
    _row_resp.on_hover_text(path.display().to_string());

    // ── Menú contextual sobre el cuerpo ──
    let context_action = Rc::new(RefCell::new(None));
    let pin_label = if is_pinned { "Unpin" } else { "Pin" };
    let path_clone = path.to_owned();
    {
        let context_action = context_action.clone();
        body_resp.context_menu(move |ui| {
            if ui.button(pin_label).clicked() {
                *context_action.borrow_mut() = if is_pinned {
                    Some(WelcomeAction::UnpinRecent(path_clone.clone()))
                } else {
                    Some(WelcomeAction::PinRecent(path_clone.clone()))
                };
                ui.close();
            }
            if ui.button("Remove from recents").clicked() {
                *context_action.borrow_mut() =
                    Some(WelcomeAction::RemoveRecent(path_clone.clone()));
                ui.close();
            }
        });
    }

    if let Some(a) = context_action.take() {
        return Some(a);
    }
    // Clic en la chincheta → toggle
    if pin_clicked {
        if is_pinned {
            return Some(WelcomeAction::UnpinRecent(path.to_owned()));
        } else {
            return Some(WelcomeAction::PinRecent(path.to_owned()));
        }
    }
    if body_clicked {
        return Some(WelcomeAction::OpenRecent(path.to_owned()));
    }
    None
}