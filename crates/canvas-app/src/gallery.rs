//! Galería de carpeta: cuadrícula de miniaturas con nombre. Las miniaturas
//! llegan por mensajes desde los hilos de trabajo; aquí solo se pintan.

use std::path::PathBuf;

use eframe::egui;

pub struct GalleryItem {
    pub path: PathBuf,
    pub name: String,
    pub tex: Option<egui::TextureHandle>,
    pub failed: bool,
}

pub struct GalleryState {
    pub folder: PathBuf,
    pub items: Vec<GalleryItem>,
    pub scanned: bool,
}

impl GalleryState {
    pub fn new(folder: PathBuf) -> Self {
        Self {
            folder,
            items: Vec::new(),
            scanned: false,
        }
    }

    pub fn set_files(&mut self, files: Vec<PathBuf>) {
        self.items = files
            .into_iter()
            .map(|path| GalleryItem {
                name: path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                path,
                tex: None,
                failed: false,
            })
            .collect();
        self.scanned = true;
    }
}

pub enum GalleryAction {
    Open(PathBuf),
}

const CELL: egui::Vec2 = egui::vec2(172.0, 200.0);
const THUMB: f32 = 156.0;

pub fn show(state: &mut GalleryState, ui: &mut egui::Ui) -> Option<GalleryAction> {
    let mut action = None;
    egui::CentralPanel::default().show(ui, |ui| {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.heading(
                state
                    .folder
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| state.folder.display().to_string()),
            );
            ui.weak(format!("— {} imágenes", state.items.len()));
        });
        ui.add_space(4.0);
        ui.separator();

        if !state.scanned {
            ui.vertical_centered(|ui| {
                ui.add_space(40.0);
                ui.add(egui::Spinner::new().size(28.0));
                ui.label("Buscando imágenes…");
            });
            return;
        }
        if state.items.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(40.0);
                ui.label("Esta carpeta no contiene imágenes.");
            });
            return;
        }

        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                for item in &state.items {
                    if let Some(open) = gallery_cell(ui, item) {
                        action = Some(open);
                    }
                }
            });
        });
    });
    action
}

fn gallery_cell(ui: &mut egui::Ui, item: &GalleryItem) -> Option<GalleryAction> {
    let (rect, response) = ui.allocate_exact_size(CELL, egui::Sense::click());
    if !ui.is_rect_visible(rect) {
        // Fuera del scroll: no pintamos nada (la cuadrícula sigue fluida
        // aunque haya cientos de imágenes).
        return None;
    }
    let painter = ui.painter();

    if response.hovered() {
        painter.rect_filled(rect, 6.0, ui.visuals().widgets.hovered.weak_bg_fill);
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    let thumb_rect = egui::Rect::from_center_size(
        egui::pos2(rect.center().x, rect.top() + 8.0 + THUMB / 2.0),
        egui::Vec2::splat(THUMB),
    );
    match (&item.tex, item.failed) {
        (Some(tex), _) => {
            // Encaja la miniatura en su celda conservando la proporción.
            let size = tex.size_vec2();
            let scale = (THUMB / size.x).min(THUMB / size.y).min(1.0);
            let fitted = egui::Rect::from_center_size(thumb_rect.center(), size * scale);
            painter.image(
                tex.id(),
                fitted,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
        (None, true) => {
            painter.text(
                thumb_rect.center(),
                egui::Align2::CENTER_CENTER,
                "⚠",
                egui::FontId::proportional(28.0),
                ui.visuals().error_fg_color,
            );
        }
        (None, false) => {
            painter.text(
                thumb_rect.center(),
                egui::Align2::CENTER_CENTER,
                "⏳",
                egui::FontId::proportional(24.0),
                ui.visuals().weak_text_color(),
            );
        }
    }

    // Nombre truncado bajo la miniatura.
    let name_pos = egui::pos2(rect.center().x, rect.bottom() - 18.0);
    let font = egui::FontId::proportional(12.5);
    let mut name = item.name.clone();
    if name.chars().count() > 24 {
        name = format!("{}…", name.chars().take(23).collect::<String>());
    }
    painter.text(
        name_pos,
        egui::Align2::CENTER_CENTER,
        name,
        font,
        ui.visuals().text_color(),
    );

    response
        .clicked()
        .then(|| GalleryAction::Open(item.path.clone()))
}
