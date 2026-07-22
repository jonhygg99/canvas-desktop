//! Guías de alineación magnéticas: matemática pura, sin UI. Al arrastrar una
//! capa, sus bordes y centro se «enganchan» a los bordes/centro de la página
//! y de las demás capas cuando quedan a menos de un umbral.

use crate::layer::Transform;

/// Resultado del imán: corrección a aplicar a la posición y guías a pintar.
#[derive(Debug, Default, PartialEq)]
pub struct SnapResult {
    /// Corrección en X (páginas px); 0 si no hay enganche horizontal.
    pub dx: f64,
    /// Corrección en Y.
    pub dy: f64,
    /// Posiciones X (página) de las guías verticales activas.
    pub v_guides: Vec<f64>,
    /// Posiciones Y (página) de las guías horizontales activas.
    pub h_guides: Vec<f64>,
}

/// Bordes y centro de un rect en un eje.
fn edges(pos: f64, size: f64) -> [f64; 3] {
    [pos, pos + size / 2.0, pos + size]
}

/// Calcula el imán para `moving` frente a la página (`page_w`×`page_h`) y al
/// resto de capas (`others`, ya sin la capa en movimiento). `threshold` en
/// píxeles de página. Solo actúa sobre capas sin rotar (con rotación los
/// bordes AABB dejan de ser significativos).
pub fn snap_translation(
    moving: &Transform,
    others: &[Transform],
    page_w: f64,
    page_h: f64,
    threshold: f64,
) -> SnapResult {
    if moving.rotation != 0.0 {
        return SnapResult::default();
    }

    // Candidatos por eje: bordes y centro de página + de cada capa.
    let mut x_targets = vec![0.0, page_w / 2.0, page_w];
    let mut y_targets = vec![0.0, page_h / 2.0, page_h];
    for other in others {
        if other.rotation != 0.0 {
            continue;
        }
        x_targets.extend(edges(other.x, other.width));
        y_targets.extend(edges(other.y, other.height));
    }

    let mut result = SnapResult::default();

    let best = |sources: [f64; 3], targets: &[f64]| -> Option<(f64, f64)> {
        let mut best: Option<(f64, f64)> = None; // (corrección, guía)
        for source in sources {
            for &target in targets {
                let delta = target - source;
                if delta.abs() <= threshold
                    && best.is_none_or(|(current, _)| delta.abs() < current.abs())
                {
                    best = Some((delta, target));
                }
            }
        }
        best
    };

    if let Some((dx, guide)) = best(edges(moving.x, moving.width), &x_targets) {
        result.dx = dx;
        result.v_guides.push(guide);
    }
    if let Some((dy, guide)) = best(edges(moving.y, moving.height), &y_targets) {
        result.dy = dy;
        result.h_guides.push(guide);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(x: f64, y: f64, w: f64, h: f64) -> Transform {
        Transform::new(x, y, w, h)
    }

    #[test]
    fn snaps_to_page_center() {
        // Capa de 100 de ancho con el centro a 3 px del centro de página (400).
        let moving = t(347.0, 10.0, 100.0, 50.0);
        let r = snap_translation(&moving, &[], 800.0, 600.0, 6.0);
        assert_eq!(r.dx, 3.0); // centro 397 → 400
        assert_eq!(r.v_guides, vec![400.0]);
    }

    #[test]
    fn snaps_to_other_layer_edge() {
        let moving = t(196.0, 300.0, 50.0, 50.0);
        let other = t(100.0, 0.0, 100.0, 100.0); // borde derecho en 200
        let r = snap_translation(&moving, &[other], 800.0, 600.0, 6.0);
        assert_eq!(r.dx, 4.0); // izquierda 196 → 200
        assert_eq!(r.v_guides, vec![200.0]);
    }

    #[test]
    fn no_snap_outside_threshold() {
        // Bordes X en 300/350/400 y targets 0/450/900; bordes Y en
        // 211/261/311 y targets 0/350/700: nada a menos de 6 px.
        let moving = t(300.0, 211.0, 100.0, 100.0);
        let r = snap_translation(&moving, &[], 900.0, 700.0, 6.0);
        assert_eq!(r.dx, 0.0);
        assert!(r.v_guides.is_empty());
        assert_eq!(r.dy, 0.0);
        assert!(r.h_guides.is_empty());
    }

    #[test]
    fn rotated_layers_do_not_snap() {
        let mut moving = t(347.0, 10.0, 100.0, 50.0);
        moving.rotation = 15.0;
        let r = snap_translation(&moving, &[], 800.0, 600.0, 6.0);
        assert_eq!(r, SnapResult::default());
    }

    #[test]
    fn picks_nearest_candidate() {
        // Borde izquierdo a 2 px de 0 y centro a 4 px del centro de página…
        // debe ganar el enganche de menor distancia (izquierda → 0).
        let moving = t(2.0, 0.0, 100.0, 50.0);
        let r = snap_translation(&moving, &[], 112.0, 600.0, 6.0);
        assert_eq!(r.dx, -2.0);
        assert_eq!(r.v_guides, vec![0.0]);
    }
}
