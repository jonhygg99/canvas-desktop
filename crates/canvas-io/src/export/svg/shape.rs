//! Emision de una capa de forma (rectangulo, elipse, linea, poligono...).

use canvas_core::{
    arrow_head_rounded, arrow_shaft_end_x, cross_points, diamond_points, heart_points,
    regular_polygon_points, star_points, triangle_points, ShapeKind,
};

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
            // Extremos redondeados si hay corner_radius, a tajo si no.
            let cap = if shape.corner_radius > 0.0 { "round" } else { "butt" };
            svg.push_str(&format!(
                "<line x1=\"0\" y1=\"{y}\" x2=\"{w}\" y2=\"{y}\" {color} stroke-width=\"{sw}\" stroke-linecap=\"{cap}\"/>\n",
                y = n(h / 2.0),
                w = n(w),
                color = color,
                sw = n(f64::from(shape.stroke_width.max(0.0))),
                cap = cap,
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
            // Extremos del astil redondeados si hay corner_radius, a tajo
            // si no (coherente con la cabeza).
            let cap = if shape.corner_radius > 0.0 { "round" } else { "butt" };
            svg.push_str(&format!(
                "<line x1=\"0\" y1=\"{y}\" x2=\"{x2}\" y2=\"{y}\" stroke=\"{c}\" stroke-opacity=\"{a}\" stroke-width=\"{sw}\" stroke-linecap=\"{cap}\"/>\n",
                y = n(h / 2.0),
                x2 = n(arrow_shaft_end_x(w)),
                c = hex(chosen),
                a = n(chosen_alpha),
                sw = n(f64::from(shape.stroke_width.max(0.0))),
                cap = cap,
            ));
            // Cabeza redondeada (punta y base), con las mismas curvas que el
            // render y el radio de `corner_radius` (0 = a tajo): ver
            // `arrow_head_rounded` en canvas-core.
            let rp = arrow_head_rounded(w, h, f64::from(shape.corner_radius.max(0.0)));
            let mut d = format!("M {} {}", n(rp.start.0), n(rp.start.1));
            for (a, c, b) in &rp.segments {
                d.push_str(&format!(
                    " L {} {} Q {} {} {} {}",
                    n(a.0),
                    n(a.1),
                    n(c.0),
                    n(c.1),
                    n(b.0),
                    n(b.1)
                ));
            }
            d.push('Z');
            svg.push_str(&format!(
                "<path d=\"{d}\" fill=\"{c}\" fill-opacity=\"{a}\"/>\n",
                d = d,
                c = hex(chosen),
                a = n(chosen_alpha),
            ));
        }
        ShapeKind::Pentagon => {
            polygon_element(svg, &regular_polygon_points(w, h, 5), &fill_attr, &stroke_attr);
        }
        ShapeKind::Hexagon => {
            polygon_element(svg, &regular_polygon_points(w, h, 6), &fill_attr, &stroke_attr);
        }
        ShapeKind::Diamond => {
            polygon_element(svg, &diamond_points(w, h), &fill_attr, &stroke_attr);
        }
        ShapeKind::Cross => {
            polygon_element(svg, &cross_points(w, h), &fill_attr, &stroke_attr);
        }
        ShapeKind::Heart => {
            polygon_element(svg, &heart_points(w, h, 32), &fill_attr, &stroke_attr);
        }
    }
}

/// Emite un `<polygon>` con los puntos dados y los atributos de relleno y
/// borde ya formateados (formas poligonales nuevas).
fn polygon_element(svg: &mut String, pts: &[(f64, f64)], fill: &str, stroke: &str) {
    let pts = pts
        .iter()
        .map(|(x, y)| format!("{},{}", n(*x), n(*y)))
        .collect::<Vec<_>>()
        .join(" ");
    svg.push_str(&format!(
        "<polygon points=\"{pts}\" {fill} {stroke}/>\n",
        pts = pts,
        fill = fill,
        stroke = stroke,
    ));
}
