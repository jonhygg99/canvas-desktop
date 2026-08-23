//! Estado y UI del editor: el lienzo con zoom/paneo y el panel de propiedades.

use eframe::egui;

/// Tamaño de los manejadores de selección (esquinas + rotación), en puntos
/// de pantalla. Compartido por `interaction` (hit-testing) y `overlay`
/// (dibujo) — deben coincidir siempre, así que viven en un solo sitio.
pub(super) const HANDLE_SIZE: f32 = 9.0;
/// Color de acento de la selección/tira activa. Compartido por `overlay` y
/// `slot_chrome`.
pub(super) const ACCENT: egui::Color32 = egui::Color32::from_rgb(0, 122, 255);

mod canvas;
mod interaction;
mod layer_ops;
mod overlay;
mod properties_panel;
mod slot_chrome;
mod viewport;

pub use canvas::{canvas_ui, CanvasAction};
pub use properties_panel::properties_ui;
pub use viewport::Viewport;

mod state;

pub use state::{DeckNav, EditorState};
pub(crate) use state::{DeleteRecord, GlobalStep};
