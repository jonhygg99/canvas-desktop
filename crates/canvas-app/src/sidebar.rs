//! Métricas y piezas visuales compartidas por los paneles laterales del editor.

use eframe::egui;

pub const PANEL_PAD: f32 = 8.0;
pub fn compact(ui: &mut egui::Ui) {
    // Conserva las métricas de densidad elegidas, también en el inspector.
    ui.spacing_mut().item_spacing.x = 6.0;
}

pub fn title(ui: &mut egui::Ui, text: &str) {
    let text = crate::i18n::tr(text);
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
    let stable_id = egui::Id::new(text);
    let text = crate::i18n::tr(text);
    let bold_family = egui::FontFamily::Name("Ubuntu-Bold".into());
    let families = ui.ctx().fonts(|f| f.families());
    let title = if families.contains(&bold_family) {
        egui::RichText::new(text).size(15.0).family(bold_family)
    } else {
        egui::RichText::new(text).size(15.0)
    };
    egui::CollapsingHeader::new(title)
        .id_salt(stable_id)
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

/// El contenido puede desbordar su layout, pero nunca redimensionar el panel.
/// El borde lo gestiona Panel; el hijo conserva su recorte e interacciones.
pub fn fixed_width_ui<R>(ui: &mut egui::Ui, contents: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let rect = ui.available_rect_before_wrap();
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    child.set_clip_rect(ui.clip_rect().intersect(rect));
    let result = contents(&mut child);
    ui.allocate_rect(rect, egui::Sense::hover());
    result
}

#[cfg(test)]
mod tests {
    use super::{fixed_width_ui, PANEL_PAD};
    use eframe::egui;

    #[test]
    fn panel_width_ignores_content_and_can_be_resized() {
        let ctx = egui::Context::default();
        let mut widths = Vec::new();
        let pointer = |pressed, x| egui::Event::PointerButton {
            pos: egui::pos2(x, 100.0),
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        let frames = [
            vec![],
            vec![
                egui::Event::PointerMoved(egui::pos2(220.0, 100.0)),
                pointer(true, 220.0),
            ],
            vec![egui::Event::PointerMoved(egui::pos2(320.0, 100.0))],
            vec![pointer(false, 320.0)],
            vec![],
        ];
        for events in frames {
            let _ = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1000.0, 600.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    let response = egui::Panel::left("test-sidebar")
                        .frame(egui::Frame::NONE)
                        .default_size(220.0)
                        .size_range(180.0..=420.0)
                        .resizable(true)
                        .show(ui, |ui| {
                            fixed_width_ui(ui, |ui| {
                                // Simula una pestaña que pide más ancho que el disponible.
                                ui.allocate_space(egui::vec2(700.0, 40.0));
                            })
                        });
                    widths.push(response.response.rect.width());
                },
            );
        }
        assert_eq!(widths[0], 220.0);
        assert_eq!(*widths.last().unwrap(), 320.0);
    }

    #[test]
    fn sidebar_padding_is_compact_but_positive() {
        const { assert!(PANEL_PAD > 0.0) };
        const { assert!(PANEL_PAD <= 8.0) };
    }
}
