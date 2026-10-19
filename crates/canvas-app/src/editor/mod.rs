//! Estado y UI del editor: el lienzo con zoom/paneo y el panel de propiedades.

use eframe::egui;

/// Tamaño de los manejadores de selección (esquinas + rotación), en puntos
/// de pantalla. Compartido por `interaction` (hit-testing) y `overlay`
/// (dibujo) — deben coincidir siempre, así que viven en un solo sitio.
pub(super) const HANDLE_SIZE: f32 = 9.0;
/// Color de acento de la selección/tira activa. Compartido por `overlay` y
/// `slot_chrome`.
pub(super) const ACCENT: egui::Color32 = egui::Color32::from_rgb(0, 122, 255);

pub(crate) mod canvas;
mod context_toolbar;
pub(crate) mod document_bar;
pub(crate) mod duplicate_gesture;
mod edge_handles;
mod gesture_cancel;
mod guides;
pub(crate) mod history_panel;
pub(crate) mod inline_text;
#[cfg(test)]
mod inline_text_tests;
mod inline_typography;
pub(crate) mod insert_tool;
#[cfg(test)]
mod insert_tool_tests;
mod interaction;
mod layer_ops;
mod layer_picking;
mod marquee;
mod selection_geometry;
mod selection_gesture;
#[cfg(test)]
mod selection_gesture_tests;
mod selection_layout;

#[cfg(test)]
#[path = "layer_ops_tests.rs"]
mod layer_ops_tests;
mod overlay;
pub(crate) mod palette;
mod palette_actions;
pub(crate) mod properties_panel;
mod slot_chrome;
mod spacing_overlay;
mod video_playback;
mod viewport;

pub use canvas::{canvas_ui, CanvasAction};
pub(crate) use layer_ops::{delete_selected, has_deletable_selection};
pub use properties_panel::properties_ui;
pub use viewport::Viewport;

pub(crate) mod state;

pub use state::{DeckNav, EditorState};
pub(crate) use state::{DeleteRecord, GlobalStep};

/// Deja el documento en un estado coherente antes de ABANDONAR el editor
/// (Back a la galería, abrir otro archivo, cerrar el proyecto):
///
/// - consolida las ediciones de panel a medias (`panel_edit`, sliders,
///   contenido, página) para que sean pasos de deshacer y el documento
///   cuente como sucio — así la comprobación de «cambios sin guardar» las
///   ve y nunca se pierden en silencio;
/// - confirma la edición de texto inline (sin esto, un texto a medio
///   escribir se descartaría al destruir la vista);
/// - cancela un gesto de ratón a medias (deja el documento como estaba) y
///   descarta los renombrados que solo eran texto de UI.
///
/// No toca un guardado/exportación en curso: quien llama comprueba antes
/// `EditorState::blocks_leaving`. Es idempotente y barata cuando no hay
/// nada abierto.
pub(crate) fn prepare_for_navigation(state: &mut EditorState) {
    if let Some(blocker) = state.idle_blocker() {
        // Traza de diagnóstico: si alguna vez una bandera se queda pegada,
        // el log dice CUÁL (el aviso vería «operación en curso» sin nombre).
        tracing::info!(
            bloqueo = blocker,
            "saliendo del editor: se consolidan o descartan ediciones transitorias"
        );
    }
    gesture_cancel::cancel_single_gesture(state);
    inline_text::finish(state, false);
    properties_panel::commit_all_panel_edits(state);
    state.rename_edit = None;
    state.file_rename_edit = None;
}
