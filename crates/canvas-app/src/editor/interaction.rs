//! Gesto de edición en curso sobre el lienzo (mover/redimensionar/rotar/
//! recortar) y su hit-testing: qué manejador hay bajo el puntero, y cómo el
//! arrastre en pantalla se traduce en un nuevo `Transform`/`CropRect`,
//! mutando el documento en vivo y consolidando en UN comando al soltar.

use canvas_core::{
    resize_rotated_from_corner, snap_translation, trim_crop_from_corner, Corner, CropRect,
    LayerContent, LayerId, SetCrop, SetTransform, Transform,
};
use eframe::egui;

use super::{
    format_dims, layer_corners_screen, rotation_handle_screen, screen_to_page, show_drag_tag,
    EditorState, HANDLE_SIZE,
};

/// Gesto de edición en curso sobre el lienzo. El documento se muta en directo
/// durante el gesto y al soltarlo se consolida en UN comando de deshacer.
pub(super) enum Gesture {
    None,
    Move {
        layer: LayerId,
        start: Transform,
        origin: egui::Pos2,
    },
    Resize {
        layer: LayerId,
        corner: Corner,
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
fn corner_at(corners: [egui::Pos2; 4], pos: egui::Pos2) -> Option<Corner> {
    const ORDER: [Corner; 4] = [
        Corner::TopLeft,
        Corner::TopRight,
        Corner::BottomLeft,
        Corner::BottomRight,
    ];
    let reach = HANDLE_SIZE / 2.0 + 3.0;
    ORDER
        .into_iter()
        .zip(corners)
        .find(|(_, p)| p.distance(pos) <= reach)
        .map(|(c, _)| c)
}

pub(super) fn layer_interaction(
    state: &mut EditorState,
    ui: &mut egui::Ui,
    response: &egui::Response,
    rect: egui::Rect,
) {
    let pointer = response
        .interact_pointer_pos()
        .or_else(|| response.hover_pos());

    // Cursor según lo que hay debajo.
    if let (Some(pos), Some(sel)) = (pointer, state.selection.primary()) {
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
            } else {
                let (px, py) = screen_to_page(&state.viewport, rect, pos);
                if layer.transform.contains_point(px, py) && matches!(state.gesture, Gesture::None)
                {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Move);
                }
            }
        }
    }

    // Inicio de gesto.
    if response.drag_started_by(egui::PointerButton::Primary) {
        if let Some(pos) = response.interact_pointer_pos() {
            state.gesture = Gesture::None;
            // ¿Sobre un manejador de la selección actual?
            if let Some(sel) = state.selection.primary() {
                if let Ok(layer) = state.doc.layer(sel) {
                    let t = layer.transform;
                    let corners = layer_corners_screen(&state.viewport, rect, &t);
                    let on_rotate = rotation_handle_screen(&state.viewport, rect, &t).distance(pos)
                        <= HANDLE_SIZE / 2.0 + 3.0;
                    if on_rotate {
                        let (px, py) = screen_to_page(&state.viewport, rect, pos);
                        let (cx, cy) = t.center();
                        let pointer_angle = (py - cy).atan2(px - cx).to_degrees();
                        state.gesture = Gesture::Rotate {
                            layer: sel,
                            start: t,
                            grab_offset: t.rotation - pointer_angle,
                        };
                    } else if let Some(corner) = corner_at(corners, pos) {
                        state.gesture = if state.crop_mode {
                            let start_crop = match &layer.content {
                                LayerContent::Image(c) => c.crop,
                                _ => None,
                            };
                            Gesture::Crop {
                                layer: sel,
                                corner,
                                start_t: t,
                                start_crop,
                                origin: pos,
                            }
                        } else {
                            Gesture::Resize {
                                layer: sel,
                                corner,
                                start: t,
                                origin: pos,
                            }
                        };
                    }
                }
            }
            // Si no, ¿sobre una capa? (selecciona y empieza a mover)
            if matches!(state.gesture, Gesture::None) {
                let (px, py) = screen_to_page(&state.viewport, rect, pos);
                let hit = state.doc.page().ok().and_then(|p| p.layer_at(px, py));
                if hit != state.selection.primary() {
                    state.crop_mode = false;
                }
                // Ctrl añade/quita de la selección, Shift extiende el tramo
                // de pila hasta la capa tocada; sin modificadores, la
                // selección se reemplaza entera (como un clic normal).
                let mods = ui.input(|i| i.modifiers);
                if mods.command {
                    if let Some(id) = hit {
                        state.selection.toggle(id);
                    }
                } else if mods.shift {
                    if let (Some(id), Ok(page)) = (hit, state.doc.page()) {
                        state.selection.extend_range(page, id);
                    }
                } else {
                    state.selection.set(hit);
                }
                if let Some(id) = hit {
                    if let Ok(layer) = state.doc.layer(id) {
                        state.gesture = Gesture::Move {
                            layer: id,
                            start: layer.transform,
                            origin: pos,
                        };
                    }
                }
            }
        }
    }

    // Gesto en curso: muta el documento en directo (sin comandos por frame),
    // siempre a partir del delta TOTAL desde el origen del gesto, inmune a
    // frames perdidos.
    if response.dragged_by(egui::PointerButton::Primary) {
        if let Some(pos) = response.interact_pointer_pos() {
            match state.gesture {
                Gesture::Move {
                    layer,
                    start,
                    origin,
                } => {
                    let (dx, dy) = (
                        f64::from(pos.x - origin.x) / state.viewport.zoom,
                        f64::from(pos.y - origin.y) / state.viewport.zoom,
                    );
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
                            let snap = snap_translation(
                                &moved,
                                &others,
                                page.width,
                                page.height,
                                threshold,
                            );
                            moved.x += snap.dx;
                            moved.y += snap.dy;
                            state.snap_guides = (snap.v_guides, snap.h_guides);
                        }
                    }
                    if let Ok(l) = state.doc.layer_mut(layer) {
                        l.transform.x = moved.x;
                        l.transform.y = moved.y;
                    }
                }
                Gesture::Resize {
                    layer,
                    corner,
                    start,
                    origin,
                } => {
                    let (dx, dy) = (
                        f64::from(pos.x - origin.x) / state.viewport.zoom,
                        f64::from(pos.y - origin.y) / state.viewport.zoom,
                    );
                    let shift = ui.ctx().input(|i| i.modifiers.shift);
                    let keep_aspect = state.aspect_lock != shift; // Shift invierte el candado
                    let t = resize_rotated_from_corner(&start, corner, dx, dy, keep_aspect, 1.0);
                    if let Ok(l) = state.doc.layer_mut(layer) {
                        l.transform = t;
                    }
                    // Dimensiones en píxeles junto al cursor mientras se arrastra.
                    show_drag_tag(ui, pos, format_dims(&t));
                }
                Gesture::Rotate {
                    layer,
                    start,
                    grab_offset,
                } => {
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
                Gesture::Crop {
                    layer,
                    corner,
                    start_t,
                    start_crop,
                    origin,
                } => {
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
                        if let LayerContent::Image(content) = &mut l.content {
                            content.crop = Some(crop);
                        }
                    }
                    show_drag_tag(ui, pos, format_dims(&t));
                }
                Gesture::None => {}
            }
        }
    }

    // Fin de gesto: consolida en UN comando de deshacer.
    if response.drag_stopped_by(egui::PointerButton::Primary) {
        state.snap_guides = (Vec::new(), Vec::new());
        match std::mem::replace(&mut state.gesture, Gesture::None) {
            Gesture::Move { layer, start, .. }
            | Gesture::Resize { layer, start, .. }
            | Gesture::Rotate { layer, start, .. } => {
                if let Ok(l) = state.doc.layer(layer) {
                    let after = l.transform;
                    if after != start {
                        state.push_undo_step(Box::new(SetTransform {
                            layer,
                            before: start,
                            after,
                        }));
                    }
                }
            }
            Gesture::Crop {
                layer,
                start_t,
                start_crop,
                ..
            } => {
                if let Ok(l) = state.doc.layer(layer) {
                    let after_t = l.transform;
                    let after_crop = match &l.content {
                        LayerContent::Image(content) => content.crop,
                        _ => None,
                    };
                    if after_t != start_t || after_crop != start_crop {
                        state.push_undo_step(Box::new(canvas_core::Composite::new(
                            "Recortar",
                            vec![
                                Box::new(SetTransform {
                                    layer,
                                    before: start_t,
                                    after: after_t,
                                }),
                                Box::new(SetCrop {
                                    layer,
                                    before: start_crop,
                                    after: after_crop,
                                }),
                            ],
                        )));
                    }
                }
            }
            Gesture::None => {}
        }
    }

    // Click sin arrastre: seleccionar / deseleccionar.
    if response.clicked_by(egui::PointerButton::Primary) {
        if let Some(pos) = response.interact_pointer_pos() {
            let (px, py) = screen_to_page(&state.viewport, rect, pos);
            let hit = state.doc.page().ok().and_then(|p| p.layer_at(px, py));
            if hit != state.selection.primary() {
                state.crop_mode = false;
            }
            let mods = ui.input(|i| i.modifiers);
            if mods.command {
                if let Some(id) = hit {
                    state.selection.toggle(id);
                }
            } else if mods.shift {
                if let (Some(id), Ok(page)) = (hit, state.doc.page()) {
                    state.selection.extend_range(page, id);
                }
            } else {
                state.selection.set(hit);
            }
        }
    }
}
