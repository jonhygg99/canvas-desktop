//! Gesto de edición en curso sobre el lienzo (mover/redimensionar/rotar/
//! recortar) y su hit-testing: qué manejador hay bajo el puntero, y cómo el
//! arrastre en pantalla se traduce en un nuevo `Transform`/`CropRect`,
//! mutando el documento en vivo y consolidando en UN comando al soltar.

use canvas_core::{Corner, CropRect, LayerId, Transform};
use eframe::egui;

use super::gesture_cancel::cancel_single_gesture;
use super::viewport::screen_to_page;
use super::{EditorState, HANDLE_SIZE};

#[path = "interaction_cursor.rs"]
mod cursor;
#[path = "interaction_finish.rs"]
mod finish;
#[path = "interaction_start.rs"]
mod start;
#[path = "interaction_update.rs"]
mod update;

/// Gesto de edición en curso sobre el lienzo. El documento se muta en directo
/// durante el gesto y al soltarlo se consolida en UN comando de deshacer.
pub(super) enum Gesture {
    None,
    Selection(super::selection_gesture::SelectionGesture),
    Marquee(super::marquee::Marquee),
    Duplicate(super::duplicate_gesture::Draft),
    Move {
        layer: LayerId,
        start: Transform,
        origin: egui::Pos2,
    },
    Resize {
        layer: LayerId,
        corner: Corner,
        side: Option<super::edge_handles::Side>,
        start: Transform,
        origin: egui::Pos2,
    },
    Rotate {
        layer: LayerId,
        start: Transform,
        /// `rotación inicial − ángulo inicial del puntero` (grados): la capa
        /// sigue al puntero sin saltar al agarrar el manejador.
        grab_offset: f64,
    },
    /// Modo recorte: las esquinas mueven los bordes de la ventana visible
    /// sobre el contenido, que queda clavado en la página.
    Crop {
        layer: LayerId,
        corner: Corner,
        start_t: Transform,
        start_crop: Option<CropRect>,
        origin: egui::Pos2,
    },
}

/// La esquina (si hay) cuyo manejador contiene el punto de pantalla.
pub(super) fn corner_at(corners: [egui::Pos2; 4], pos: egui::Pos2) -> Option<Corner> {
    const ORDER: [Corner; 4] = [
        Corner::TopLeft,
        Corner::TopRight,
        Corner::BottomLeft,
        Corner::BottomRight,
    ];
    let reach = HANDLE_SIZE / 2.0 + 6.0;
    ORDER
        .into_iter()
        .zip(corners)
        .find(|(_, p)| p.distance(pos) <= reach)
        .map(|(c, _)| c)
}

/// El manejador de crop conserva la prioridad incluso fuera de la página.
pub(super) fn crop_corner_at(
    state: &EditorState,
    rect: egui::Rect,
    pos: egui::Pos2,
) -> Option<Corner> {
    if !state.crop_mode || state.selection.len() != 1 {
        return None;
    }
    let id = state.selection.primary()?;
    let page = state.doc.page().ok()?;
    let layer = state.doc.layer(id).ok()?;
    if page.effective_locked(id)
        || !matches!(
            layer.content,
            canvas_core::LayerContent::Image(_) | canvas_core::LayerContent::Video(_)
        )
    {
        return None;
    }
    corner_at(
        super::viewport::layer_corners_screen(&state.viewport, rect, &layer.transform),
        pos,
    )
}

pub(super) fn layer_interaction(
    state: &mut EditorState,
    ui: &mut egui::Ui,
    response: &egui::Response,
    rect: egui::Rect,
) {
    if state.ytdlp.edit.is_some() {
        return;
    }
    if super::insert_tool::handle(state, ui, response, rect) {
        return;
    }
    if state.inline_text.is_some() {
        return;
    }
    if super::duplicate_gesture::handle(state, ui, response, rect) {
        return;
    }
    if response.double_clicked_by(egui::PointerButton::Primary) {
        if let Some(pos) = response.interact_pointer_pos() {
            let (x, y) = screen_to_page(&state.viewport, rect, pos);
            if let Some(id) = state.doc.page().ok().and_then(|p| p.layer_at(x, y)) {
                super::inline_text::begin(state, id);
                if state.inline_text.is_some() {
                    return;
                }
            }
        }
    }
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) && cancel_single_gesture(state) {
        return;
    }
    if super::selection_gesture::handle(state, ui, response, rect) {
        return;
    }
    if super::marquee::handle(state, ui, response, rect) {
        return;
    }
    cursor::show(state, ui, response, rect);
    start::begin(state, ui, response, rect);
    update::apply(state, ui, response, rect);
    finish::commit(state, response);
    finish::select(state, ui, response, rect);
}
