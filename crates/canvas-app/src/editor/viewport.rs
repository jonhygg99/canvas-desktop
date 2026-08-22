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
