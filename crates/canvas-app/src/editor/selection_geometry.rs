//! Caja común y transformación proporcional de conjuntos. Las coordenadas
//! de hijos de grupos ya son absolutas: nunca se aplica dos veces un delta.
use super::{layer_ops::selection_transforms, EditorState};
use canvas_core::{LayerId, Transform};

pub(super) fn bounds(items: &[(LayerId, Transform)]) -> Option<Transform> {
    let mut points = items.iter().flat_map(|(_, t)| t.corners());
    let (x, y) = points.next()?;
    let (mut left, mut top, mut right, mut bottom) = (x, y, x, y);
    for (x, y) in points {
        left = left.min(x);
        top = top.min(y);
        right = right.max(x);
        bottom = bottom.max(y);
    }
    Some(Transform::new(
        left,
        top,
        (right - left).max(1.0),
        (bottom - top).max(1.0),
    ))
}

/// Una capa normal conserva su caja rotada. Grupos y selecciones múltiples
/// usan la envolvente de las hojas editables, sin manejadores para bloqueadas.
pub(super) fn selection_box(state: &EditorState) -> Option<Transform> {
    if let super::interaction::Gesture::Selection(gesture) = &state.gesture {
        return Some(gesture.to);
    }
    let items = selection_transforms(state);
    if state.selection.len() == 1 && items.len() == 1 {
        Some(items[0].1)
    } else {
        bounds(&items)
    }
}

pub(super) fn transform_item(t: Transform, from: Transform, to: Transform) -> Transform {
    let scale = to.width / from.width;
    let angle = (to.rotation - from.rotation).to_radians();
    let (sin, cos) = angle.sin_cos();
    let (cx, cy) = t.center();
    let (fx, fy) = from.center();
    let (tx, ty) = to.center();
    let (dx, dy) = ((cx - fx) * scale, (cy - fy) * scale);
    let (w, h) = (t.width * scale, t.height * scale);
    Transform {
        x: tx + dx * cos - dy * sin - w / 2.0,
        y: ty + dx * sin + dy * cos - h / 2.0,
        width: w,
        height: h,
        rotation: t.rotation + to.rotation - from.rotation,
        ..t
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn common_resize_preserves_spacing_aspect_and_opposite_corner() {
        let from = Transform::new(10.0, 20.0, 100.0, 50.0);
        let to = Transform::new(10.0, 20.0, 200.0, 100.0);
        let a = transform_item(Transform::new(10.0, 20.0, 20.0, 10.0), from, to);
        let b = transform_item(Transform::new(90.0, 60.0, 20.0, 10.0), from, to);
        assert_eq!(a, Transform::new(10.0, 20.0, 40.0, 20.0));
        assert_eq!(b, Transform::new(170.0, 100.0, 40.0, 20.0));
    }
    #[test]
    fn rotation_moves_centers_around_common_pivot() {
        let from = Transform::new(0.0, 0.0, 100.0, 100.0);
        let mut to = from;
        to.rotation = 90.0;
        let a = transform_item(Transform::new(70.0, 40.0, 20.0, 20.0), from, to);
        assert!((a.x - 40.0).abs() < 1e-9 && (a.y - 70.0).abs() < 1e-9);
        assert_eq!(a.rotation, 90.0);
    }
    #[test]
    fn bounds_include_rotated_corners() {
        let mut t = Transform::new(0.0, 0.0, 20.0, 10.0);
        t.rotation = 90.0;
        let b = bounds(&[(LayerId::from_raw(1), t)]).unwrap();
        assert!((b.width - 10.0).abs() < 1e-9 && (b.height - 20.0).abs() < 1e-9);
    }
}
