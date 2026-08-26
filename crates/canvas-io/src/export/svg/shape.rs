//! Emision de una capa de forma (rectangulo, elipse, linea, poligono...).

use canvas_core::{arrow_head_points, arrow_shaft_end_x, star_points, triangle_points, ShapeKind};

use super::util::{alpha, hex, n};

/// Rect/ellipse/line nativos de SVG, en la MISMA caja local (0,0)..(w,h) que
/// usa `scene.rs` (mismo criterio para omitir relleno/borde: `if fa > 0` /
/// `if sa > 0 && stroke_width > 0`, y la línea usa el borde si lo hay, si no
/// el relleno).
pub(super) fn shape_element(svg: &mut String, shape: &canvas_core::ShapeContent, w: f64, h: f64) {
    let [_, _, _, fa] = shape.fill;
    let [_, _, _, sa] = shape.stroke;
    let has_fill = fa > 0;
    let has_stroke = sa > 0 && shape.stroke_width > 0.0;
    let fill_attr = if has_fill {
        format!(
            "fill=\"{}\" fill-opacity=\"{}\"",
            hex(shape.fill),
            n(alpha(shape.fill))
        )
    } else {
        "fill=\"none\"".to_owned()
    };
    let stroke_attr = if has_stroke {
        format!(
            "stroke=\"{}\" stroke-opacity=\"{}\" stroke-width=\"{}\"",
            hex(shape.stroke),
            n(alpha(shape.stroke)),
            n(f64::from(shape.stroke_width)),
        )
    } else {
        String::new()
    };
    match shape.kind {
        ShapeKind::Rect => {
            svg.push_str(&format!(
                "<rect width=\"{w}\" height=\"{h}\" rx=\"{rx}\" {fill} {stroke}/>\n",
                w = n(w),
                h = n(h),
                rx = n(f64::from(shape.corner_radius.max(0.0))),
                fill = fill_attr,
                stroke = stroke_attr,
            ));
        }
        ShapeKind::Ellipse => {
            svg.push_str(&format!(
                "<ellipse cx=\"{cx}\" cy=\"{cy}\" rx=\"{rx}\" ry=\"{ry}\" {fill} {stroke}/>\n",
                cx = n(w / 2.0),
                cy = n(h / 2.0),
                rx = n(w / 2.0),
                ry = n(h / 2.0),
                fill = fill_attr,
                stroke = stroke_attr,
            ));
        }
        ShapeKind::Line => {
            // La línea usa el borde si lo hay; si no, el relleno (scene.rs:314).
            let color = if has_stroke {
                format!(
                    "stroke=\"{}\" stroke-opacity=\"{}\"",
                    hex(shape.stroke),
                    n(alpha(shape.stroke))
                )
            } else {
                format!(
                    "stroke=\"{}\" stroke-opacity=\"{}\"",
                    hex(shape.fill),
                    n(alpha(shape.fill))
                )
            };
            svg.push_str(&format!(
                "<line x1=\"0\" y1=\"{y}\" x2=\"{w}\" y2=\"{y}\" {color} stroke-width=\"{sw}\"/>\n",
                y = n(h / 2.0),
                w = n(w),
                color = color,
                sw = n(f64::from(shape.stroke_width.max(0.0))),
            ));
        }
        ShapeKind::Triangle => {
            let pts = triangle_points(w, h)
                .iter()
                .map(|(x, y)| format!("{},{}", n(*x), n(*y)))
                .collect::<Vec<_>>()
                .join(" ");
            svg.push_str(&format!(
                "<polygon points=\"{pts}\" {fill} {stroke}/>\n",
                pts = pts,
                fill = fill_attr,
                stroke = stroke_attr,
            ));
        }
        ShapeKind::Star => {
            let pts = star_points(w, h, 5, 0.45)
                .iter()
                .map(|(x, y)| format!("{},{}", n(*x), n(*y)))
                .collect::<Vec<_>>()
                .join(" ");
            svg.push_str(&format!(
                "<polygon points=\"{pts}\" {fill} {stroke}/>\n",
                pts = pts,
                fill = fill_attr,
                stroke = stroke_attr,
            ));
        }
        ShapeKind::Arrow => {
            // Mismo criterio de color que la línea: borde si lo hay, si no
            // relleno, y astil y cabeza lo comparten (escena shape.rs).
            let (chosen, chosen_alpha) = if has_stroke {
                (shape.stroke, alpha(shape.stroke))
            } else {
                (shape.fill, alpha(shape.fill))
            };
            let head_pts = arrow_head_points(w, h)
                .iter()
                .map(|(x, y)| format!("{},{}", n(*x), n(*y)))
                .collect::<Vec<_>>()
                .join(" ");
            svg.push_str(&format!(
                "<line x1=\"0\" y1=\"{y}\" x2=\"{x2}\" y2=\"{y}\" stroke=\"{c}\" stroke-opacity=\"{a}\" stroke-width=\"{sw}\"/>\n",
                y = n(h / 2.0),
                x2 = n(arrow_shaft_end_x(w)),
                c = hex(chosen),
                a = n(chosen_alpha),
                sw = n(f64::from(shape.stroke_width.max(0.0))),
            ));
            // La cabeza va rellena del MISMO color.
            svg.push_str(&format!(
                "<polygon points=\"{pts}\" fill=\"{c}\" fill-opacity=\"{a}\"/>\n",
                pts = head_pts,
                c = hex(chosen),
                a = n(chosen_alpha),
            ));
        }
    }
}
