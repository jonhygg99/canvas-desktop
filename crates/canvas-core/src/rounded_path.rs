//! Maquinaria de esquinas redondeadas para polígonos cerrados: el formato
//! agnóstico [`RoundedPath`] y su constructor [`rounded_polygon_path`].
//! Una sola fuente de verdad compartida por el render (vello) y la
//! exportación a SVG. Separada de `shape_geom` (primitivas de contorno)
//! para mantener cada archivo por debajo del objetivo de 400 líneas.

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

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn empty_polygon_yields_an_empty_path() {
        let rp = rounded_polygon_path(&[], 5.0);
        assert_eq!(rp.segments.len(), 0);
        assert_eq!(rp.start, (0.0, 0.0));
    }
}
