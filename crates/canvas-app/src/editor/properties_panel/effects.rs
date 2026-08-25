//! Controles de efectos no destructivos de una capa: desenfoque, ajustes de
//! color y sombra proyectada. Cada uno consolida sus cambios en vivo en UN
//! paso de deshacer al soltar el control.

use canvas_core::LayerId;
use eframe::egui;

use super::EditorState;

/// Slider de opacidad de la capa (0-100 %), con consolidación en un solo paso
/// de deshacer al soltar.
///
/// `Layer::opacity` NO forma parte de `Effects`: el renderer la honra
/// estructuralmente con un `push_layer`, no con un shader, y se hereda por la
/// cadena de grupos (`Page::effective_opacity`). Por eso vale también para un
/// grupo, a diferencia del resto de esta sección.
///
/// A diferencia de `blur_control`, pinta su propia etiqueta: se dibuja antes
/// del retorno temprano de los grupos, así que tiene que poder no pintar nada
/// si la capa ya no existe.
pub(super) fn opacity_control(state: &mut EditorState, ui: &mut egui::Ui, target: LayerId) {
    let Ok(layer) = state.doc.layer(target) else {
        return;
    };
    let current = layer.opacity;
    let mut pct = current * 100.0;

    ui.label("Opacity");
    let r = ui.add(
        egui::Slider::new(&mut pct, 0.0..=100.0)
            .suffix(" %")
            .fixed_decimals(0),
    );
    if r.changed() {
        set_opacity_live(state, target, pct / 100.0);
    }
    if r.drag_stopped() || r.lost_focus() {
        commit_opacity(state);
    }
}

/// Aplica la opacidad EN VIVO durante el arrastre: muta el documento
/// directamente y recuerda el valor original la primera vez, para que todo el
/// gesto acabe siendo un solo paso de deshacer (mismo criterio que el resto de
/// gestos continuos del editor).
pub(super) fn set_opacity_live(state: &mut EditorState, target: LayerId, after: f32) {
    let after = after.clamp(0.0, 1.0);
    let Ok(layer) = state.doc.layer(target) else {
        return;
    };
    if (after - layer.opacity).abs() <= f32::EPSILON {
        return;
    }
    let original = layer.opacity;
    // Solo la PRIMERA vez del gesto: si no, cada frame del arrastre pisaría el
    // valor de partida y el deshacer devolvería al penúltimo, no al inicial.
    if state.opacity_edit.is_none() {
        state.opacity_edit = Some((target, original));
    }
    if let Ok(l) = state.doc.layer_mut(target) {
        l.opacity = after;
    }
}

/// Consolida el ajuste de opacidad en curso en UN paso de deshacer. Lo llaman
/// el propio slider al soltarlo y el panel cuando cambia la capa seleccionada
/// sin haberlo soltado.
pub(super) fn commit_opacity(state: &mut EditorState) {
    let Some((id, before)) = state.opacity_edit.take() else {
        return;
    };
    let after = state.doc.layer(id).map(|l| l.opacity).unwrap_or(before);
    if (after - before).abs() > f32::EPSILON {
        state.push_undo_step(Box::new(canvas_core::SetOpacity {
            layer: id,
            before,
            after,
        }));
    }
}

/// Slider de desenfoque (no destructivo) de una capa, con consolidación en un
/// solo paso de deshacer al soltar. Se usa tanto en la sección de la capa
/// seleccionada como junto al checkbox del fondo desenfocado.
pub(super) fn blur_control(state: &mut EditorState, ui: &mut egui::Ui, target: LayerId) {
    let current_blur = state
        .doc
        .layer(target)
        .map(|l| l.effects.blur_radius)
        .unwrap_or(0.0);
    let mut blur = current_blur;
    ui.horizontal(|ui| {
        let r = ui.add(
            egui::Slider::new(&mut blur, 0.0..=100.0)
                .suffix(" px")
                .fixed_decimals(0),
        );
        if r.changed() && blur != current_blur {
            if state.blur_edit.is_none() {
                state.blur_edit = Some((target, current_blur));
            }
            if let Ok(l) = state.doc.layer_mut(target) {
                l.effects.blur_radius = blur;
            }
        }
        if r.drag_stopped() || r.lost_focus() {
            if let Some((id, before)) = state.blur_edit.take() {
                let after = state
                    .doc
                    .layer(id)
                    .map(|l| l.effects.blur_radius)
                    .unwrap_or(before);
                if (after - before).abs() > f32::EPSILON {
                    state.push_undo_step(Box::new(canvas_core::SetBlur {
                        layer: id,
                        before,
                        after,
                    }));
                }
            }
        }
        if current_blur > 0.0 && ui.button("Remove").clicked() {
            if let Err(e) = state.apply_undo_step(Box::new(canvas_core::SetBlur {
                layer: target,
                before: current_blur,
                after: 0.0,
            })) {
                tracing::error!("quitar desenfoque falló: {e}");
            }
        }
    });
}

/// Sliders de ajuste de color de una capa (brillo, contraste, saturación,
/// temperatura, grises, sepia). Preview en vivo por GPU y consolidación de
/// todos los sliders en UN paso de deshacer al soltar.
pub(super) fn color_adjustments_ui(state: &mut EditorState, ui: &mut egui::Ui, sel: LayerId) {
    let Ok(layer) = state.doc.layer(sel) else {
        return;
    };
    let original = layer.effects;
    let mut fx = original;
    let mut changed = false;
    let mut commit = false;
    let mut reset = false;

    let mut slider =
        |ui: &mut egui::Ui, label: &str, value: &mut f32, range: std::ops::RangeInclusive<f32>| {
            // Etiqueta encima y el slider a ancho completo debajo.
            ui.label(label);
            let mut pct = *value * 100.0;
            let r = ui.add(
                egui::Slider::new(&mut pct, *range.start() * 100.0..=*range.end() * 100.0)
                    .suffix(" %")
                    .fixed_decimals(0),
            );
            *value = pct / 100.0;
            if r.changed() {
                changed = true;
            }
            if r.drag_stopped() || r.lost_focus() {
                commit = true;
            }
        };
    slider(ui, "Brightness", &mut fx.brightness, -1.0..=1.0);
    slider(ui, "Contrast", &mut fx.contrast, -1.0..=1.0);
    slider(ui, "Saturation", &mut fx.saturation, -1.0..=1.0);
    slider(ui, "Temperature", &mut fx.temperature, -1.0..=1.0);
    slider(ui, "Grayscale", &mut fx.grayscale, 0.0..=1.0);
    slider(ui, "Sepia", &mut fx.sepia, 0.0..=1.0);
    if original.has_color_adjustments() && ui.small_button("Reset adjustments").clicked() {
        reset = true;
    }

    if reset {
        let mut neutral = original;
        neutral.brightness = 0.0;
        neutral.contrast = 0.0;
        neutral.saturation = 0.0;
        neutral.temperature = 0.0;
        neutral.grayscale = 0.0;
        neutral.sepia = 0.0;
        let before = state.color_edit.take().map_or(original, |(_, b)| b);
        if let Err(e) = state.apply_undo_step(Box::new(canvas_core::SetEffects {
            layer: sel,
            before,
            after: neutral,
        })) {
            tracing::error!("reset de ajustes falló: {e}");
        }
        return;
    }

    if changed && fx != original {
        if state.color_edit.is_none() {
            state.color_edit = Some((sel, original));
        }
        if let Ok(l) = state.doc.layer_mut(sel) {
            l.effects = fx;
        }
    }
    if commit {
        if let Some((id, before)) = state.color_edit.take() {
            let after = state.doc.layer(id).map(|l| l.effects).unwrap_or(before);
            if after != before {
                state.push_undo_step(Box::new(canvas_core::SetEffects {
                    layer: id,
                    before,
                    after,
                }));
            }
        }
    }
}

/// Checkbox y controles de la sombra proyectada de una capa.
pub(super) fn shadow_ui(state: &mut EditorState, ui: &mut egui::Ui, sel: LayerId) {
    let current = state.doc.layer(sel).ok().and_then(|l| l.effects.shadow);

    let mut enabled = current.is_some();
    if ui.checkbox(&mut enabled, "Shadow").changed() {
        let after = enabled.then(canvas_core::Shadow::default);
        if let Err(e) = state.apply_undo_step(Box::new(canvas_core::SetShadow {
            layer: sel,
            before: current,
            after,
        })) {
            tracing::error!("sombra falló: {e}");
        }
        return;
    }

    let Some(shadow) = current else { return };
    let mut sh = shadow;
    let mut changed = false;
    let mut commit = false;
    let mut track = |r: egui::Response| {
        if r.changed() {
            changed = true;
        }
        if r.drag_stopped() || r.lost_focus() {
            commit = true;
        }
    };

    ui.horizontal(|ui| {
        ui.label("Offset");
        track(
            ui.add(
                egui::DragValue::new(&mut sh.offset_x)
                    .speed(1.0)
                    .range(-500.0..=500.0)
                    .prefix("X ")
                    .max_decimals(0),
            ),
        );
        track(
            ui.add(
                egui::DragValue::new(&mut sh.offset_y)
                    .speed(1.0)
                    .range(-500.0..=500.0)
                    .prefix("Y ")
                    .max_decimals(0),
            ),
        );
    });
    ui.horizontal(|ui| {
        ui.label("Softness");
        track(
            ui.add(
                egui::Slider::new(&mut sh.blur, 0.0..=100.0)
                    .suffix(" px")
                    .fixed_decimals(0),
            ),
        );
    });
    ui.horizontal(|ui| {
        ui.label("Opacity");
        let mut pct = sh.opacity * 100.0;
        track(
            ui.add(
                egui::Slider::new(&mut pct, 0.0..=100.0)
                    .suffix(" %")
                    .fixed_decimals(0),
            ),
        );
        sh.opacity = pct / 100.0;
    });

    if changed && sh != shadow {
        if state.shadow_edit.is_none() {
            state.shadow_edit = Some((sel, current));
        }
        if let Ok(l) = state.doc.layer_mut(sel) {
            l.effects.shadow = Some(sh);
        }
    }
    if commit {
        if let Some((id, before)) = state.shadow_edit.take() {
            let after = state.doc.layer(id).ok().and_then(|l| l.effects.shadow);
            if after != before {
                state.push_undo_step(Box::new(canvas_core::SetShadow {
                    layer: id,
                    before,
                    after,
                }));
            }
        }
    }
}
