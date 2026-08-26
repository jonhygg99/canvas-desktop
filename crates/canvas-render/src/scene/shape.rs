//! Pintado de una capa de forma (rectangulo, elipse, linea, poligono...).
//!
//! Movido tal cual desde el brazo `LayerContent::Shape` de `append_document`:
//! el cuerpo es identico, solo cambian `layer`/`view` de variables del bucle a
//! parametros.

use canvas_core::{arrow_head_points, arrow_shaft_end_x, star_points, triangle_points, Layer, ShapeContent, ShapeKind};
use vello::kurbo::{Affine, Rect};
use vello::peniko::{Color, Fill};
use vello::Scene;

use super::raster::place_transform;

pub(super) fn draw_shape(scene: &mut Scene, layer: &Layer, shape: &ShapeContent, view: Affine) {
    let t = layer.transform;
    let place = view * place_transform(&t);
    let [fr, fg, fb, fa] = shape.fill;
    let [sr, sg, sb, sa] = shape.stroke;
    let fill_color = Color::from_rgba8(fr, fg, fb, fa);
    let stroke_color = Color::from_rgba8(sr, sg, sb, sa);
    let stroke = vello::kurbo::Stroke::new(f64::from(shape.stroke_width.max(0.0)));
    match shape.kind {
        ShapeKind::Rect => {
            let rounded = vello::kurbo::RoundedRect::from_rect(
                Rect::new(0.0, 0.0, t.width, t.height),
                f64::from(shape.corner_radius.max(0.0)),
            );
            if fa > 0 {
                scene.fill(Fill::NonZero, place, fill_color, None, &rounded);
            }
            if sa > 0 && shape.stroke_width > 0.0 {
                scene.stroke(&stroke, place, stroke_color, None, &rounded);
            }
        }
        ShapeKind::Ellipse => {
            let ellipse = vello::kurbo::Ellipse::new(
                (t.width / 2.0, t.height / 2.0),
                (t.width / 2.0, t.height / 2.0),
                0.0,
            );
            if fa > 0 {
                scene.fill(Fill::NonZero, place, fill_color, None, &ellipse);
            }
            if sa > 0 && shape.stroke_width > 0.0 {
                scene.stroke(&stroke, place, stroke_color, None, &ellipse);
            }
        }
        ShapeKind::Line => {
            let line = vello::kurbo::Line::new((0.0, t.height / 2.0), (t.width, t.height / 2.0));
            let color = if sa > 0 { stroke_color } else { fill_color };
            scene.stroke(&stroke, place, color, None, &line);
        }
        ShapeKind::Triangle => {
            let path = polygon_path(&triangle_points(t.width, t.height));
            if fa > 0 {
                scene.fill(Fill::NonZero, place, fill_color, None, &path);
            }
            if sa > 0 && shape.stroke_width > 0.0 {
                scene.stroke(&stroke, place, stroke_color, None, &path);
            }
        }
        ShapeKind::Star => {
            let path = polygon_path(&star_points(t.width, t.height, 5, 0.45));
            if fa > 0 {
                scene.fill(Fill::NonZero, place, fill_color, None, &path);
            }
            if sa > 0 && shape.stroke_width > 0.0 {
                scene.stroke(&stroke, place, stroke_color, None, &path);
            }
        }
        ShapeKind::Arrow => {
            // Mismo criterio de color que la línea: borde si lo hay, si no
            // relleno; astil y cabeza comparten ese color para que la flecha
            // sea un solo objeto visual.
            let color = if sa > 0 { stroke_color } else { fill_color };
            let shaft = vello::kurbo::Line::new(
                (0.0, t.height / 2.0),
                (arrow_shaft_end_x(t.width), t.height / 2.0),
            );
            scene.stroke(&stroke, place, color, None, &shaft);
            let head = polygon_path(&arrow_head_points(t.width, t.height));
            scene.fill(Fill::NonZero, place, color, None, &head);
        }
    }
}

/// Camino cerrado a partir de una lista de puntos (caja local). Usado por las
/// formas poligonales (triángulo, estrella, cabeza de flecha).
fn polygon_path(points: &[(f64, f64)]) -> vello::kurbo::BezPath {
    let mut path = vello::kurbo::BezPath::new();
    let Some((x0, y0)) = points.first() else {
        return path;
    };
    path.move_to((*x0, *y0));
    for (x, y) in &points[1..] {
        path.line_to((*x, *y));
    }
    path.close_path();
    path
}
