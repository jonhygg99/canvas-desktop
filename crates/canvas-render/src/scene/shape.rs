//! Pintado de una capa de forma (rectangulo, elipse, linea, poligono...).
//!
//! Movido tal cual desde el brazo `LayerContent::Shape` de `append_document`:
//! el cuerpo es identico, solo cambian `layer`/`view` de variables del bucle a
//! parametros.

use canvas_core::{Layer, ShapeContent, ShapeKind};
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
    }
}
