//! Cámara del lienzo: zoom, paneo, ajuste automático, y las conversiones de
//! coordenadas página↔pantalla que dependen de ella.

use canvas_core::Transform;
use eframe::egui;

use crate::deck::DeckRect;

const MIN_ZOOM: f64 = 0.02;
const MAX_ZOOM: f64 = 32.0;

/// Qué se vuelve a ajustar solo cuando la ventana cambia de tamaño. El
/// último ajuste automático manda: `Ctrl+0` deja `Active`, `Ctrl+Alt+0` deja
/// `All`, y cualquier zoom o paneo MANUAL lo apaga (`Off`) — nadie quiere
/// pelearse con un reajuste que le deshace el zoom que acaba de hacer a
/// mano. `Ctrl+0`/`Ctrl+Alt+0` lo vuelven a encender.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum AutoFit {
    Off,
    Active,
    All,
}

pub struct Viewport {
    /// Factor página → puntos de pantalla.
    pub zoom: f64,
    /// Desplazamiento del origen de la página, en puntos, relativo al lienzo.
    pub pan: egui::Vec2,
    pub(super) needs_fit: bool,
    /// Centrar este rect (en espacio de baraja) en el próximo frame, sin
    /// tocar el zoom: saltar a otro lienzo de la baraja por la tira o el
    /// teclado. `canvas_ui` lo consume en cuanto conoce el tamaño real del
    /// lienzo — un clic directo sobre un lienzo visible no lo usa, porque
    /// si ya se ve no hace falta recentrar la vista.
    pub(super) center_request: Option<DeckRect>,
    /// Qué volver a ajustar si el área de dibujo cambia de tamaño (ventana
    /// maximizada/restaurada, un panel lateral arrastrado). `Off` tras
    /// cualquier zoom o paneo manual.
    pub(super) auto_fit: AutoFit,
    /// Último tamaño visto del área de dibujo, para detectar el cambio.
    last_avail: egui::Vec2,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            pan: egui::Vec2::ZERO,
            needs_fit: true,
            center_request: None,
            auto_fit: AutoFit::Active,
            last_avail: egui::Vec2::ZERO,
        }
    }
}

impl Viewport {
    /// Ajusta `target` (en espacio de baraja) a la ventana: cambia zoom y
    /// pan. Con `target = (0,0,w,h)` (un solo lienzo en el origen) es
    /// idéntico al `fit` de antes de la Fase 14c — el caso general solo
    /// añade centrar sobre el centro de `target`, no necesariamente el
    /// origen. `mode` es qué volver a repetir si la ventana cambia de
    /// tamaño más tarde (ver `AutoFit`) — se recibe como parámetro, no se
    /// asigna suelto en cada sitio, para que sea imposible añadir un camino
    /// de ajuste que se olvide de armar/desarmar el reajuste automático.
    pub(super) fn fit(&mut self, target: DeckRect, avail: egui::Vec2, mode: AutoFit) {
        const MARGIN: f32 = 24.0;
        if target.w <= 0.0 || target.h <= 0.0 {
            return;
        }
        let usable_w = f64::from((avail.x - 2.0 * MARGIN).max(32.0));
        let usable_h = f64::from((avail.y - 2.0 * MARGIN).max(32.0));
        self.zoom = (usable_w / target.w)
            .min(usable_h / target.h)
            .clamp(MIN_ZOOM, MAX_ZOOM);
        self.center_on(target, avail);
        self.needs_fit = false;
        self.auto_fit = mode;
    }

    /// Centra `target` (en espacio de baraja) en la ventana sin tocar el
    /// zoom — saltar a otro lienzo de la baraja sin que el nivel de zoom
    /// cambie de golpe.
    pub(super) fn center_on(&mut self, target: DeckRect, avail: egui::Vec2) {
        let (cx, cy) = (target.x + target.w / 2.0, target.y + target.h / 2.0);
        self.pan = egui::vec2(
            (f64::from(avail.x) / 2.0 - cx * self.zoom) as f32,
            (f64::from(avail.y) / 2.0 - cy * self.zoom) as f32,
        );
    }

    pub(super) fn zoom_at(&mut self, anchor: egui::Vec2, factor: f64) {
        self.manual_view_change();
        let new_zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        let applied = new_zoom / self.zoom;
        self.pan = anchor - (anchor - self.pan) * applied as f32;
        self.zoom = new_zoom;
    }

    /// Vuelve a ajustar el lienzo activo a la ventana en el próximo frame.
    pub fn request_fit(&mut self) {
        self.needs_fit = true;
    }

    /// Pide centrar `target` (en espacio de baraja) en cuanto se conozca el
    /// tamaño real del lienzo, sin tocar el zoom.
    pub(crate) fn request_center(&mut self, target: DeckRect) {
        self.center_request = Some(target);
    }

    /// ¿Cambió el tamaño del área de dibujo desde el frame anterior? Sella
    /// el nuevo tamaño de paso. El umbral de medio punto evita reajustar
    /// por el temblor sub-píxel de `available_size` al arrastrar un panel.
    pub(super) fn note_size(&mut self, avail: egui::Vec2) -> bool {
        let changed =
            (self.last_avail.x - avail.x).abs() > 0.5 || (self.last_avail.y - avail.y).abs() > 0.5;
        self.last_avail = avail;
        changed
    }

    /// El usuario ha movido la vista a mano (zoom o paneo): deja de
    /// reajustar solo al redimensionar la ventana, hasta que vuelva a pedir
    /// un ajuste explícito (`Ctrl+0`/`Ctrl+Alt+0`).
    pub(super) fn manual_view_change(&mut self) {
        self.auto_fit = AutoFit::Off;
    }
}

pub(super) fn page_to_screen(vp: &Viewport, rect: egui::Rect, x: f64, y: f64) -> egui::Pos2 {
    rect.min + vp.pan + egui::vec2((x * vp.zoom) as f32, (y * vp.zoom) as f32)
}

pub(super) fn screen_to_page(vp: &Viewport, rect: egui::Rect, pos: egui::Pos2) -> (f64, f64) {
    let local = pos - rect.min - vp.pan;
    (f64::from(local.x) / vp.zoom, f64::from(local.y) / vp.zoom)
}

/// Esquinas de la capa (rotadas) en pantalla: [sup-izq, sup-der, inf-izq, inf-der].
pub(super) fn layer_corners_screen(
    vp: &Viewport,
    rect: egui::Rect,
    t: &Transform,
) -> [egui::Pos2; 4] {
    t.corners().map(|(x, y)| page_to_screen(vp, rect, x, y))
}

/// Posición en pantalla del manejador de rotación (por encima del centro del
/// borde superior, en la dirección local de la capa).
pub(super) fn rotation_handle_screen(vp: &Viewport, rect: egui::Rect, t: &Transform) -> egui::Pos2 {
    const OFFSET_SCREEN: f64 = 26.0;
    let theta = t.rotation.to_radians();
    let (sin, cos) = theta.sin_cos();
    let (cx, cy) = t.center();
    // Centro del borde superior + prolongación hacia fuera (en px de página).
    let reach = t.height / 2.0 + OFFSET_SCREEN / vp.zoom;
    let px = cx + reach * sin;
    let py = cy - reach * cos;
    page_to_screen(vp, rect, px, py)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64, w: f64, h: f64) -> DeckRect {
        DeckRect { x, y, w, h }
    }

    /// Un viewport con zoom y pan puestos a mano, sin pasar por `fit`.
    fn vp(zoom: f64, pan: egui::Vec2) -> Viewport {
        Viewport {
            zoom,
            pan,
            ..Default::default()
        }
    }

    /// El punto de la página que hay bajo `anchor` (coordenadas relativas al
    /// área de dibujo), según el zoom y el pan actuales.
    fn page_point_under(vp: &Viewport, anchor: egui::Vec2) -> (f64, f64) {
        (
            f64::from(anchor.x - vp.pan.x) / vp.zoom,
            f64::from(anchor.y - vp.pan.y) / vp.zoom,
        )
    }

    #[test]
    fn fit_centers_the_target_in_the_available_area() {
        let mut vp = Viewport::default();
        vp.fit(
            rect(0.0, 0.0, 800.0, 600.0),
            egui::vec2(1000.0, 800.0),
            AutoFit::Active,
        );

        // El centro de la página tiene que caer en el centro del área.
        let center = egui::vec2(500.0, 400.0);
        let (px, py) = page_point_under(&vp, center);
        assert!((px - 400.0).abs() < 1e-6, "centro en x: {px}");
        assert!((py - 300.0).abs() < 1e-6, "centro en y: {py}");
    }

    #[test]
    fn fit_leaves_a_margin_so_the_target_never_touches_the_edges() {
        let mut vp = Viewport::default();
        let avail = egui::vec2(1000.0, 800.0);
        vp.fit(rect(0.0, 0.0, 800.0, 600.0), avail, AutoFit::Active);

        let painted_w = 800.0 * vp.zoom;
        let painted_h = 600.0 * vp.zoom;
        assert!(painted_w < f64::from(avail.x), "sin margen horizontal");
        assert!(painted_h < f64::from(avail.y), "sin margen vertical");
    }

    #[test]
    fn fit_uses_the_axis_that_constrains_the_most() {
        // Un objetivo muy ancho lo limita el ancho disponible, no el alto.
        let mut vp = Viewport::default();
        vp.fit(
            rect(0.0, 0.0, 4000.0, 100.0),
            egui::vec2(1000.0, 1000.0),
            AutoFit::Active,
        );
        assert!(4000.0 * vp.zoom <= 1000.0, "se sale por el ancho");
        assert!(100.0 * vp.zoom <= 1000.0);
    }

    #[test]
    fn fit_ignores_a_degenerate_target() {
        let mut vp = vp(3.0, egui::Vec2::ZERO);
        vp.fit(
            rect(0.0, 0.0, 0.0, 0.0),
            egui::vec2(1000.0, 800.0),
            AutoFit::All,
        );
        assert_eq!(vp.zoom, 3.0, "una baraja vacía no debe tocar el zoom");
        assert!(vp.needs_fit, "ni consumir la petición de ajuste");
    }

    #[test]
    fn fit_arms_the_mode_it_was_asked_for_and_clears_the_pending_request() {
        let mut vp = Viewport::default();
        assert!(vp.needs_fit, "un viewport nuevo pide ajuste");

        vp.fit(
            rect(0.0, 0.0, 800.0, 600.0),
            egui::vec2(1000.0, 800.0),
            AutoFit::All,
        );
        assert!(!vp.needs_fit);
        assert!(vp.auto_fit == AutoFit::All);

        vp.fit(
            rect(0.0, 0.0, 800.0, 600.0),
            egui::vec2(1000.0, 800.0),
            AutoFit::Active,
        );
        assert!(vp.auto_fit == AutoFit::Active);
    }

    #[test]
    fn fit_clamps_the_zoom_to_the_allowed_range() {
        let mut vp = Viewport::default();
        // Un objetivo minúsculo pediría un zoom enorme.
        vp.fit(
            rect(0.0, 0.0, 0.001, 0.001),
            egui::vec2(1000.0, 800.0),
            AutoFit::Active,
        );
        assert!(vp.zoom <= MAX_ZOOM, "zoom sin tope: {}", vp.zoom);

        // Y uno gigantesco, un zoom ínfimo.
        let mut vp = Viewport::default();
        vp.fit(
            rect(0.0, 0.0, 1e9, 1e9),
            egui::vec2(1000.0, 800.0),
            AutoFit::Active,
        );
        assert!(vp.zoom >= MIN_ZOOM, "zoom sin suelo: {}", vp.zoom);
    }

    #[test]
    fn center_on_moves_the_view_without_touching_the_zoom() {
        let mut vp = vp(2.5, egui::Vec2::ZERO);
        let before = vp.zoom;
        vp.center_on(rect(1000.0, 0.0, 800.0, 600.0), egui::vec2(1000.0, 800.0));

        assert_eq!(vp.zoom, before, "saltar de lienzo no debe cambiar el zoom");
        let (px, py) = page_point_under(&vp, egui::vec2(500.0, 400.0));
        assert!((px - 1400.0).abs() < 1e-6, "centro en x: {px}");
        assert!((py - 300.0).abs() < 1e-6, "centro en y: {py}");
    }

    #[test]
    fn zoom_at_keeps_the_point_under_the_cursor_fixed() {
        // La invariante que hace que el zoom con rueda se sienta bien: el
        // píxel de la página que está bajo el cursor sigue estando ahí.
        let mut vp = vp(1.0, egui::vec2(37.0, -12.0));
        let anchor = egui::vec2(320.0, 180.0);
        let (bx, by) = page_point_under(&vp, anchor);

        vp.zoom_at(anchor, 1.7);
        let (ax, ay) = page_point_under(&vp, anchor);

        assert!((ax - bx).abs() < 1e-3, "se movió en x: {bx} -> {ax}");
        assert!((ay - by).abs() < 1e-3, "se movió en y: {by} -> {ay}");
    }

    #[test]
    fn zoom_at_keeps_the_anchor_fixed_even_when_it_hits_the_zoom_limit() {
        // Al toparse con el tope, el factor aplicado es menor que el pedido:
        // el pan tiene que compensar con el factor REAL, no con el pedido.
        let mut vp = vp(MAX_ZOOM / 2.0, egui::vec2(10.0, 20.0));
        let anchor = egui::vec2(400.0, 300.0);
        let (bx, by) = page_point_under(&vp, anchor);

        vp.zoom_at(anchor, 100.0);

        assert_eq!(vp.zoom, MAX_ZOOM);
        let (ax, ay) = page_point_under(&vp, anchor);
        assert!((ax - bx).abs() < 1e-3, "se movió en x: {bx} -> {ax}");
        assert!((ay - by).abs() < 1e-3, "se movió en y: {by} -> {ay}");
    }

    #[test]
    fn any_manual_zoom_disarms_the_automatic_refit() {
        // Nadie quiere pelearse con un reajuste que le deshace el zoom que
        // acaba de hacer a mano.
        let mut vp = Viewport::default();
        vp.fit(
            rect(0.0, 0.0, 800.0, 600.0),
            egui::vec2(1000.0, 800.0),
            AutoFit::All,
        );
        assert!(vp.auto_fit == AutoFit::All);

        vp.zoom_at(egui::vec2(100.0, 100.0), 1.1);
        assert!(vp.auto_fit == AutoFit::Off);
    }

    #[test]
    fn note_size_ignores_sub_pixel_jitter_but_reports_a_real_resize() {
        let mut vp = Viewport::default();
        assert!(
            vp.note_size(egui::vec2(1000.0, 800.0)),
            "el primer tamaño cuenta"
        );
        assert!(
            !vp.note_size(egui::vec2(1000.0, 800.0)),
            "el mismo tamaño, no"
        );

        // Temblor sub-píxel al arrastrar un panel lateral.
        assert!(!vp.note_size(egui::vec2(1000.3, 799.7)));
        // Un cambio de verdad.
        assert!(vp.note_size(egui::vec2(1000.0, 700.0)));
    }

    #[test]
    fn note_size_seals_the_size_even_when_it_reports_no_change() {
        // Si no sellara, el temblor se acumularía hasta pasar el umbral.
        let mut vp = Viewport::default();
        vp.note_size(egui::vec2(1000.0, 800.0));
        vp.note_size(egui::vec2(1000.4, 800.0));
        // Desde 1000.4, otro salto de 0.4 sigue por debajo del umbral.
        assert!(!vp.note_size(egui::vec2(1000.8, 800.0)));
    }

    #[test]
    fn request_center_is_a_pending_request_not_an_immediate_move() {
        let mut vp = Viewport::default();
        let before = vp.pan;
        vp.request_center(rect(500.0, 0.0, 800.0, 600.0));
        assert_eq!(
            vp.pan, before,
            "no debe mover la vista hasta el próximo frame"
        );
        assert!(vp.center_request.is_some());
    }
}
