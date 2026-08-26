//! Geometría de las formas vectoriales nativas (triángulo, estrella,
//! flecha): los puntos de sus contornos en la caja local (0,0)..(w,h).
//! Una sola fuente de verdad compartida por el render (vello) y la
//! exportación a SVG, para que pantalla y archivo no diverjan.

/// Triángulo regular apuntando hacia arriba, centrado en la caja.
pub fn triangle_points(w: f64, h: f64) -> [(f64, f64); 3] {
    [
        (w * 0.5, h * 0.10),
        (w * 0.12, h * 0.90),
        (w * 0.88, h * 0.90),
    ]
}

/// Puntos del contorno de una estrella de `spikes` puntas centrada en la
/// caja, con la punta superior alineada con el centro vertical. Cada dos
/// puntos alterna el radio exterior con el interior (`inner_ratio` de 0..=1).
pub fn star_points(w: f64, h: f64, spikes: u32, inner_ratio: f64) -> Vec<(f64, f64)> {
    let spikes = spikes.max(2);
    let (cx, cy) = (w / 2.0, h / 2.0);
    let r_out = w.min(h) * 0.48;
    let r_in = r_out * inner_ratio.clamp(0.1, 0.9);
    let mut pts = Vec::with_capacity(spikes as usize * 2);
    for i in 0..spikes * 2 {
        let a = -std::f64::consts::FRAC_PI_2 + std::f64::consts::PI * i as f64 / spikes as f64;
        let r = if i % 2 == 0 { r_out } else { r_in };
        pts.push((cx + a.cos() * r, cy + a.sin() * r));
    }
    pts
}

/// Extremo derecho del astil de la flecha (la punta arranca ahí): la
/// geometría de astil y cabeza no se solapan ni dejan hueco.
pub fn arrow_shaft_end_x(w: f64) -> f64 {
    w * 0.60
}

/// Cabeza de la flecha: triángulo apuntando a la derecha, enganchado al
/// extremo del astil.
pub fn arrow_head_points(w: f64, h: f64) -> [(f64, f64); 3] {
    [
        (arrow_shaft_end_x(w), h * 0.14),
        (arrow_shaft_end_x(w), h * 0.86),
        (w * 0.98, h * 0.50),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triangle_has_three_points_inside_the_box() {
        let pts = triangle_points(200.0, 100.0);
        assert_eq!(pts.len(), 3);
        // La punta está en el centro horizontal y por encima de la base.
        assert!((pts[0].0 - 100.0).abs() < 1e-6);
        assert!(pts[0].1 < pts[1].1);
        // Base simétrica.
        assert!((pts[1].0 - 24.0).abs() < 1e-6);
        assert!((pts[2].0 - 176.0).abs() < 1e-6);
        assert!((pts[1].1 - pts[2].1).abs() < 1e-6);
    }

    #[test]
    fn star_has_10_points_and_first_is_top_center() {
        let pts = star_points(300.0, 300.0, 5, 0.45);
        assert_eq!(pts.len(), 10);
        // La primera punta cae en el eje vertical superior.
        assert!((pts[0].0 - 150.0).abs() < 1e-6);
        assert!(pts[0].1 < 150.0);
        // Los radios exterior/interior alternan: la punta 0 está más lejos
        // del centro que la muesca 1.
        let d = |p: (f64, f64)| (p.0 - 150.0).hypot(p.1 - 150.0);
        assert!(d(pts[0]) > d(pts[1]));
    }

    #[test]
    fn arrow_head_attaches_to_the_shaft_end() {
        let w = 400.0;
        let h = 100.0;
        let head = arrow_head_points(w, h);
        let shaft_x = arrow_shaft_end_x(w);
        // La base de la cabeza comparte x con el final del astil.
        assert!((head[0].0 - shaft_x).abs() < 1e-6);
        assert!((head[1].0 - shaft_x).abs() < 1e-6);
        // La punta sobresale hacia la derecha y el centro vertical coincide.
        assert!(head[2].0 > shaft_x);
        assert!((head[2].1 - h / 2.0).abs() < 1e-6);
    }
}
