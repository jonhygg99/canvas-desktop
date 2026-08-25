//! Campos de posición/tamaño/rotación/recorte/alineación de la capa
//! seleccionada — comunes a cualquier tipo de contenido (el contenido en sí
//! se delega a `content::content_properties_ui`).

use canvas_core::{
    cover_transform, uncrop_transform, LayerContent, LayerId, SetCrop, SetTransform, Transform,
};
use eframe::egui;

use crate::app_icons::{
    draw_check_icon, draw_crop_icon, draw_double_arrow_icon, draw_fill_icon, draw_lock_icon,
    draw_target_icon, draw_triangle_icon, icon_button_ui, icon_text_button_ui, IconDir,
};
use crate::sidebar;

use super::content::content_properties_ui;
use super::effects::{blur_control, color_adjustments_ui, opacity_control, shadow_ui};
use super::EditorState;

/// Campos de posición/tamaño/escala y botones de alineación de una capa.
/// Cada propiedad va en su propio desplegable (`Size`, `Position`, etc.),
/// en vez de un único desplegable «Layer» que lo envolvía todo.
pub(super) fn layer_properties_ui(
    state: &mut EditorState,
    ui: &mut egui::Ui,
    sel: LayerId,
    (page_w, page_h): (f64, f64),
) {
    let Ok(layer) = state.doc.layer(sel) else {
        return;
    };

    // Un grupo no tiene geometría propia (su `transform` es una caja
    // envolvente derivada de sus hijos, recalculada por
    // `refresh_group_bounds`): posición/tamaño/rotación/recorte no
    // significan nada aquí. Solo la opacidad le afecta (y se hereda por la
    // cadena de grupos). Se gestiona desde el panel de capas.
    if matches!(layer.content, LayerContent::Group(_)) {
        let group_name = layer.name.clone();
        sidebar::section(ui, "Opacity", true, |ui| {
            opacity_control(state, ui, sel);
        });
        ui.label(format!("Group: {group_name}"));
        ui.weak("Manage its contents from the layers panel on the left.");
        return;
    }

    let original = layer.transform;
    let natural = match &layer.content {
        LayerContent::Image(img) => (f64::from(img.natural_width), f64::from(img.natural_height)),
        LayerContent::Svg(svg) => (f64::from(svg.natural_width), f64::from(svg.natural_height)),
        LayerContent::Text(_) | LayerContent::Shape(_) => (0.0, 0.0),
        LayerContent::Group(_) => unreachable!("ya se devolvió arriba para los grupos"),
    };
    let current_crop = match &layer.content {
        LayerContent::Image(img) => img.crop,
        _ => None,
    };
    let is_image = matches!(&layer.content, LayerContent::Image(_));
    let mut t = original;
    let mut changed = false;
    let mut commit = false;
    let mut track = |r: egui::Response| -> bool {
        let c = r.changed();
        // Consolida al soltar el arrastre del campo o al salir de él (Enter/Tab).
        if r.drag_stopped() || r.lost_focus() {
            commit = true;
        }
        c
    };
    // Flags para acciones de botón (reset/flip/align/crop): acumulan aquí y
    // se fusionan al final, para no pelear con el préstamo de `track`.
    let mut force_commit = false;
    let mut reset_crop = false;
    let mut aligned: Option<Transform> = None;

    // --- Tamaño ---
    sidebar::section(ui, "Size", true, |ui| {
        let locked = state.aspect_lock;
        ui.horizontal(|ui| {
            let (lock_rect, lock_resp) =
                ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::click());
            if lock_resp.hovered() {
                ui.painter()
                    .rect_filled(lock_rect, 4.0, ui.visuals().widgets.hovered.weak_bg_fill);
            }
            let lock_c = if locked {
                ui.visuals().strong_text_color()
            } else if lock_resp.hovered() {
                ui.visuals().widgets.active.text_color()
            } else {
                ui.visuals().widgets.inactive.text_color()
            };
            draw_lock_icon(ui.painter(), lock_rect, locked, lock_c);
            if lock_resp
                .on_hover_text("Locked aspect ratio (hold Shift while dragging to invert)")
                .clicked()
            {
                state.aspect_lock = !state.aspect_lock;
            }
        });
        let ratio = original.aspect_ratio();
        ui.horizontal(|ui| {
            ui.label("W");
            let before_w = t.width;
            if track(
                ui.add(
                    egui::DragValue::new(&mut t.width)
                        .speed(1.0)
                        .range(1.0..=f64::MAX)
                        .max_decimals(1),
                ),
            ) {
                changed = true;
                if state.aspect_lock && t.width != before_w {
                    t.height = (t.width / ratio).max(1.0);
                }
            }
            ui.label("H");
            let before_h = t.height;
            if track(
                ui.add(
                    egui::DragValue::new(&mut t.height)
                        .speed(1.0)
                        .range(1.0..=f64::MAX)
                        .max_decimals(1),
                ),
            ) {
                changed = true;
                if state.aspect_lock && t.height != before_h {
                    t.width = (t.height * ratio).max(1.0);
                }
            }
        });

        // Escala respecto al tamaño natural de la imagen.
        if natural.0 > 0.0 && natural.1 > 0.0 {
            let mut scale = t.width / natural.0 * 100.0;
            ui.horizontal(|ui| {
                ui.label("Scale");
                if track(
                    ui.add(
                        egui::DragValue::new(&mut scale)
                            .speed(0.5)
                            .range(0.1..=10_000.0)
                            .suffix(" %")
                            .max_decimals(1),
                    ),
                ) {
                    changed = true;
                    t.width = (natural.0 * scale / 100.0).max(1.0);
                    t.height = (natural.1 * scale / 100.0).max(1.0);
                }
            });
        }

        // Los campos de tamaño (W/H/Scale) escalan alrededor del centro, igual
        // que la rotación gira sobre él; anclar la esquina superior izquierda
        // hacía "caer" la capa hacia abajo-derecha al agrandarla.
        if t.width != original.width || t.height != original.height {
            let sized = canvas_core::resize_around_center(&original, t.width, t.height);
            t.x = sized.x;
            t.y = sized.y;
        }
    });

    // --- Posición ---
    sidebar::section(ui, "Position", true, |ui| {
        ui.horizontal(|ui| {
            ui.label("X");
            changed |= track(ui.add(egui::DragValue::new(&mut t.x).speed(1.0).max_decimals(1)));
            ui.label("Y");
            changed |= track(ui.add(egui::DragValue::new(&mut t.y).speed(1.0).max_decimals(1)));
        });
    });

    // --- Opacidad ---
    sidebar::section(ui, "Opacity", true, |ui| {
        opacity_control(state, ui, sel);
    });

    // --- Rotación y volteo ---
    sidebar::section(ui, "Rotation", true, |ui| {
        let mut reset_rotation = false;
        let mut flip_h = false;
        let mut flip_v = false;
        ui.horizontal(|ui| {
            ui.label("Rotation");
            if track(
                ui.add(
                    egui::DragValue::new(&mut t.rotation)
                        .speed(1.0)
                        .range(-180.0..=180.0)
                        .suffix("°")
                        .max_decimals(1),
                ),
            ) {
                changed = true;
            }
            reset_rotation = t.rotation != 0.0
                && ui
                    .small_button("0°")
                    .on_hover_text("Reset rotation")
                    .clicked();
            flip_h = icon_button_ui(ui, 18.0, true, |p, r, c| {
                draw_double_arrow_icon(p, r, true, true, c)
            })
            .on_hover_text("Flip horizontally")
            .clicked();
            flip_v = icon_button_ui(ui, 18.0, true, |p, r, c| {
                draw_double_arrow_icon(p, r, false, false, c)
            })
            .on_hover_text("Flip vertically")
            .clicked();
        });
        if reset_rotation {
            t.rotation = 0.0;
            changed = true;
            force_commit = true;
        }
        if flip_h {
            t.flip_h = !t.flip_h;
            changed = true;
            force_commit = true;
        }
        if flip_v {
            t.flip_v = !t.flip_v;
            changed = true;
            force_commit = true;
        }
    });

    // --- Contenido (texto / forma) ---
    content_properties_ui(state, ui, sel);

    // --- Recorte no destructivo (solo capas de imagen) ---
    if is_image {
        sidebar::section(ui, "Crop", true, |ui| {
            ui.horizontal(|ui| {
                let crop_resp = if state.crop_mode {
                    icon_text_button_ui(
                        ui,
                        true,
                        |p, r, c| draw_check_icon(p, r, c),
                        "Done",
                        None,
                        egui::Vec2::ZERO,
                    )
                } else {
                    icon_text_button_ui(
                        ui,
                        true,
                        |p, r, c| draw_crop_icon(p, r, c),
                        "Crop",
                        None,
                        egui::Vec2::ZERO,
                    )
                };
                if crop_resp
                    .on_hover_text("Drag the corner handles to trim the image; the pixels stay intact")
                    .clicked()
                {
                    state.crop_mode = !state.crop_mode;
                }
                if current_crop.is_some() && ui.button("Reset").clicked() {
                    reset_crop = true;
                }
            });
        });
    }

    // --- Desenfoque (no destructivo, vista previa en vivo) ---
    sidebar::section(ui, "Blur", true, |ui| {
        blur_control(state, ui, sel);
    });

    // --- Ajustes de color (GPU, no destructivos, vista previa en vivo) ---
    sidebar::section(ui, "Color", true, |ui| {
        color_adjustments_ui(state, ui, sel);
    });

    // --- Sombra proyectada ---
    sidebar::section(ui, "Shadow", true, |ui| {
        shadow_ui(state, ui, sel);
    });

    // --- Alineación respecto a la página ---
    sidebar::section(ui, "Align", true, |ui| {
        ui.horizontal(|ui| {
            if icon_text_button_ui(
                ui,
                true,
                |p, r, c| draw_triangle_icon(p, r, IconDir::Left, c),
                "Left",
                None,
                egui::Vec2::ZERO,
            )
            .clicked()
            {
                aligned = Some(canvas_core::align_horizontal(
                    &t,
                    page_w,
                    canvas_core::HAlign::Left,
                ));
            }
            if icon_text_button_ui(
                ui,
                true,
                |p, r, c| draw_double_arrow_icon(p, r, true, false, c),
                "Center",
                None,
                egui::Vec2::ZERO,
            )
            .clicked()
            {
                aligned = Some(canvas_core::align_horizontal(
                    &t,
                    page_w,
                    canvas_core::HAlign::Center,
                ));
            }
            if icon_text_button_ui(
                ui,
                true,
                |p, r, c| draw_triangle_icon(p, r, IconDir::Right, c),
                "Right",
                None,
                egui::Vec2::ZERO,
            )
            .clicked()
            {
                aligned = Some(canvas_core::align_horizontal(
                    &t,
                    page_w,
                    canvas_core::HAlign::Right,
                ));
            }
        });
        ui.horizontal(|ui| {
            if icon_text_button_ui(
                ui,
                true,
                |p, r, c| draw_triangle_icon(p, r, IconDir::Up, c),
                "Top",
                None,
                egui::Vec2::ZERO,
            )
            .clicked()
            {
                aligned = Some(canvas_core::align_vertical(
                    &t,
                    page_h,
                    canvas_core::VAlign::Top,
                ));
            }
            if icon_text_button_ui(
                ui,
                true,
                |p, r, c| draw_double_arrow_icon(p, r, false, false, c),
                "Middle",
                None,
                egui::Vec2::ZERO,
            )
            .clicked()
            {
                aligned = Some(canvas_core::align_vertical(
                    &t,
                    page_h,
                    canvas_core::VAlign::Middle,
                ));
            }
            if icon_text_button_ui(
                ui,
                true,
                |p, r, c| draw_triangle_icon(p, r, IconDir::Down, c),
                "Bottom",
                None,
                egui::Vec2::ZERO,
            )
            .clicked()
            {
                aligned = Some(canvas_core::align_vertical(
                    &t,
                    page_h,
                    canvas_core::VAlign::Bottom,
                ));
            }
        });
        if icon_text_button_ui(
            ui,
            true,
            |p, r, c| draw_target_icon(p, r, c),
            "Center on page",
            None,
            egui::Vec2::ZERO,
        )
        .clicked()
        {
            let centered = canvas_core::align_horizontal(&t, page_w, canvas_core::HAlign::Center);
            aligned = Some(canvas_core::align_vertical(
                &centered,
                page_h,
                canvas_core::VAlign::Middle,
            ));
        }
        if icon_text_button_ui(
            ui,
            true,
            |p, r, c| draw_fill_icon(p, r, c),
            "Cover the page",
            None,
            egui::Vec2::ZERO,
        )
        .on_hover_text("The image fills the whole page keeping its aspect ratio")
        .clicked()
        {
            aligned = Some(cover_transform(natural.0, natural.1, page_w, page_h));
        }
    });

    // --- Aplicar cambios ---
    if reset_crop {
        if let Some(crop) = current_crop {
            let before = state.panel_edit.take().map_or(original, |(_, b)| b);
            let restored = uncrop_transform(&before, crop);
            if let Err(e) = state.apply_undo_step(Box::new(canvas_core::Composite::new(
                "Reset crop",
                vec![
                    Box::new(SetTransform {
                        layer: sel,
                        before,
                        after: restored,
                    }),
                    Box::new(SetCrop {
                        layer: sel,
                        before: current_crop,
                        after: None,
                    }),
                ],
            ))) {
                tracing::error!("reset crop falló: {e}");
            }
        }
        return;
    }

    if let Some(after) = aligned {
        // Botón de alineación: comando inmediato (consolidando cualquier
        // edición de campo pendiente como parte del mismo paso).
        let before = state.panel_edit.take().map_or(original, |(_, b)| b);
        if after != before {
            if let Err(e) = state.apply_undo_step(Box::new(SetTransform {
                layer: sel,
                before,
                after,
            })) {
                tracing::error!("alinear falló: {e}");
            }
        }
        return;
    }

    if changed {
        if state.panel_edit.is_none() {
            state.panel_edit = Some((sel, original));
        }
        if let Ok(l) = state.doc.layer_mut(sel) {
            l.transform = t;
        }
    }
    if commit || force_commit {
        if let Some((id, before)) = state.panel_edit.take() {
            if let Ok(l) = state.doc.layer(id) {
                let after = l.transform;
                if after != before {
                    state.push_undo_step(Box::new(SetTransform {
                        layer: id,
                        before,
                        after,
                    }));
                }
            }
        }
    }
}
