//! Fondo desenfocado y frente: mismo cover/offset que el consumidor.
use super::Session;
use canvas_core::framing::{Framing, HEIGHT, WIDTH};
use eframe::egui;

pub fn background(source: &canvas_io::LoadedImage) -> Result<egui::ColorImage, String> {
    let img = image::RgbaImage::from_raw(source.width, source.height, source.rgba.clone())
        .ok_or("Invalid framing image dimensions")?;
    let resized = image::DynamicImage::ImageRgba8(img)
        .resize_to_fill(360, 640, image::imageops::FilterType::Triangle)
        .to_rgba8();
    let blurred = image::imageops::blur(&resized, 20.0 / 3.0);
    Ok(egui::ColorImage::from_rgba_unmultiplied(
        [360, 640],
        blurred.as_raw(),
    ))
}

pub fn paint(
    painter: &egui::Painter,
    rect: egui::Rect,
    texture: &egui::TextureHandle,
    background: Option<&egui::TextureHandle>,
    framing: Framing,
) {
    let uv = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0));
    let painter = painter.with_clip_rect(rect);
    painter.rect_filled(rect, 0.0, egui::Color32::BLACK);
    if let Some(bg) = background {
        painter.image(bg.id(), rect, uv, egui::Color32::WHITE);
    }
    if let Some(p) = framing.placement(texture.size()[0] as f64, texture.size()[1] as f64) {
        let scale = rect.width() / WIDTH as f32;
        let fg = egui::Rect::from_min_size(
            rect.min + egui::vec2(p.x as f32, p.y as f32) * scale,
            egui::vec2(p.width as f32, p.height as f32) * scale,
        );
        painter.image(texture.id(), fg, uv, egui::Color32::WHITE);
    }
}

pub(super) fn show(session: &mut Session, ui: &mut egui::Ui) {
    session.poll(ui.ctx());
    let Some(texture) = &session.texture else {
        if session.busy() {
            ui.spinner();
        }
        if let Some(error) = &session.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        return;
    };
    let available = ui.available_size();
    let height = available.y.min(available.x * 16.0 / 9.0).max(1.0);
    let size = egui::vec2(height * 9.0 / 16.0, height);
    let (area, _) = ui.allocate_exact_size(available, egui::Sense::hover());
    let rect = egui::Rect::from_center_size(area.center(), size);
    let response = ui.interact(rect, ui.id().with("framing-drag"), egui::Sense::drag());
    paint(
        ui.painter(),
        rect,
        texture,
        session.background.as_ref(),
        session.value,
    );
    if session.show_guides {
        let safe = guide_rect(rect);
        ui.painter().rect_stroke(
            safe,
            0.0,
            egui::Stroke::new(1.5, egui::Color32::WHITE),
            egui::StrokeKind::Inside,
        );
        ui.painter().line_segment(
            [
                egui::pos2(rect.center().x, safe.top()),
                egui::pos2(rect.center().x, safe.bottom()),
            ],
            egui::Stroke::new(1.0, egui::Color32::WHITE),
        );
    }
    ui.painter().rect_stroke(
        rect,
        0.0,
        egui::Stroke::new(1.0, ui.visuals().text_color()),
        egui::StrokeKind::Inside,
    );
    if session.busy() {
        return;
    }
    if response.drag_started() {
        session.gesture = Some(session.value);
    }
    if response.dragged() {
        // Absoluto desde el press: idempotente si egui repite el pase de layout.
        let delta = response.total_drag_delta().unwrap_or_default();
        if let (Some(before), Some(p)) = (
            session.gesture,
            session
                .value
                .placement(texture.size()[0] as f64, texture.size()[1] as f64),
        ) {
            let sign_x = if p.width > f64::from(WIDTH) {
                -1.0
            } else {
                1.0
            };
            let sign_y = if p.height > f64::from(HEIGHT) {
                -1.0
            } else {
                1.0
            };
            session.value.x_pct =
                (before.x_pct + sign_x * delta.x / rect.width() * 100.0).clamp(-100.0, 100.0);
            session.value.y_pct =
                (before.y_pct + sign_y * delta.y / rect.height() * 100.0).clamp(-100.0, 100.0);
        }
    }
    if response.drag_stopped() {
        if let Some(before) = session.gesture.take() {
            session.commit(before);
        }
    }
}

fn guide_rect(rect: egui::Rect) -> egui::Rect {
    rect.shrink2(rect.size() * 0.1)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn guides_preserve_center_and_inset_each_edge() {
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(200.0, 400.0));
        let guide = guide_rect(rect);
        assert_eq!(guide.center(), rect.center());
        assert_eq!(guide.min, egui::pos2(20.0, 40.0));
        assert_eq!(guide.max, egui::pos2(180.0, 360.0));
    }
}
