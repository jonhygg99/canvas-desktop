//! Controles de contenido de una capa de forma (relleno, trazo, radio de
//! esquina para rectángulos).

use eframe::egui;

/// Dibuja los controles y aplica los cambios directamente sobre `shape`.
/// Devuelve `(changed, commit)`: `changed` si algo se tocó este frame,
/// `commit` si el control que cambió ya soltó el foco/arrastre.
pub(super) fn shape_content_ui(
    ui: &mut egui::Ui,
    shape: &mut canvas_core::ShapeContent,
) -> (bool, bool) {
    let mut changed = false;
    let mut commit = false;

    ui.label("Shape");
    ui.horizontal(|ui| {
        ui.label("Fill");
        let mut fill = egui::Color32::from_rgba_unmultiplied(
            shape.fill[0],
            shape.fill[1],
            shape.fill[2],
            shape.fill[3],
        );
        if ui.color_edit_button_srgba(&mut fill).changed() {
            shape.fill = fill.to_array();
            changed = true;
            commit = true;
        }
        ui.label("Stroke");
        let mut stroke = egui::Color32::from_rgba_unmultiplied(
            shape.stroke[0],
            shape.stroke[1],
            shape.stroke[2],
            shape.stroke[3],
        );
        if ui.color_edit_button_srgba(&mut stroke).changed() {
            shape.stroke = stroke.to_array();
            changed = true;
            commit = true;
        }
        let r = ui.add(
            egui::DragValue::new(&mut shape.stroke_width)
                .range(0.0..=100.0)
                .speed(0.5)
                .max_decimals(1),
        );
        changed |= r.changed();
        commit |= r.drag_stopped() || r.lost_focus();
    });
    // El radio de esquina aplica al rectángulo, a la línea/astil (0 =
    // extremos a tajo, > 0 = redondeados) y a la cabeza de la flecha.
    if matches!(
        shape.kind,
        canvas_core::ShapeKind::Rect
            | canvas_core::ShapeKind::Line
            | canvas_core::ShapeKind::Arrow
    ) {
        ui.horizontal(|ui| {
            ui.label("Corner radius");
            let r = ui.add(
                egui::DragValue::new(&mut shape.corner_radius)
                    .range(0.0..=500.0)
                    .speed(1.0)
                    .max_decimals(0),
            );
            changed |= r.changed();
            commit |= r.drag_stopped() || r.lost_focus();
        });
    }
    ui.add_space(8.0);

    (changed, commit)
}
