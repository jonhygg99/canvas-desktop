//! Controles de contenido de una capa de texto (texto, fuente, tamaño,
//! peso/cursiva, color, espaciado, alineación).

use eframe::egui;

/// Dibuja los controles y aplica los cambios directamente sobre `text`.
/// Devuelve `(changed, commit)`: `changed` si algo se tocó este frame,
/// `commit` si el control que cambió ya soltó el foco/arrastre.
pub(super) fn text_content_ui(
    ui: &mut egui::Ui,
    text: &mut canvas_core::TextContent,
) -> (bool, bool) {
    let mut changed = false;
    let mut commit = false;

    ui.label("Text");
    let r = ui.add(
        egui::TextEdit::multiline(&mut text.text)
            .desired_rows(2)
            .desired_width(f32::INFINITY),
    );
    changed |= r.changed();
    commit |= r.lost_focus();

    ui.horizontal(|ui| {
        ui.label("Font");
        let r = ui.add(egui::TextEdit::singleline(&mut text.family).hint_text("System default"));
        changed |= r.changed();
        commit |= r.lost_focus();
    });
    ui.horizontal(|ui| {
        ui.label("Size");
        let r = ui.add(
            egui::DragValue::new(&mut text.size)
                .range(4.0..=800.0)
                .speed(1.0),
        );
        changed |= r.changed();
        commit |= r.drag_stopped() || r.lost_focus();

        let bold = text.weight >= 600;
        if ui
            .selectable_label(bold, "B")
            .on_hover_text("Bold")
            .clicked()
        {
            text.weight = if bold { 400 } else { 700 };
            changed = true;
            commit = true;
        }
        if ui
            .selectable_label(text.italic, "I")
            .on_hover_text("Italic")
            .clicked()
        {
            text.italic = !text.italic;
            changed = true;
            commit = true;
        }
        let mut color = egui::Color32::from_rgba_unmultiplied(
            text.color[0],
            text.color[1],
            text.color[2],
            text.color[3],
        );
        if ui.color_edit_button_srgba(&mut color).changed() {
            text.color = color.to_array();
            changed = true;
            commit = true;
        }
    });
    ui.horizontal(|ui| {
        ui.label("Spacing");
        let r = ui.add(
            egui::DragValue::new(&mut text.letter_spacing)
                .range(-20.0..=60.0)
                .speed(0.2)
                .max_decimals(1),
        );
        changed |= r.changed();
        commit |= r.drag_stopped() || r.lost_focus();
        ui.label("Line");
        let r = ui.add(
            egui::DragValue::new(&mut text.line_height)
                .range(0.5..=3.0)
                .speed(0.02)
                .max_decimals(2),
        );
        changed |= r.changed();
        commit |= r.drag_stopped() || r.lost_focus();
    });
    ui.horizontal(|ui| {
        for (align, label) in [
            (canvas_core::TextAlign::Left, "Left"),
            (canvas_core::TextAlign::Center, "Center"),
            (canvas_core::TextAlign::Right, "Right"),
        ] {
            if ui.selectable_label(text.align == align, label).clicked() {
                text.align = align;
                changed = true;
                commit = true;
            }
        }
    });
    ui.add_space(8.0);

    (changed, commit)
}
