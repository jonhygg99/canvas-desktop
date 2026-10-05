//! Cursor del gesto segun el manejador bajo el puntero.
use super::super::EditorState;
use super::super::{
    viewport::{layer_corners_screen, rotation_handle_screen, screen_to_page},
    HANDLE_SIZE,
};
use super::corner_at;
use super::Gesture;
use canvas_core::Corner;
use eframe::egui;
pub(super) fn show(
    state: &EditorState,
    ui: &egui::Ui,
    response: &egui::Response,
    rect: egui::Rect,
) {
    let pointer = response
        .interact_pointer_pos()
        .or_else(|| response.hover_pos());

    // Cursor según lo que hay debajo.
    let editable_primary = state
        .selection
        .primary()
        .filter(|&id| state.doc.page().is_ok_and(|p| !p.effective_locked(id)));
    if let (Some(pos), Some(sel)) = (pointer, editable_primary) {
        if let Ok(layer) = state.doc.layer(sel) {
            let corners = layer_corners_screen(&state.viewport, rect, &layer.transform);
            let on_rotate = rotation_handle_screen(&state.viewport, rect, &layer.transform)
                .distance(pos)
                <= HANDLE_SIZE / 2.0 + 3.0;
            if on_rotate {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
            } else if let Some(corner) = corner_at(corners, pos) {
                let icon = match corner {
                    Corner::TopLeft | Corner::BottomRight => egui::CursorIcon::ResizeNwSe,
                    Corner::TopRight | Corner::BottomLeft => egui::CursorIcon::ResizeNeSw,
                };
                ui.ctx().set_cursor_icon(icon);
            } else if let Some(side) =
                super::super::edge_handles::at(corners, pos).filter(|_| !state.crop_mode)
            {
                ui.ctx().set_cursor_icon(match side {
                    super::super::edge_handles::Side::Left
                    | super::super::edge_handles::Side::Right => egui::CursorIcon::ResizeHorizontal,
                    _ => egui::CursorIcon::ResizeVertical,
                });
            } else {
                let (px, py) = screen_to_page(&state.viewport, rect, pos);
                if layer.transform.contains_point(px, py) && matches!(state.gesture, Gesture::None)
                {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Move);
                }
            }
        }
    }
}
