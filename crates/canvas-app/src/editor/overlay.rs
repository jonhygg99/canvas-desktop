//! Overlays de solo lectura sobre el lienzo activo: contorno/manejadores de
//! la selección, la etiqueta flotante de arrastre, la cuadrícula y las
//! reglas — nada de esto muta `EditorState`, solo lo lee para dibujar.

use canvas_core::Transform;
use eframe::egui;

use super::viewport::{
    layer_corners_screen, page_to_screen, rotation_handle_screen, screen_to_page,
};
use super::{EditorState, ACCENT, HANDLE_SIZE};

pub(super) fn format_dims(t: &Transform) -> String {
    format!(
        "{} × {} px",
        t.width.round() as i64,
        t.height.round() as i64
    )
}

/// Etiqueta flotante junto al cursor durante un gesto (dimensiones, grados).
pub(super) fn show_drag_tag(ui: &egui::Ui, pos: egui::Pos2, text: String) {
    let painter = ui.painter();
    let galley =
        painter.layout_no_wrap(text, egui::FontId::proportional(12.0), egui::Color32::WHITE);
    let tag_pos = pos + egui::vec2(14.0, 16.0);
    let bg = egui::Rect::from_min_size(tag_pos, galley.size() + egui::vec2(10.0, 6.0));
    painter.rect_filled(bg, 4.0, egui::Color32::from_black_alpha(190));
    painter.galley(tag_pos + egui::vec2(5.0, 3.0), galley, egui::Color32::WHITE);
}

/// Recuadro de selección (rotado), manejadores, manejador de rotación y guías
/// magnéticas, pintados por encima del lienzo.
/// `coord`: origen de coordenadas página→pantalla (el lienzo activo puede no
/// estar en el origen de la baraja, ver `canvas_ui`). `clip`: rect real del
/// viewport en pantalla — recorte del `Painter` y extremos de las guías/
/// manejadores que deben llegar de borde a borde del lienzo visible, no del
/// lienzo activo si estuviera desplazado.
pub(super) fn draw_selection_overlay(
    state: &EditorState,
    ui: &egui::Ui,
    coord: egui::Rect,
    clip: egui::Rect,
) {
    let painter = ui.painter_at(clip);

    // Guías magnéticas activas (líneas que cruzan todo el lienzo).
    let guide_stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(255, 64, 129));
    for &gx in &state.snap_guides.0 {
        let x = page_to_screen(&state.viewport, coord, gx, 0.0).x;
        painter.line_segment(
            [egui::pos2(x, clip.top()), egui::pos2(x, clip.bottom())],
            guide_stroke,
        );
    }
    for &gy in &state.snap_guides.1 {
        let y = page_to_screen(&state.viewport, coord, 0.0, gy).y;
        painter.line_segment(
            [egui::pos2(clip.left(), y), egui::pos2(clip.right(), y)],
            guide_stroke,
        );
    }

    // Contorno fino (sin manejadores) para las capas seleccionadas que NO
    // son la primaria: la primaria es la única que manda en el panel y los
    // gestos, pero el resto de la selección múltiple sigue siendo visible.
    for &id in state.selection.ids() {
        if Some(id) == state.selection.primary() {
            continue;
        }
        let Ok(layer) = state.doc.layer(id) else {
            continue;
        };
        let [tl, tr, bl, br] = layer_corners_screen(&state.viewport, coord, &layer.transform);
        painter.add(egui::Shape::closed_line(
            vec![tl, tr, br, bl],
            egui::Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(0, 122, 255, 140)),
        ));
    }

    let Some(sel) = state.selection.primary() else {
        return;
    };
    let Ok(layer) = state.doc.layer(sel) else {
        return;
    };
    let t = &layer.transform;
    let accent = if state.crop_mode {
        egui::Color32::from_rgb(255, 149, 0) // naranja: modo recorte
    } else {
        ACCENT
    };

    // Contorno rotado: sup-izq → sup-der → inf-der → inf-izq.
    let [tl, tr, bl, br] = layer_corners_screen(&state.viewport, coord, t);
    painter.add(egui::Shape::closed_line(
        vec![tl, tr, br, bl],
        egui::Stroke::new(1.5, accent),
    ));

    // Manejador de rotación: línea + círculo (no en modo recorte).
    if !state.crop_mode {
        let top_center = egui::pos2((tl.x + tr.x) / 2.0, (tl.y + tr.y) / 2.0);
        let handle = rotation_handle_screen(&state.viewport, coord, t);
        painter.line_segment([top_center, handle], egui::Stroke::new(1.0, accent));
        painter.circle_filled(handle, HANDLE_SIZE / 2.0, egui::Color32::WHITE);
        painter.circle_stroke(handle, HANDLE_SIZE / 2.0, egui::Stroke::new(1.5, accent));
    }

    // Manejadores de esquina (cuadrados centrados en las esquinas rotadas).
    for corner in [tl, tr, bl, br] {
        let hrect = egui::Rect::from_center_size(corner, egui::Vec2::splat(HANDLE_SIZE));
        painter.rect_filled(hrect, 2.0, egui::Color32::WHITE);
        painter.rect_stroke(
            hrect,
            2.0,
            egui::Stroke::new(1.5, accent),
            egui::StrokeKind::Inside,
        );
    }

    if state.crop_mode {
        show_drag_tag(
            ui,
            egui::pos2(tl.x, tl.y - 34.0),
            "Crop: drag the corners; click Done to finish".to_owned(),
        );
    }
}

/// Cuadrícula adaptativa sobre la página (paso elegido para ~24 px de
/// pantalla como mínimo entre líneas).
pub(super) fn draw_grid(
    state: &EditorState,
    ui: &egui::Ui,
    coord: egui::Rect,
    clip: egui::Rect,
    page: (f64, f64),
) {
    const STEPS: [f64; 10] = [1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0, 1000.0];
    let zoom = state.viewport.zoom;
    let Some(step) = STEPS.iter().copied().find(|s| s * zoom >= 24.0) else {
        return;
    };
    let painter = ui.painter_at(clip);
    let stroke = egui::Stroke::new(1.0, ui.visuals().text_color().gamma_multiply(0.15));
    let (pw, ph) = page;

    let mut x = 0.0;
    while x <= pw {
        let a = page_to_screen(&state.viewport, coord, x, 0.0);
        let b = page_to_screen(&state.viewport, coord, x, ph);
        painter.line_segment([a, b], stroke);
        x += step;
    }
    let mut y = 0.0;
    while y <= ph {
        let a = page_to_screen(&state.viewport, coord, 0.0, y);
        let b = page_to_screen(&state.viewport, coord, pw, y);
        painter.line_segment([a, b], stroke);
        y += step;
    }
}

/// Reglas superior e izquierda con marcas y números en píxeles de página.
/// `coord`/`clip`: ver `draw_selection_overlay`. Las barras de las reglas y
/// el rango visible que miden son siempre del viewport real (`clip`); solo
/// las marcas y números usan `coord` para traducir a píxeles de página.
pub(super) fn draw_rulers(state: &EditorState, ui: &egui::Ui, coord: egui::Rect, clip: egui::Rect) {
    const THICKNESS: f32 = 18.0;
    const STEPS: [f64; 10] = [1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0, 1000.0];
    let zoom = state.viewport.zoom;
    let Some(step) = STEPS.iter().copied().find(|s| s * zoom >= 56.0) else {
        return;
    };
    let painter = ui.painter_at(clip);
    let bg = ui.visuals().extreme_bg_color.gamma_multiply(0.9);
    let fg = ui.visuals().text_color().gamma_multiply(0.75);
    let tick_stroke = egui::Stroke::new(1.0, fg);
    let font = egui::FontId::proportional(9.5);

    let top = egui::Rect::from_min_max(clip.min, egui::pos2(clip.max.x, clip.min.y + THICKNESS));
    let left = egui::Rect::from_min_max(clip.min, egui::pos2(clip.min.x + THICKNESS, clip.max.y));
    painter.rect_filled(top, 0.0, bg);
    painter.rect_filled(left, 0.0, bg);

    // Rango de página visible en el lienzo.
    let (x0, y0) = screen_to_page(&state.viewport, coord, clip.min);
    let (x1, y1) = screen_to_page(&state.viewport, coord, clip.max);

    let mut x = (x0 / step).floor() * step;
    while x <= x1 {
        let sx = page_to_screen(&state.viewport, coord, x, 0.0).x;
        painter.line_segment(
            [
                egui::pos2(sx, top.bottom() - 6.0),
                egui::pos2(sx, top.bottom()),
            ],
            tick_stroke,
        );
        painter.text(
            egui::pos2(sx + 3.0, top.top() + 1.0),
            egui::Align2::LEFT_TOP,
            format!("{x:.0}"),
            font.clone(),
            fg,
        );
        x += step;
    }
    let mut y = (y0 / step).floor() * step;
    while y <= y1 {
        let sy = page_to_screen(&state.viewport, coord, 0.0, y).y;
        painter.line_segment(
            [
                egui::pos2(left.right() - 6.0, sy),
                egui::pos2(left.right(), sy),
            ],
            tick_stroke,
        );
        painter.text(
            egui::pos2(left.left() + 1.0, sy + 2.0),
            egui::Align2::LEFT_TOP,
            format!("{y:.0}"),
            font.clone(),
            fg,
        );
        y += step;
    }
}
