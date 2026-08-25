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
) -> (
    egui::Response,
    egui::InnerResponse<()>,
    Option<egui::InnerResponse<R>>,
) {
    // Cabecera con el título más grande (por defecto el CollapsingHeader usa
    // TextStyle::Button, ~13px): pintamos la flecha por defecto y un label
    // propio en negrita a 15px. La id se deriva del texto, igual que hace
    // `CollapsingHeader::new(text)` con `make_persistent_id`, así el estado
    // abierto/cerrado se conserva entre frames.
    let id = ui.make_persistent_id(text);
    egui::containers::collapsing_header::CollapsingState::load_with_default_open(
        ui.ctx(),
        id,
        default_open,
    )
    .show_header(ui, |ui| {
        ui.label(
            egui::RichText::new(text)
                .size(15.0)
                .strong()
                .color(ui.visuals().strong_text_color()),
        );
    })
    .body_unindented(add_contents)
}

/// Estira un slider hasta casi el borde del panel dejando hueco para el
/// display del valor (el `DragValue` que egui pinta a su derecha, ~56px).
///
/// NO se puede poner en `section`: el ancho correcto depende de cuánto
/// queda libre en la fila actual (aquí el `Slider` pinta pista + valor
/// SEGUIDOS, así que si la pista ocupa todo el ancho, el total desborda el
/// panel y egui agranda el `max_rect` del padre — lo que a su vez hace que
/// el siguiente slider sea aún más ancho, y así hasta ocupar toda la
/// pantalla). Se llama justo antes de `ui.add(egui::Slider...)`.
pub fn stretch_slider(ui: &mut egui::Ui) {
    ui.spacing_mut().slider_width = (ui.available_width() - 64.0).max(60.0);
}

#[cfg(test)]
mod tests {
    use super::PANEL_PAD;

    #[test]
    fn sidebar_padding_is_compact_but_positive() {
        const { assert!(PANEL_PAD > 0.0) };
        const { assert!(PANEL_PAD <= 8.0) };
    }
}
