//! Miniaturas cacheadas por capa; cambiar el bitmap invalida la caché.
use crate::editor::EditorState;
use canvas_core::{LayerContent, LayerId, ShapeKind};
use eframe::egui;

pub(super) fn show(state: &EditorState, ui: &mut egui::Ui, id: LayerId) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, 3.0, ui.visuals().extreme_bg_color);
    if let Some(image) = state.images.get(&id) {
        let key = egui::Id::new(("layer_thumbnail", state.active_slot_id, id.raw()));
        let cached = ui.data(|d| d.get_temp::<(u64, egui::TextureHandle)>(key));
        let texture = match cached {
            Some((blob, texture)) if blob == image.data.id() => texture,
            _ => {
                let Some(thumb) = thumbnail(image) else {
                    return;
                };
                let texture =
                    ui.ctx()
                        .load_texture("layer-thumbnail", thumb, egui::TextureOptions::LINEAR);
                ui.data_mut(|d| d.insert_temp(key, (image.data.id(), texture.clone())));
                texture
            }
        };
        let ratio = image.width as f32 / image.height.max(1) as f32;
        let size = if ratio > 1.0 {
            egui::vec2(22.0, 22.0 / ratio)
        } else {
            egui::vec2(22.0 * ratio, 22.0)
        };
        ui.painter().image(
            texture.id(),
            egui::Rect::from_center_size(rect.center(), size),
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    } else if let Ok(layer) = state.doc.layer(id) {
        match &layer.content {
            LayerContent::Shape(shape) => shape_preview(ui.painter(), rect.shrink(3.0), shape),
            LayerContent::Text(_) => {
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "Aa",
                    egui::FontId::proportional(13.0),
                    ui.visuals().text_color(),
                );
            }
            _ => {
                ui.painter().rect_stroke(
                    rect.shrink(4.0),
                    2.0,
                    egui::Stroke::new(1.0, ui.visuals().text_color()),
                    egui::StrokeKind::Inside,
                );
            }
        }
    }
}
fn thumbnail(image: &vello::peniko::ImageData) -> Option<egui::ColorImage> {
    if image.width == 0
        || image.height == 0
        || image.data.data().len() < image.width as usize * image.height as usize * 4
    {
        return None;
    }
    let mut rgba = Vec::with_capacity(48 * 48 * 4);
    for y in 0..48 {
        for x in 0..48 {
            let offset = ((y * image.height / 48) as usize * image.width as usize
                + (x * image.width / 48) as usize)
                * 4;
            rgba.extend_from_slice(&image.data.data()[offset..offset + 4]);
        }
    }
    Some(egui::ColorImage::from_rgba_unmultiplied([48, 48], &rgba))
}

fn shape_preview(painter: &egui::Painter, rect: egui::Rect, shape: &canvas_core::ShapeContent) {
    let color =
        |rgba: [u8; 4]| egui::Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]);
    let fill = color(shape.fill);
    let stroke = egui::Stroke::new(shape.stroke_width.clamp(0.0, 2.0), color(shape.stroke));
    match shape.kind {
        ShapeKind::Rect => {
            painter.rect(
                rect,
                if shape.corner_radius > 0.0 { 2.0 } else { 0.0 },
                fill,
                stroke,
                egui::StrokeKind::Inside,
            );
        }
        ShapeKind::Ellipse => {
            painter.circle(rect.center(), rect.width() / 2.0, fill, stroke);
        }
        ShapeKind::Line => {
            painter.line_segment([rect.left_center(), rect.right_center()], stroke);
        }
        ShapeKind::Arrow => {
            let shaft_end = egui::pos2(
                rect.left() + canvas_core::arrow_shaft_end_x(f64::from(rect.width())) as f32,
                rect.center().y,
            );
            painter.line_segment(
                [rect.left_center(), shaft_end],
                egui::Stroke::new(3.0, fill),
            );
            let points =
                canvas_core::arrow_head_points(f64::from(rect.width()), f64::from(rect.height()));
            painter.add(egui::Shape::convex_polygon(
                points
                    .into_iter()
                    .map(|(x, y)| rect.min + egui::vec2(x as f32, y as f32))
                    .collect(),
                fill,
                stroke,
            ));
        }
        kind => polygon_preview(painter, rect, kind, fill, stroke),
    }
}
fn polygon_preview(
    painter: &egui::Painter,
    rect: egui::Rect,
    kind: ShapeKind,
    fill: egui::Color32,
    stroke: egui::Stroke,
) {
    let (w, h) = (f64::from(rect.width()), f64::from(rect.height()));
    let points = match kind {
        ShapeKind::Triangle => canvas_core::triangle_points(w, h).to_vec(),
        ShapeKind::Star => canvas_core::star_points(w, h, 5, 0.45),
        ShapeKind::Pentagon => canvas_core::regular_polygon_points(w, h, 5),
        ShapeKind::Hexagon => canvas_core::regular_polygon_points(w, h, 6),
        ShapeKind::Diamond => canvas_core::diamond_points(w, h).to_vec(),
        ShapeKind::Cross => canvas_core::cross_points(w, h).to_vec(),
        ShapeKind::Heart => canvas_core::heart_points(w, h, 32),
        _ => unreachable!(),
    };
    let points: Vec<_> = points
        .into_iter()
        .map(|(x, y)| rect.min + egui::vec2(x as f32, y as f32))
        .collect();
    // Estas siluetas admiten un abanico desde su centro, incluso las cóncavas.
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.center(), fill);
    for point in &points {
        mesh.colored_vertex(*point, fill);
    }
    for i in 0..points.len() as u32 {
        mesh.add_triangle(0, i + 1, (i + 1) % points.len() as u32 + 1);
    }
    painter.add(egui::Shape::mesh(mesh));
    painter.add(egui::Shape::closed_line(points, stroke));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn thumbnails_handle_small_images_and_invalid_buffers() {
        let image = canvas_render::image_data_from_rgba(vec![255, 0, 0, 255], 1, 1);
        let thumb = thumbnail(&image).unwrap();
        assert!(thumb.pixels.iter().all(|c| *c == egui::Color32::RED));
        let bad = canvas_render::image_data_from_rgba(vec![], 1, 1);
        assert!(thumbnail(&bad).is_none());
    }
}
