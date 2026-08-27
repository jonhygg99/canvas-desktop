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

/// Puntos de un polígono regular de `sides` lados centrado en la caja, con
/// un vértice arriba (alineado con el centro vertical), como el triángulo
/// y la estrella.
pub fn regular_polygon_points(w: f64, h: f64, sides: u32) -> Vec<(f64, f64)> {
    let sides = sides.max(3);
    let (cx, cy) = (w / 2.0, h / 2.0);
    let r = w.min(h) * 0.48;
    let mut pts = Vec::with_capacity(sides as usize);
    for i in 0..sides {
        let a = -std::f64::consts::FRAC_PI_2 + std::f64::consts::TAU * i as f64 / sides as f64;
        pts.push((cx + a.cos() * r, cy + a.sin() * r));
    }
    pts
}

/// Rombo (cuadrado girado 45°) centrado en la caja.
pub fn diamond_points(w: f64, h: f64) -> [(f64, f64); 4] {
    [
        (w * 0.5, h * 0.10),
        (w * 0.90, h * 0.50),
        (w * 0.5, h * 0.90),
        (w * 0.10, h * 0.50),
    ]
}

/// Cruz (signo de suma) centrada en la caja: cuatro brazos del mismo grosor.
pub fn cross_points(w: f64, h: f64) -> [(f64, f64); 12] {
    let (cx, cy) = (w / 2.0, h / 2.0);
    let t = w.min(h) * 0.26; // grosor de los brazos
    let r = w.min(h) * 0.42; // alcance de los brazos
    let (hx, hy) = (t / 2.0, t / 2.0);
    [
        (cx - hx, cy - r),
        (cx + hx, cy - r),
        (cx + hx, cy - hy),
        (cx + r, cy - hy),
        (cx + r, cy + hy),
        (cx + hx, cy + hy),
        (cx + hx, cy + r),
        (cx - hx, cy + r),
        (cx - hx, cy + hy),
        (cx - r, cy + hy),
        (cx - r, cy - hy),
        (cx - hx, cy - hy),
    ]
}

/// Puntos del contorno de un corazón (curva paramétrica clásica), centrado
/// en la caja, con la punta hacia abajo. `samples` controla la suavidad
/// (32 dan un contorno redondeado sin necesidad de curvas). Los límites se
/// miden sobre los propios puntos, así la forma queda siempre contenida y
/// centrada aunque la curva cambie.
pub fn heart_points(w: f64, h: f64, samples: usize) -> Vec<(f64, f64)> {
    let n = samples.max(16);
    let mut raw = Vec::with_capacity(n);
    for i in 0..n {
        let t = std::f64::consts::TAU * i as f64 / n as f64;
        let x = 16.0 * t.sin().powi(3) / 17.0;
        let y =
            (13.0 * t.cos() - 5.0 * (2.0 * t).cos() - 2.0 * (3.0 * t).cos() - (4.0 * t).cos())
                / 17.0;
        raw.push((x, y));
    }
    let min_x = raw.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
    let max_x = raw.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
    let min_y = raw.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
    let max_y = raw.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
    let (x_span, y_span) = (max_x - min_x, max_y - min_y);
    // Margen del 8 % como el triángulo y la estrella, para que la forma
    // no toque los bordes de la caja de la capa.
    let s = (w / x_span).min(h / y_span) * 0.88;
    let ox = w / 2.0;
    // Y de pantalla invertida (la punta −1 abajo), centrada en la caja.
    let oy = h / 2.0 + (max_y + min_y) / 2.0 * s;
    raw.into_iter()
        .map(|(x, y)| (ox + x * s, oy - y * s))
        .collect()
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

/// Un segmento de esquina redondeada: `(entrada en la arista, vértice de
/// control, salida de la esquina)`. Tras la entrada se traza una línea
/// hasta el vértice y una cuadrática con ese control hasta la salida.
pub type RoundedSegment = ((f64, f64), (f64, f64), (f64, f64));

/// Ruta de un polígono cerrado con esquinas redondeadas, en un formato
/// agnóstico que cada backend consume: arranca en `start` y por cada
/// segmento traza una línea hasta `a`, una cuadrática con el vértice `c`
/// de control, y termina en `b`; la ruta se cierra después del último
/// segmento.
#[derive(Debug, Clone, PartialEq)]
pub struct RoundedPath {
    pub start: (f64, f64),
    pub segments: Vec<RoundedSegment>,
}

impl RoundedPath {
    /// Puntos del contorno aplanado a segmentos rectos, para backends sin
    /// curvas (el painter de egui). `smooth` = segmentos por esquina.
    pub fn to_polyline(&self, smooth: usize) -> Vec<(f64, f64)> {
        let smooth = smooth.max(1);
        let mut pts = Vec::with_capacity(1 + self.segments.len() * (smooth + 1));
        pts.push(self.start);
        for (a, c, b) in &self.segments {
            for i in 1..=smooth {
                let t = i as f64 / smooth as f64;
                let u = 1.0 - t;
                pts.push((
                    u * u * a.0 + 2.0 * u * t * c.0 + t * t * b.0,
                    u * u * a.1 + 2.0 * u * t * c.1 + t * t * b.1,
                ));
            }
        }
        pts
    }
}

/// Redondea las esquinas de un polígono cerrado: en cada vértice las dos
/// aristas adyacentes se recortan `radius` y se unen con una cuadrática
/// cuyo control es el propio vértice. El radio se recorta para no superar
/// ~40 % de la arista más corta (si no, las tangentes se cruzan).
pub fn rounded_polygon_path(corners: &[(f64, f64)], radius: f64) -> RoundedPath {
    let n = corners.len();
    if n == 0 {
        return RoundedPath {
            start: (0.0, 0.0),
            segments: Vec::new(),
        };
    }
    let min_edge = (0..n)
        .map(|i| {
            let a = corners[i];
            let b = corners[(i + 1) % n];
            (a.0 - b.0).hypot(a.1 - b.1)
        })
        .fold(f64::INFINITY, f64::min);
    let r = radius.max(0.0).min(min_edge * 0.4);
    let along = |from: (f64, f64), to: (f64, f64)| {
        let (dx, dy) = (to.0 - from.0, to.1 - from.1);
        let len = dx.hypot(dy).max(1e-9);
        (from.0 + dx / len * r, from.1 + dy / len * r)
    };
    let mut segments = Vec::with_capacity(n);
    let mut start = None;
    for i in 0..n {
        let prev = corners[(i + n - 1) % n];
        let c = corners[i];
        let next = corners[(i + 1) % n];
        let a = along(c, prev);
        let b = along(c, next);
        if start.is_none() {
            start = Some(a);
        }
        segments.push((a, c, b));
    }
    RoundedPath {
        start: start.expect("n > 0 garantiza una primera esquina"),
        segments,
    }
}

/// Contorno de la cabeza de la flecha en la caja local (0,0)..(w,h) con
/// `radius` como radio de esquina: 0 = triángulo a tajo, > 0 = punta y
/// esquinas de la base redondeadas (el MISMO estilo que el astil de tapas
/// redondas). `radius` se recorta si la cabeza es pequeña
/// (`rounded_polygon_path`).
pub fn arrow_head_rounded(w: f64, h: f64, radius: f64) -> RoundedPath {
    let corners = arrow_head_points(w, h);
    rounded_polygon_path(&corners, radius)
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

    #[test]
    fn rounded_arrow_head_moves_corners_inward_and_stays_convex() {
        let w = 400.0;
        let h = 200.0;
        let sharp = arrow_head_points(w, h);
        // El radio por defecto (≈18 % del largo de la cabeza a tamaño de
        // inserción) reproducía el aspecto redondeado actual.
        let rp = arrow_head_rounded(w, h, w * 0.38 * 0.18);
        assert_eq!(rp.segments.len(), 3);
        // Cada esquina se recorta hacia dentro (la entrada `a` ya no es el
        // vértice agudo).
        for (i, (a, c, _)) in rp.segments.iter().enumerate() {
            assert_eq!(*c, sharp[i]);
            assert!(*a != sharp[i]);
        }
        // La punta redondeada queda entre las dos esquinas de la base.
        let flat = rp.to_polyline(6);
        let min_x = flat.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
        let max_x = flat.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
        assert!(min_x >= sharp[0].0);
        assert!(max_x <= sharp[2].0);
        // start + 3 esquinas × 6 muestras.
        assert_eq!(flat.len(), 1 + 3 * 6);
    }

    #[test]
    fn regular_polygon_has_sides_points_centered_and_first_on_top() {
        for (sides, label) in [(5, "pentagon"), (6, "hexagon")] {
            let pts = regular_polygon_points(300.0, 200.0, sides);
            assert_eq!(pts.len(), sides as usize, "{label}");
            let (cx, cy) = (150.0, 100.0);
            // Primer vértice arriba, centrado horizontalmente.
            assert!((pts[0].0 - cx).abs() < 1e-6, "{label}");
            assert!(pts[0].1 < cy, "{label}");
            // Todos a la misma distancia del centro (polígono regular).
            let r0 = (pts[0].0 - cx).hypot(pts[0].1 - cy);
            for p in &pts {
                let r = (p.0 - cx).hypot(p.1 - cy);
                assert!((r - r0).abs() < 1e-6, "{label}");
            }
        }
    }

    #[test]
    fn diamond_is_symmetric_and_inside_the_box() {
        let pts = diamond_points(200.0, 100.0);
        assert_eq!(pts.len(), 4);
        // Vértice superior centrado, izquierdo/derecho a mitad de altura.
        assert!((pts[0].0 - 100.0).abs() < 1e-6);
        assert!((pts[1].1 - 50.0).abs() < 1e-6);
        assert!((pts[2].0 - 100.0).abs() < 1e-6);
        assert!((pts[3].1 - 50.0).abs() < 1e-6);
        for (x, y) in pts {
            assert!((0.0..=200.0).contains(&x) && (0.0..=100.0).contains(&y));
        }
    }

    #[test]
    fn cross_is_symmetric_with_four_equal_arms() {
        let pts = cross_points(200.0, 200.0);
        assert_eq!(pts.len(), 12);
        let (cx, cy) = (100.0, 100.0);
        let reach = |p: (f64, f64)| (p.0 - cx).abs().max((p.1 - cy).abs());
        // Los cuatro brazos (arriba, derecha, abajo, izquierda) llegan
        // igual de lejos del centro.
        assert_eq!(reach(pts[0]), reach(pts[3]));
        assert_eq!(reach(pts[3]), reach(pts[6]));
        assert_eq!(reach(pts[6]), reach(pts[9]));
        // Los dos vértices de cada brazo comparten la coordenada fija
        // (brazo superior: mismo y; brazo derecho: mismo x).
        assert!((pts[0].1 - pts[1].1).abs() < 1e-6);
        assert!((pts[3].0 - pts[4].0).abs() < 1e-6);
        assert!((pts[6].1 - pts[7].1).abs() < 1e-6);
        assert!((pts[9].0 - pts[10].0).abs() < 1e-6);
        // Todo dentro de la caja.
        for (x, y) in pts {
            assert!((0.0..=200.0).contains(&x) && (0.0..=200.0).contains(&y));
        }
    }

    #[test]
    fn heart_fits_the_box_with_tip_below_center() {
        let pts = heart_points(280.0, 280.0, 32);
        assert_eq!(pts.len(), 32);
        let min_x = pts.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
        let max_x = pts.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
        let min_y = pts.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
        let max_y = pts.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
        // Dentro de la caja, con margen (~6 % por lado tras el 0.88).
        assert!(min_x >= 0.0 && max_x <= 280.0);
        assert!(min_y >= 0.0 && max_y <= 280.0);
        assert!(min_x > 5.0 && max_x < 275.0);
        // La punta (y máxima, abajo en pantalla) queda por debajo del centro.
        assert!(max_y > 140.0);
        assert!(min_y < 140.0);
        // El corazón es simétrico en x.
        for p in &pts {
            assert!((p.0 - 140.0).abs() <= 140.0);
        }
        let left = pts.iter().map(|p| 140.0 - p.0).fold(f64::NEG_INFINITY, f64::max);
        let right = pts.iter().map(|p| p.0 - 140.0).fold(f64::NEG_INFINITY, f64::max);
        assert!((left - right).abs() < 1e-6);
    }

    #[test]
    fn zero_radius_keeps_the_sharp_triangle() {
        let w = 400.0;
        let h = 200.0;
        let rp = arrow_head_rounded(w, h, 0.0);
        // Radio 0: los tangentes coinciden con los vértices y la polilínea
        // (start + una muestra por esquina) repite la primera esquina y
        // luego recorre el triángulo original.
        let flat = rp.to_polyline(1);
        let sharp = arrow_head_points(w, h);
        assert_eq!(flat.len(), 4);
        assert_eq!(flat[0], sharp[0]);
        for (got, want) in flat[1..].iter().zip(sharp.iter()) {
            assert!((got.0 - want.0).abs() < 1e-9);
            assert!((got.1 - want.1).abs() < 1e-9);
        }
    }

    #[test]
    fn rounded_radius_is_clamped_to_short_edges() {
        // Un triángulo diminuto no debe romper las tangentes: el radio se
        // recorta a ~40 % de la arista más corta.
        let corners = [(0.0, 0.0), (10.0, 0.0), (5.0, 10.0)];
        let rp = rounded_polygon_path(&corners, 1000.0);
        for (a, c, b) in &rp.segments {
            let d1 = (a.0 - c.0).hypot(a.1 - c.1);
            let d2 = (b.0 - c.0).hypot(b.1 - c.1);
            assert!(d1 <= 4.0 + 1e-9);
            assert!(d2 <= 4.0 + 1e-9);
        }
    }
}
