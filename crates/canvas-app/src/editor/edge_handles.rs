//! Manejadores de borde: cambian un único eje y anclan el borde opuesto,
//! también en capas rotadas. Ignoran el candado de proporción de las esquinas.
use canvas_core::Transform;
use eframe::egui;
#[derive(Clone, Copy)]
pub(super) enum Side {
    Top,
    Right,
    Bottom,
    Left,
}
pub(super) fn positions([tl, tr, bl, br]: [egui::Pos2; 4]) -> [(Side, egui::Pos2); 4] {
    [
        (Side::Top, tl + (tr - tl) * 0.5),
        (Side::Right, tr + (br - tr) * 0.5),
        (Side::Bottom, bl + (br - bl) * 0.5),
        (Side::Left, tl + (bl - tl) * 0.5),
    ]
}
pub(super) fn at(corners: [egui::Pos2; 4], pos: egui::Pos2) -> Option<Side> {
    positions(corners)
        .into_iter()
        .find(|(_, p)| p.distance(pos) <= 10.5)
        .map(|(side, _)| side)
}
pub(super) fn resize(start: Transform, side: Side, dx: f64, dy: f64) -> Transform {
    let (sin, cos) = start.rotation.to_radians().sin_cos();
    let (lx, ly) = (dx * cos + dy * sin, -dx * sin + dy * cos);
    let (w, h, sx, sy) = match side {
        Side::Right => {
            let w = (start.width + lx).max(1.0);
            (w, start.height, (w - start.width) / 2.0, 0.0)
        }
        Side::Left => {
            let w = (start.width - lx).max(1.0);
            (w, start.height, -(w - start.width) / 2.0, 0.0)
        }
        Side::Bottom => {
            let h = (start.height + ly).max(1.0);
            (start.width, h, 0.0, (h - start.height) / 2.0)
        }
        Side::Top => {
            let h = (start.height - ly).max(1.0);
            (start.width, h, 0.0, -(h - start.height) / 2.0)
        }
    };
    let (cx, cy) = start.center();
    Transform {
        x: cx + sx * cos - sy * sin - w / 2.0,
        y: cy + sx * sin + sy * cos - h / 2.0,
        width: w,
        height: h,
        ..start
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn side_resize_changes_one_axis_and_preserves_opposite_edge_when_rotated() {
        let mut t = Transform::new(20.0, 30.0, 80.0, 40.0);
        t.rotation = 90.0;
        let r = resize(t, Side::Right, 0.0, 20.0);
        assert_eq!(r.width, 100.0);
        assert_eq!(r.height, 40.0);
        let before = t.corners();
        let after = r.corners();
        for i in [0, 2] {
            assert!(
                (before[i].0 - after[i].0).abs() < 1e-9 && (before[i].1 - after[i].1).abs() < 1e-9
            );
        }
    }
    #[test]
    fn crossing_opposite_edge_never_flips_the_layer() {
        let r = resize(Transform::new(0.0, 0.0, 40.0, 20.0), Side::Left, 100.0, 0.0);
        assert_eq!(r.width, 1.0);
        assert_eq!(r.x, 39.0);
    }
}
