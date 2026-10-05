//! Mutacion continua durante un gesto simple.
use super::super::EditorState;
use super::super::{
    overlay::{format_dims, show_drag_tag},
    viewport::screen_to_page,
};
use super::Gesture;
use canvas_core::{
    resize_rotated_from_corner, snap_translation, trim_crop_from_corner, CropRect, LayerContent,
    Transform,
};
use eframe::egui;
pub(super) fn apply(
    state: &mut EditorState,
    ui: &egui::Ui,
    response: &egui::Response,
    rect: egui::Rect,
) {
    // Gesto en curso: muta el documento en directo (sin comandos por frame),
    // siempre a partir del delta TOTAL desde el origen del gesto, inmune a
    // frames perdidos.
    if response.dragged_by(egui::PointerButton::Primary) {
        if let Some(pos) = response.interact_pointer_pos() {
            match state.gesture {
                Gesture::Move { .. } => move_layer(state, ui, pos),
                Gesture::Resize { .. } => resize_layer(state, ui, pos),
                Gesture::Rotate { .. } => rotate_layer(state, ui, pos, rect),
                Gesture::Crop { .. } => crop_layer(state, ui, pos),
                Gesture::None | Gesture::Selection(_) | Gesture::Marquee(_) => {}
            }
        }
    }
}

fn move_layer(state: &mut EditorState, ui: &egui::Ui, pos: egui::Pos2) {
    let Gesture::Move {
        layer,
        start,
        origin,
    } = state.gesture
    else {
        return;
    };

    let (mut dx, mut dy) = (
        f64::from(pos.x - origin.x) / state.viewport.zoom,
        f64::from(pos.y - origin.y) / state.viewport.zoom,
    );
    let shift = ui.input(|i| i.modifiers.shift);
    if shift {
        if dx.abs() >= dy.abs() {
            dy = 0.0;
        } else {
            dx = 0.0;
        }
    }
    let mut moved = Transform {
        x: start.x + dx,
        y: start.y + dy,
        ..start
    };
    // Guías magnéticas (Alt las desactiva).
    state.snap_guides = (Vec::new(), Vec::new());
    let alt = ui.ctx().input(|i| i.modifiers.alt);
    if !alt {
        if let Ok(page) = state.doc.page() {
            // Los grupos no tienen geometría propia (su
            // `transform` es una caja envolvente derivada) y
            // una capa oculta por un ancestro no debe atraer
            // el arrastre aunque su propio flag `visible`
            // siga en `true`.
            let others: Vec<Transform> = page
                .layers
                .iter()
                .filter(|l| {
                    l.id != layer
                        && !matches!(l.content, LayerContent::Group(_))
                        && page.effective_visible(l.id)
                })
                .map(|l| l.transform)
                .collect();
            let threshold = 6.0 / state.viewport.zoom;
            let snap = snap_translation(&moved, &others, page.width, page.height, threshold);
            if !shift || dy == 0.0 {
                moved.x += snap.dx;
                state.snap_guides.0 = snap.v_guides;
            }
            if !shift || dx == 0.0 {
                moved.y += snap.dy;
                state.snap_guides.1 = snap.h_guides;
            }
        }
    }
    if let Ok(l) = state.doc.layer_mut(layer) {
        l.transform.x = moved.x;
        l.transform.y = moved.y;
    }
}

fn resize_layer(state: &mut EditorState, ui: &egui::Ui, pos: egui::Pos2) {
    let Gesture::Resize {
        layer,
        corner,
        side,
        start,
        origin,
    } = state.gesture
    else {
        return;
    };

    let (dx, dy) = (
        f64::from(pos.x - origin.x) / state.viewport.zoom,
        f64::from(pos.y - origin.y) / state.viewport.zoom,
    );
    let shift = ui.ctx().input(|i| i.modifiers.shift);
    let keep_aspect = state.aspect_lock != shift; // Shift invierte el candado
    let t = if let Some(side) = side {
        super::super::edge_handles::resize(start, side, dx, dy)
    } else {
        resize_rotated_from_corner(&start, corner, dx, dy, keep_aspect, 1.0)
    };
    if let Ok(l) = state.doc.layer_mut(layer) {
        l.transform = t;
    }
    // Dimensiones en píxeles junto al cursor mientras se arrastra.
    show_drag_tag(ui, pos, format_dims(&t));
}

fn rotate_layer(state: &mut EditorState, ui: &egui::Ui, pos: egui::Pos2, rect: egui::Rect) {
    let Gesture::Rotate {
        layer,
        start,
        grab_offset,
    } = state.gesture
    else {
        return;
    };

    let (px, py) = screen_to_page(&state.viewport, rect, pos);
    let (cx, cy) = start.center();
    let pointer_angle = (py - cy).atan2(px - cx).to_degrees();
    let mut rotation = grab_offset + pointer_angle;
    // Shift: pasos de 15°.
    if ui.ctx().input(|i| i.modifiers.shift) {
        rotation = (rotation / 15.0).round() * 15.0;
    }
    rotation = rotation.rem_euclid(360.0);
    if rotation > 180.0 {
        rotation -= 360.0;
    }
    if let Ok(l) = state.doc.layer_mut(layer) {
        l.transform.rotation = rotation;
    }
    show_drag_tag(ui, pos, format!("{rotation:.0}°"));
}

fn crop_layer(state: &mut EditorState, ui: &egui::Ui, pos: egui::Pos2) {
    let Gesture::Crop {
        layer,
        corner,
        start_t,
        start_crop,
        origin,
    } = state.gesture
    else {
        return;
    };

    let (dx, dy) = (
        f64::from(pos.x - origin.x) / state.viewport.zoom,
        f64::from(pos.y - origin.y) / state.viewport.zoom,
    );
    let (t, crop) = trim_crop_from_corner(
        &start_t,
        start_crop.unwrap_or_else(CropRect::full),
        corner,
        dx,
        dy,
    );
    if let Ok(l) = state.doc.layer_mut(layer) {
        l.transform = t;
        match &mut l.content {
            LayerContent::Image(content) => content.crop = Some(crop),
            LayerContent::Video(content) => content.crop = Some(crop),
            _ => {}
        }
    }
    show_drag_tag(ui, pos, format_dims(&t));
}
