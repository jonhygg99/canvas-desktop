//! Miniaturas normales y encuadres verticales de una celda visible.
use crate::gallery::{framing::Card, GalleryItem, ItemKind};
use eframe::egui;

pub(super) fn paint(
    painter: &egui::Painter,
    item: &GalleryItem,
    thumb_rect: egui::Rect,
    card: Option<&Card>,
    vertical: bool,
    visuals: &egui::Visuals,
) {
    let portrait_rect = egui::Rect::from_center_size(
        thumb_rect.center(),
        egui::vec2(thumb_rect.height() * 9.0 / 16.0, thumb_rect.height()),
    );
    if vertical {
        if let Some(card) = card.filter(|card| card.saved.is_some()) {
            if let (Some(tex), Some(value)) = (&card.texture, card.saved) {
                crate::framing::preview::paint(
                    painter,
                    portrait_rect,
                    tex,
                    card.background.as_ref(),
                    value,
                );
            } else {
                painter.text(
                    thumb_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "Loading framing…",
                    egui::FontId::proportional(12.0),
                    visuals.weak_text_color(),
                );
            }
        } else {
            painter.text(
                thumb_rect.center(),
                egui::Align2::CENTER_CENTER,
                "No saved framing",
                egui::FontId::proportional(12.0),
                visuals.weak_text_color(),
            );
        }
    } else {
        match (&item.tex, item.failed) {
            (Some(tex), _) => {
                let size = tex.size_vec2();
                let scale = (thumb_rect.width() / size.x).max(thumb_rect.height() / size.y);
                let fitted = egui::Rect::from_center_size(thumb_rect.center(), size * scale);
                // Clip the cover thumbnail to the thumb rect: without
                // this, tall/panoramic photos bleed over neighbour cells.
                painter.rect_filled(thumb_rect, 2.0, visuals.extreme_bg_color);
                painter.with_clip_rect(thumb_rect).image(
                    tex.id(),
                    fitted,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }
            (None, _) if item.kind == ItemKind::Design => {
                painter.text(
                    thumb_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "Document",
                    egui::FontId::proportional(14.0),
                    visuals.weak_text_color(),
                );
            }
            (None, true) => {
                painter.text(
                    thumb_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "Failed to load",
                    egui::FontId::proportional(14.0),
                    visuals.error_fg_color,
                );
            }
            (None, false) => {
                painter.text(
                    thumb_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "Loading",
                    egui::FontId::proportional(14.0),
                    visuals.weak_text_color(),
                );
            }
        }
    }

    if item.kind == ItemKind::Design {
        painter.text(
            thumb_rect.right_top() + egui::vec2(-2.0, 2.0),
            egui::Align2::RIGHT_TOP,
            "Design",
            egui::FontId::proportional(11.0),
            visuals.weak_text_color(),
        );
    }
    if item.kind == ItemKind::Video {
        painter.text(
            thumb_rect.right_top() + egui::vec2(-2.0, 2.0),
            egui::Align2::RIGHT_TOP,
            "Video",
            egui::FontId::proportional(11.0),
            visuals.weak_text_color(),
        );
        // Triángulo de play sutil en el centro para distinguir video de imagen
        let play_sz = 22.0;
        let play_rect =
            egui::Rect::from_center_size(thumb_rect.center(), egui::vec2(play_sz, play_sz));
        let center = play_rect.center();
        let r = play_sz * 0.42;
        let p1 = egui::pos2(center.x - r * 0.45, center.y - r);
        let p2 = egui::pos2(center.x - r * 0.45, center.y + r);
        let p3 = egui::pos2(center.x + r * 0.75, center.y);
        painter.circle_filled(center, r + 9.0, egui::Color32::from_black_alpha(110));
        painter.add(egui::Shape::convex_polygon(
            vec![p1, p2, p3],
            egui::Color32::WHITE,
            egui::Stroke::NONE,
        ));
    }
}
