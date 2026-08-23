//! Métricas y piezas visuales compartidas por los paneles laterales del editor.

use eframe::egui;

pub const PANEL_PAD: f32 = 8.0;
pub fn compact(ui: &mut egui::Ui) {
    ui.spacing_mut().item_spacing = egui::vec2(4.0, 3.0);
    ui.spacing_mut().button_padding = egui::vec2(5.0, 2.0);
    ui.spacing_mut().interact_size.y = 22.0;
}

pub fn title(ui: &mut egui::Ui, text: &str) {
    let width = ui.available_width().max(1.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 24.0), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 4.0, ui.visuals().faint_bg_color);
    painter.text(
        rect.left_center() + egui::vec2(PANEL_PAD, 0.0),
        egui::Align2::LEFT_CENTER,
        text,
        egui::FontId::proportional(13.0),
        ui.visuals().strong_text_color(),
    );
}

pub fn section<R>(
    ui: &mut egui::Ui,
    text: &str,
    default_open: bool,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::containers::collapsing_header::CollapsingResponse<R> {
    egui::CollapsingHeader::new(text)
        .default_open(default_open)
        .show(ui, add_contents)
}

#[cfg(test)]
mod tests {
    use super::PANEL_PAD;

    #[test]
    fn sidebar_padding_is_compact_but_positive() {
        assert!(PANEL_PAD > 0.0);
        assert!(PANEL_PAD <= 8.0);
    }
}
