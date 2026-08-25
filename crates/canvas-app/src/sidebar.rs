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
    // El `CollapsingHeader` por defecto hace clicable TODA la fila (flecha +
    // texto) con `Sense::click()` sobre el rect completo — usar un título
    // personalizado con `show_header` dejaría el texto sin reacción. Por eso
    // pasamos el título como `RichText` al propio `CollapsingHeader`, que
    // conserva el clic en toda la fila y pinta el texto con la fuente que le
    // damos.
    //
    // La negrita real necesita la familia "Ubuntu-Bold" registrada en
    // `App::new` (egui 0.35 no trae negrita y `RichText::strong()` solo
    // cambia el color). En contextos sin registrar (tests, previews) cae a la
    // fuente proporcional por defecto.
    let bold_family = egui::FontFamily::Name("Ubuntu-Bold".into());
    let families = ui.ctx().fonts(|f| f.families());
    let title = if families.contains(&bold_family) {
        egui::RichText::new(text).size(15.0).family(bold_family)
    } else {
        egui::RichText::new(text).size(15.0)
    };
    egui::CollapsingHeader::new(title)
        .default_open(default_open)
        .show_unindented(ui, add_contents)
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
