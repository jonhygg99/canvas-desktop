//! Los iconos de la cabecera, dibujados a mano con el `Painter` de egui: no
//! hay fuente de iconos en el proyecto y son cuatro formas simples.

use eframe::egui;

/// Dirección de `draw_triangle_icon`.
#[derive(Clone, Copy)]
pub(super) enum IconDir {
    Up,
    Down,
    Left,
    Right,
}

/// Triángulo relleno apuntando en `dir`, centrado en `rect`.
pub(super) fn draw_triangle_icon(
    painter: &egui::Painter,
    rect: egui::Rect,
    dir: IconDir,
    color: egui::Color32,
) {
    let c = rect.center();
    let s = rect.width().min(rect.height()) * 0.28;
    let points = match dir {
        IconDir::Up => vec![
            c + egui::vec2(0.0, -s),
            c + egui::vec2(-s, s * 0.8),
            c + egui::vec2(s, s * 0.8),
        ],
        IconDir::Down => vec![
            c + egui::vec2(0.0, s),
            c + egui::vec2(-s, -s * 0.8),
            c + egui::vec2(s, -s * 0.8),
        ],
        IconDir::Left => vec![
            c + egui::vec2(-s, 0.0),
            c + egui::vec2(s * 0.8, -s),
            c + egui::vec2(s * 0.8, s),
        ],
        IconDir::Right => vec![
            c + egui::vec2(s, 0.0),
            c + egui::vec2(-s * 0.8, -s),
            c + egui::vec2(-s * 0.8, s),
        ],
    };
    painter.add(egui::Shape::convex_polygon(
        points,
        color,
        egui::Stroke::NONE,
    ));
}

/// Puntos a lo largo de un arco de circunferencia. Convención pensada para
/// que `start = -FRAC_PI_2`/`end = FRAC_PI_2` trace un semicírculo superior
/// de izquierda a derecha pasando por arriba (coordenadas de pantalla, eje Y
/// hacia abajo) — el arco del candado.
pub(super) fn arc_points(
    center: egui::Pos2,
    radius: f32,
    start: f32,
    end: f32,
    segments: usize,
) -> Vec<egui::Pos2> {
    (0..=segments)
        .map(|i| {
            let t = start + (end - start) * (i as f32 / segments as f32);
            center + egui::vec2(t.sin(), -t.cos()) * radius
        })
        .collect()
}

/// Candado: cuerpo relleno (rect redondeado) + arco. Cerrado, el arco se
/// apoya simétrico sobre el cuerpo; abierto, se desplaza hacia arriba y a
/// la derecha, dejando el lado izquierdo suelto.
pub(super) fn draw_lock_icon(
    painter: &egui::Painter,
    rect: egui::Rect,
    locked: bool,
    color: egui::Color32,
) {
    let half_pi = std::f32::consts::FRAC_PI_2;
    let body_w = rect.width() * 0.5;
    let body_h = rect.height() * 0.42;
    let body = egui::Rect::from_center_size(
        rect.center() + egui::vec2(0.0, body_h * 0.35),
        egui::vec2(body_w, body_h),
    );
    painter.rect_filled(body, 1.0, color);
    let shackle_r = body_w * 0.42;
    let stroke = egui::Stroke::new(1.3, color);
    let shackle_center = egui::pos2(body.center().x, body.top());
    let center = if locked {
        shackle_center
    } else {
        shackle_center + egui::vec2(shackle_r * 0.55, -shackle_r * 0.35)
    };
    let pts = arc_points(center, shackle_r, -half_pi, half_pi, 10);
    painter.add(egui::Shape::line(pts, stroke));
}

/// Duplicar: dos rects redondeados solapados (icono estándar de "copiar").
/// `bg` es el fondo de la propia cabecera — rellena el rect de delante para
/// que de verdad tape la esquina del de detrás, en vez de dejar ambos
/// contornos cruzándose sin más.
pub(super) fn draw_duplicate_icon(
    painter: &egui::Painter,
    rect: egui::Rect,
    color: egui::Color32,
    bg: egui::Color32,
) {
    let s = rect.width().min(rect.height()) * 0.36;
    let stroke = egui::Stroke::new(1.2, color);
    let back = egui::Rect::from_center_size(
        rect.center() + egui::vec2(-s * 0.28, -s * 0.28),
        egui::vec2(s, s),
    );
    let front = egui::Rect::from_center_size(
        rect.center() + egui::vec2(s * 0.28, s * 0.28),
        egui::vec2(s, s),
    );
    painter.rect_stroke(back, 1.0, stroke, egui::StrokeKind::Outside);
    painter.rect_filled(front, 1.0, bg);
    painter.rect_stroke(front, 1.0, stroke, egui::StrokeKind::Outside);
}

/// Borrar: cubo de basura simple — cuerpo, tapa y un par de ranuras.
pub(super) fn draw_delete_icon(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let stroke = egui::Stroke::new(1.2, color);
    let w = rect.width() * 0.42;
    let h = rect.height() * 0.4;
    let body =
        egui::Rect::from_center_size(rect.center() + egui::vec2(0.0, h * 0.2), egui::vec2(w, h));
    painter.rect_stroke(body, 0.5, stroke, egui::StrokeKind::Outside);
    painter.line_segment(
        [
            egui::pos2(body.left() - w * 0.15, body.top()),
            egui::pos2(body.right() + w * 0.15, body.top()),
        ],
        stroke,
    );
    for i in [-1.0_f32, 1.0] {
        let x = rect.center().x + i * w * 0.22;
        painter.line_segment(
            [
                egui::pos2(x, body.top() + h * 0.18),
                egui::pos2(x, body.bottom() - h * 0.12),
            ],
            stroke,
        );
    }
}
