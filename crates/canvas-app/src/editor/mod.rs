//! Estado y UI del editor: el lienzo con zoom/paneo y el panel de propiedades.

use std::sync::mpsc::Sender;

use canvas_core::{
    cover_transform, resize_rotated_from_corner, snap_translation, trim_crop_from_corner,
    uncrop_transform, Corner, CropRect, Document, LayerContent, LayerId, SetCrop, SetPageSize,
    SetTransform, Transform,
};
use canvas_render::{CanvasRenderer, FxScope, ImageMap};
use eframe::egui;
use eframe::egui_wgpu::RenderState;
use vello::kurbo::Affine;

use crate::deck::{Deck, DeckAxis, DeckRect, MoveDir, Slot, SlotContent};
use crate::gallery::ItemKind;
use crate::loader::{self, AppMsg};
use crate::surface::CanvasSurface;

mod layer_ops;
mod viewport;

use layer_ops::{apply_alignment, reorder_layer, sibling_position, ZOrder};
pub use viewport::Viewport;
use viewport::{
    layer_corners_screen, page_to_screen, rotation_handle_screen, screen_to_page, AutoFit,
};

/// Gesto de edición en curso sobre el lienzo. El documento se muta en directo
/// durante el gesto y al soltarlo se consolida en UN comando de deshacer.
enum Gesture {
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

mod state;

pub use state::{DeckNav, EditorState};
pub(crate) use state::{DeleteRecord, GlobalStep};

/// Panel derecho: propiedades de la capa seleccionada.
pub fn properties_ui(state: &mut EditorState, ui: &mut egui::Ui) {
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            properties_ui_inner(state, ui);
        });
}

fn properties_ui_inner(state: &mut EditorState, ui: &mut egui::Ui) {
    ui.add_space(8.0);

    // Banner: el archivo cambió en disco fuera de la app.
    if state.external_change {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            "⚠ This file changed on disk outside Canvas Desktop.",
        );
        ui.horizontal(|ui| {
            if ui.button("Reload").clicked() {
                state.reload_requested = true;
            }
            if ui.button("Keep mine").clicked() {
                state.external_change = false;
            }
        });
        ui.separator();
    }

    if state.from_gallery.is_some() && ui.button("⏴ Back to gallery").clicked() {
        state.return_requested = true;
    }
    file_name_ui(state, ui);
    let page_dims = match state.doc.page() {
        Ok(p) => (p.width, p.height),
        Err(_) => (0.0, 0.0),
    };
    ui.weak(format!(
        "{} × {} px",
        page_dims.0 as i64, page_dims.1 as i64
    ));
    ui.separator();

    page_ui(state, ui);
    ui.separator();

    ui.label("Insert");
    ui.horizontal(|ui| {
        if ui.button("T Text").clicked() {
            state.insert_layer_centered(
                "Text",
                500.0,
                120.0,
                LayerContent::Text(canvas_core::TextContent::default()),
            );
        }
        if ui.button("R").on_hover_text("Rectangle").clicked() {
            state.insert_layer_centered(
                "Rectangle",
                320.0,
                220.0,
                LayerContent::Shape(canvas_core::ShapeContent::default()),
            );
        }
        if ui.button("O").on_hover_text("Ellipse").clicked() {
            state.insert_layer_centered(
                "Ellipse",
                280.0,
                280.0,
                LayerContent::Shape(canvas_core::ShapeContent {
                    kind: canvas_core::ShapeKind::Ellipse,
                    ..Default::default()
                }),
            );
        }
        if ui.button("L").on_hover_text("Line").clicked() {
            state.insert_layer_centered(
                "Line",
                400.0,
                24.0,
                LayerContent::Shape(canvas_core::ShapeContent {
                    kind: canvas_core::ShapeKind::Line,
                    stroke: [30, 30, 30, 255],
                    stroke_width: 6.0,
                    ..Default::default()
                }),
            );
        }
    });
    ui.separator();

    if let Some(sel) = state.selection.primary() {
        if state.doc.layer(sel).is_ok() {
            layer_properties_ui(state, ui, sel, page_dims);
        }
    } else {
        ui.weak("No layer selected.");
        ui.weak("Click the image to select it.");
    }

    ui.separator();
    ui.horizontal(|ui| {
        let dirty_mark = if state.is_dirty() { " •" } else { "" };
        if ui
            .add_enabled(
                !state.saving,
                egui::Button::new(format!("💾 Save{dirty_mark}")),
            )
            .clicked()
        {
            state.save_clicked = true;
        }
        if ui
            .add_enabled(!state.saving, egui::Button::new("Save as…"))
            .clicked()
        {
            state.save_as_clicked = true;
        }
        // Va a la Papelera de reciclaje (`trash::delete`), no borrado
        // permanente: recuperable si el usuario se equivoca, así que no
        // hace falta pedir confirmación aparte.
        if ui
            .add_enabled(
                !state.saving,
                egui::Button::new(
                    egui::RichText::new("Delete").color(egui::Color32::from_rgb(220, 70, 70)),
                ),
            )
            .clicked()
        {
            state.delete_requested = true;
        }
    });
    if state.is_design {
        ui.weak("Design file (.canvas) — layers are always kept.");
    } else {
        ui.checkbox(&mut state.sidecar_enabled, "Editable sidecar (.canvas)")
            .on_hover_text(
                "Writes a .canvas file next to the image so the layers stay \
                 editable when you reopen it. Turn it off if you don't want \
                 extra files in your folders.",
            );
    }
    if state.saving {
        ui.horizontal(|ui| {
            ui.add(egui::Spinner::new());
            ui.label("Saving…");
        });
    }
    if let Some(error) = state.save_error.clone() {
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(ui.visuals().error_fg_color, &error);
            if ui.small_button("✕").clicked() {
                state.save_error = None;
            }
        });
    }
    ui.label(format!("Zoom: {:.0} %", state.viewport.zoom * 100.0));
    ui.weak("Wheel: pan · Shift+wheel: pan the other axis · Ctrl+wheel: zoom");
    ui.weak("Space/middle button: pan · Ctrl+0: fit");
    ui.weak("Ctrl+S: save · Ctrl+Shift+S: save as");
    ui.weak("Ctrl+C / Ctrl+V: copy layers, even between designs");
    ui.add_space(4.0);
    if ui.small_button("⚙ Settings").clicked() {
        state.settings_clicked = true;
    }
}

/// Nombre del archivo abierto, arriba del panel: un lápiz lo vuelve editable
/// in-place (mismo patrón que el renombrado de la galería —
/// `gallery::gallery_cell` — y el de capas — `rename_edit_ui` más abajo)
/// cuando el documento ya tiene archivo en disco. Un diseño nuevo sin
/// guardar (`source_path` en `None`) no ofrece el lápiz: no hay nada que
/// renombrar todavía.
fn file_name_ui(state: &mut EditorState, ui: &mut egui::Ui) {
    let id = egui::Id::new("editor_file_rename");
    if state.file_rename_edit.is_some() {
        // Mismo patrón que `gallery::gallery_cell`: Escape cancela, perder
        // el foco confirma (comprobado antes que `lost_focus` para que el
        // propio Escape no dispare un commit).
        let mut cancel = false;
        let mut commit = false;
        if let Some(text) = state.file_rename_edit.as_mut() {
            let resp = ui.add(egui::TextEdit::singleline(text).id(id));
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                cancel = true;
            } else if resp.lost_focus() {
                commit = true;
            }
        }
        if cancel {
            state.file_rename_edit = None;
        } else if commit {
            if let Some(text) = state.file_rename_edit.take() {
                let new_stem = text.trim().to_owned();
                let original_stem = state
                    .doc
                    .source_path
                    .as_deref()
                    .and_then(|p| p.file_stem())
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                if !new_stem.is_empty() && new_stem != original_stem {
                    state.file_rename_requested = Some(new_stem);
                }
            }
        }
    } else {
        ui.horizontal(|ui| {
            ui.heading(state.file_name());
            if state.doc.source_path.is_some() && ui.small_button("✏").clicked() {
                let stem = state
                    .doc
                    .source_path
                    .as_deref()
                    .and_then(|p| p.file_stem())
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                state.file_rename_edit = Some(stem);
                ui.memory_mut(|m| m.request_focus(id));
            }
        });
    }
}

/// Sección «Página»: resolución (campos + presets) y fondo desenfocado.
fn page_ui(state: &mut EditorState, ui: &mut egui::Ui) {
    let Ok(page) = state.doc.page() else { return };
    let original = (page.width, page.height);
    let mut w = original.0;
    let mut h = original.1;
    let mut changed = false;
    let mut commit = false;

    ui.label("Page");
    ui.horizontal(|ui| {
        ui.label("W");
        let rw = ui.add(
            egui::DragValue::new(&mut w)
                .speed(2.0)
                .range(16.0..=16384.0)
                .max_decimals(0),
        );
        ui.label("H");
        let rh = ui.add(
            egui::DragValue::new(&mut h)
                .speed(2.0)
                .range(16.0..=16384.0)
                .max_decimals(0),
        );
        changed |= rw.changed() || rh.changed();
        commit |= rw.drag_stopped() || rw.lost_focus() || rh.drag_stopped() || rh.lost_focus();

        // Presets rápidos de resolución.
        let image_size = state.doc.page().ok().and_then(|p| {
            p.layers.iter().rev().find_map(|l| match &l.content {
                LayerContent::Image(img) if Some(l.id) != state.background_layer => {
                    Some((f64::from(img.natural_width), f64::from(img.natural_height)))
                }
                _ => None,
            })
        });
        if page_size_presets_ui(ui, &mut w, &mut h, image_size) {
            changed = true;
            commit = true;
        }
    });

    if changed
        && (w, h)
            != (state
                .doc
                .page()
                .map(|p| (p.width, p.height))
                .unwrap_or(original))
    {
        if state.page_edit.is_none() {
            state.page_edit = Some(original);
        }
        if let Ok(page) = state.doc.page_mut() {
            page.width = w.max(16.0);
            page.height = h.max(16.0);
        }
    }
    if commit {
        if let Some(before) = state.page_edit.take() {
            let after = state
                .doc
                .page()
                .map(|p| (p.width, p.height))
                .unwrap_or(before);
            if after != before {
                // El fondo desenfocado (si lo hay) se recoloca para seguir
                // cubriendo la página nueva, todo en UN paso de deshacer.
                let mut commands: Vec<Box<dyn canvas_core::Command>> =
                    vec![Box::new(SetPageSize { before, after })];
                if let Some(cmd) = state.resync_background_cover() {
                    commands.push(cmd);
                }
                state.push_undo_step(Box::new(canvas_core::Composite::new(
                    "Resize page",
                    commands,
                )));
            }
        }
    }

    // Fondo desenfocado: copia «cover» de la imagen, con blur 50 por defecto.
    let active = state.background_active();
    let can_toggle = active || state.background_source().is_some();
    let mut bg_on = active;
    let response = ui.add_enabled(
        can_toggle,
        egui::Checkbox::new(&mut bg_on, "Blurred background"),
    );
    if response.changed() && bg_on != active {
        state.set_blurred_background(bg_on);
    }
    if active {
        if let Some(id) = state.background_layer {
            blur_control(state, ui, id);
        }
    }
}

/// Selector compartido para la página y el cuadro contextual Size.
/// Devuelve `true` cuando el usuario eligió un tamaño.
fn page_size_presets_ui(
    ui: &mut egui::Ui,
    w: &mut f64,
    h: &mut f64,
    image_size: Option<(f64, f64)>,
) -> bool {
    let mut selected = false;
    egui::ComboBox::from_id_salt("page_presets")
        .selected_text("Presets")
        .width(72.0)
        .show_ui(ui, |ui| {
            let mut preset = |ui: &mut egui::Ui, label: &str, pw: f64, ph: f64| {
                if ui.selectable_label(false, label).clicked() {
                    *w = pw;
                    *h = ph;
                    selected = true;
                }
            };

            ui.strong("Social");
            preset(
                ui,
                "Vertical / Reels / Shorts (1080 × 1920)",
                1080.0,
                1920.0,
            );
            preset(ui, "Square / Facebook 1:1 (1080 × 1080)", 1080.0, 1080.0);
            preset(
                ui,
                "Instagram portrait / Facebook feed (1080 × 1350)",
                1080.0,
                1350.0,
            );
            preset(
                ui,
                "LinkedIn / Facebook landscape (1200 × 628)",
                1200.0,
                628.0,
            );
            preset(ui, "Pinterest vertical (1000 × 1500)", 1000.0, 1500.0);

            ui.separator();
            ui.strong("Branding");
            preset(ui, "YouTube channel logo (800 × 800)", 800.0, 800.0);
            preset(ui, "Facebook page profile (320 × 320)", 320.0, 320.0);
            preset(ui, "Facebook page cover (851 × 315)", 851.0, 315.0);

            ui.separator();
            ui.strong("Video");
            preset(ui, "Video Full HD (1920 × 1080)", 1920.0, 1080.0);
            preset(ui, "Video 4K (3840 × 2160)", 3840.0, 2160.0);
            preset(ui, "YouTube thumbnail (1280 × 720)", 1280.0, 720.0);
            preset(ui, "YouTube banner (2560 × 1440)", 2560.0, 1440.0);

            if let Some((iw, ih)) = image_size {
                ui.separator();
                ui.strong("Source");
                let label = format!("Image ({} × {})", iw as i64, ih as i64);
                preset(ui, &label, iw, ih);
            }
        });
    selected
}
/// Ventanita flotante "Size" pedida desde el menú contextual del lienzo
/// (`canvas_ui`, botón "Size"): un formulario aparte con W/H en vez de
/// arrastrar el `DragValue` del panel — mismo commit que `page_ui` (un solo
/// paso de deshacer, con el fondo desenfocado recolocado si lo hay). Apply
/// confirma, Cancel (o la X) cierra sin tocar el documento.
fn size_popup_ui(state: &mut EditorState, ctx: &egui::Context) {
    let Some((mut w, mut h)) = state.size_popup else {
        return;
    };
    let mut open = true;
    let mut apply = false;
    let mut cancel = false;
    egui::Window::new("Page size")
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("W");
                ui.add(
                    egui::DragValue::new(&mut w)
                        .speed(2.0)
                        .range(16.0..=16384.0)
                        .max_decimals(0),
                );
                ui.label("H");
                ui.add(
                    egui::DragValue::new(&mut h)
                        .speed(2.0)
                        .range(16.0..=16384.0)
                        .max_decimals(0),
                );
                ui.add_space(8.0);
                page_size_presets_ui(ui, &mut w, &mut h, None);
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("Apply").clicked() {
                    apply = true;
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        });
    state.size_popup = if open && !cancel { Some((w, h)) } else { None };
    if !apply {
        return;
    }
    state.size_popup = None;
    let original = state
        .doc
        .page()
        .map(|p| (p.width, p.height))
        .unwrap_or((w, h));
    let (w, h) = (w.max(16.0), h.max(16.0));
    if (w, h) == original {
        return;
    }
    if let Ok(page) = state.doc.page_mut() {
        page.width = w;
        page.height = h;
    }
    let mut commands: Vec<Box<dyn canvas_core::Command>> = vec![Box::new(SetPageSize {
        before: original,
        after: (w, h),
    })];
    if let Some(cmd) = state.resync_background_cover() {
        commands.push(cmd);
    }
    state.push_undo_step(Box::new(canvas_core::Composite::new(
        "Resize page",
        commands,
    )));
}

/// Slider de desenfoque (no destructivo) de una capa, con consolidación en un
/// solo paso de deshacer al soltar. Se usa tanto en la sección de la capa
/// seleccionada como junto al checkbox del fondo desenfocado.
fn blur_control(state: &mut EditorState, ui: &mut egui::Ui, target: LayerId) {
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
fn color_adjustments_ui(state: &mut EditorState, ui: &mut egui::Ui, sel: LayerId) {
    let Ok(layer) = state.doc.layer(sel) else {
        return;
    };
    let original = layer.effects;
    let mut fx = original;
    let mut changed = false;
    let mut commit = false;
    let mut reset = false;

    ui.label("Adjustments");
    let mut slider =
        |ui: &mut egui::Ui, label: &str, value: &mut f32, range: std::ops::RangeInclusive<f32>| {
            ui.horizontal(|ui| {
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
            });
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
fn shadow_ui(state: &mut EditorState, ui: &mut egui::Ui, sel: LayerId) {
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

/// Campos de posición/tamaño/escala y botones de alineación de una capa.
fn layer_properties_ui(
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
    // significan nada aquí. Se gestiona desde el panel de capas.
    if matches!(layer.content, LayerContent::Group(_)) {
        ui.label(format!("Group: {}", layer.name));
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

    // --- Posición ---
    ui.label("Position");
    ui.horizontal(|ui| {
        ui.label("X");
        changed |= track(ui.add(egui::DragValue::new(&mut t.x).speed(1.0).max_decimals(1)));
        ui.label("Y");
        changed |= track(ui.add(egui::DragValue::new(&mut t.y).speed(1.0).max_decimals(1)));
    });

    // --- Rotación y volteo ---
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
        flip_h = ui
            .small_button("⇋")
            .on_hover_text("Flip horizontally")
            .clicked();
        flip_v = ui
            .small_button("⇅")
            .on_hover_text("Flip vertically")
            .clicked();
    });
    // `track` retiene prestado `commit` hasta su último uso: los botones
    // acumulan en un flag aparte que se fusiona al final.
    let mut force_commit = false;
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

    ui.add_space(6.0);

    // --- Tamaño ---
    ui.horizontal(|ui| {
        ui.label("Size");
        let lock_icon = if state.aspect_lock { "🔒" } else { "🔓" };
        if ui
            .selectable_label(state.aspect_lock, lock_icon)
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

    // --- Escala respecto al tamaño natural de la imagen ---
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

    ui.add_space(8.0);

    // --- Contenido (texto / forma) ---
    content_properties_ui(state, ui, sel);

    // --- Recorte no destructivo (solo capas de imagen) ---
    let mut reset_crop = false;
    if is_image {
        ui.label("Crop");
        ui.horizontal(|ui| {
            let label = if state.crop_mode {
                "✔ Done"
            } else {
                "✂ Crop"
            };
            if ui
                .button(label)
                .on_hover_text("Drag the corner handles to trim the image; the pixels stay intact")
                .clicked()
            {
                state.crop_mode = !state.crop_mode;
            }
            if current_crop.is_some() && ui.button("Reset").clicked() {
                reset_crop = true;
            }
        });
    }

    ui.add_space(8.0);

    // --- Desenfoque (no destructivo, vista previa en vivo) ---
    ui.label("Blur");
    blur_control(state, ui, sel);

    ui.add_space(8.0);

    // --- Ajustes de color (GPU, no destructivos, vista previa en vivo) ---
    color_adjustments_ui(state, ui, sel);

    ui.add_space(8.0);

    // --- Sombra proyectada ---
    shadow_ui(state, ui, sel);

    ui.add_space(8.0);

    // --- Alineación respecto a la página ---
    ui.label("Align to page");
    let mut aligned: Option<Transform> = None;
    ui.horizontal(|ui| {
        if ui.button("⏴ Left").clicked() {
            aligned = Some(canvas_core::align_horizontal(
                &t,
                page_w,
                canvas_core::HAlign::Left,
            ));
        }
        if ui.button("↔ Center").clicked() {
            aligned = Some(canvas_core::align_horizontal(
                &t,
                page_w,
                canvas_core::HAlign::Center,
            ));
        }
        if ui.button("Right ⏵").clicked() {
            aligned = Some(canvas_core::align_horizontal(
                &t,
                page_w,
                canvas_core::HAlign::Right,
            ));
        }
    });
    ui.horizontal(|ui| {
        if ui.button("⏶ Top").clicked() {
            aligned = Some(canvas_core::align_vertical(
                &t,
                page_h,
                canvas_core::VAlign::Top,
            ));
        }
        if ui.button("↕ Middle").clicked() {
            aligned = Some(canvas_core::align_vertical(
                &t,
                page_h,
                canvas_core::VAlign::Middle,
            ));
        }
        if ui.button("Bottom ⏷").clicked() {
            aligned = Some(canvas_core::align_vertical(
                &t,
                page_h,
                canvas_core::VAlign::Bottom,
            ));
        }
    });
    if ui.button("◎ Center on page").clicked() {
        let centered = canvas_core::align_horizontal(&t, page_w, canvas_core::HAlign::Center);
        aligned = Some(canvas_core::align_vertical(
            &centered,
            page_h,
            canvas_core::VAlign::Middle,
        ));
    }
    if ui
        .button("⛶ Cover the page")
        .on_hover_text("The image fills the whole page keeping its aspect ratio")
        .clicked()
    {
        aligned = Some(cover_transform(natural.0, natural.1, page_w, page_h));
    }

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

/// Acción pedida desde la cabecera de un lienzo (área central) que necesita
/// tocar disco (duplicar/borrar) o reconciliarse con el nombre real del
/// archivo (renombrar) — `canvas_ui` la arma pero no la ejecuta; se resuelve
/// en `main.rs`, mismo espíritu que `StripAction` desde la tira.
pub enum CanvasAction {
    Rename(u64, String),
    Duplicate(u64),
    Delete(u64),
    ReplaceFromLocal(LayerId),
    ReplaceFromUrl(LayerId, String),
    /// Elegido en el menú contextual (clic derecho) del propio lienzo —
    /// reutiliza el mismo `MenuAction` que ya resuelve la barra de menú
    /// nativa/de respaldo, sin duplicar esa lógica.
    Menu(crate::menus::MenuAction),
}

fn replace_url_popup_ui(state: &mut EditorState, ctx: &egui::Context) -> Option<CanvasAction> {
    let (layer, mut url) = state.replace_url_popup.take()?;
    let mut open = true;
    let mut replace = false;
    let mut cancel = false;
    egui::Window::new("Replace from URL")
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut url)
                    .hint_text("https://example.com/image.jpg")
                    .desired_width(360.0),
            );
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!url.trim().is_empty(), egui::Button::new("Replace"))
                    .clicked()
                {
                    replace = true;
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        });

    if replace {
        Some(CanvasAction::ReplaceFromUrl(layer, url.trim().to_owned()))
    } else {
        if open && !cancel {
            state.replace_url_popup = Some((layer, url));
        }
        None
    }
}
/// El lienzo: gestiona zoom/paneo, carga perezosa/descarte de la baraja, y
/// renderiza en una sola escena todos los lienzos visibles (el activo con
/// `state.doc`/`state.images`; el resto con su propio `SlotDoc`).
#[allow(clippy::too_many_arguments)]
pub fn canvas_ui(
    state: &mut EditorState,
    deck: &mut Deck,
    ui: &mut egui::Ui,
    rs: &RenderState,
    renderer: &mut CanvasRenderer,
    surface_slot: &mut Option<CanvasSurface>,
    tx: &Sender<AppMsg>,
    // Extensión de `settings.new_canvas_format` — qué crea la zona "+" al
    // final de la baraja cuando se pulsa directamente sobre el lienzo.
    new_canvas_ext: &str,
) -> Option<CanvasAction> {
    // Duplicar/borrar/renombrar tocan disco o el watcher: se arman aquí (en
    // la cabecera de un lienzo, ver más abajo) pero se resuelven en
    // `main.rs`, igual que `StripAction` desde la tira.
    let mut action: Option<CanvasAction> = None;

    let avail = ui.available_size();
    let (rect, response) = ui.allocate_exact_size(avail, egui::Sense::click_and_drag());

    // Menú contextual (clic derecho): antes no había ninguno en el área de
    // edición. Solo las acciones que de verdad se usan desde un clic
    // derecho — no una copia entera del menú Edit (eso ya está a un atajo
    // de teclado o al menú superior de distancia).
    response.context_menu(|ui| {
        use crate::menus::MenuAction;
        let mut item = |ui: &mut egui::Ui, label: &str, enabled: bool, a: MenuAction| {
            if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
                action = Some(CanvasAction::Menu(a));
                ui.close();
            }
        };
        item(ui, "Undo", state.can_undo(), MenuAction::Undo);
        item(ui, "Redo", state.can_redo(), MenuAction::Redo);
        ui.separator();
        item(ui, "Cut", true, MenuAction::Cut);
        item(ui, "Copy", true, MenuAction::Copy);
        item(ui, "Paste", true, MenuAction::Paste);
        item(ui, "Duplicate", true, MenuAction::Duplicate);
        item(ui, "Delete", true, MenuAction::Delete);
        ui.separator();
        item(ui, "Select All", true, MenuAction::SelectAll);
        item(ui, "Group", true, MenuAction::Group);
        item(ui, "Ungroup", true, MenuAction::Ungroup);
        let selected_image = state.selection.primary().filter(|id| {
            state
                .doc
                .layer(*id)
                .ok()
                .is_some_and(|l| matches!(l.content, LayerContent::Image(_)))
        });
        let design_sources: Vec<(LayerId, String)> = selected_image
            .and_then(|target| {
                state.doc.page().ok().map(|page| {
                    page.layers
                        .iter()
                        .filter(|layer| {
                            layer.id != target
                                && matches!(layer.content, LayerContent::Image(_))
                                && state.images.contains_key(&layer.id)
                        })
                        .map(|layer| (layer.id, layer.name.clone()))
                        .collect()
                })
            })
            .unwrap_or_default();
        ui.add_enabled_ui(selected_image.is_some(), |ui| {
            ui.menu_button("Replace", |ui| {
                let Some(target) = selected_image else {
                    return;
                };

                ui.menu_button("From this design", |ui| {
                    if design_sources.is_empty() {
                        ui.add_enabled(false, egui::Button::new("No other images"));
                    }
                    for (source, name) in &design_sources {
                        if ui.button(name).clicked() {
                            if let Err(e) = state.replace_image_from_layer(target, *source) {
                                state.save_error = Some(e);
                            }
                            ui.close();
                        }
                    }
                });

                if ui.button("From local file").clicked() {
                    action = Some(CanvasAction::ReplaceFromLocal(target));
                    ui.close();
                }
                if ui.button("From internet URL").clicked() {
                    state.replace_url_popup = Some((target, String::new()));
                    ui.close();
                }
            });
        });
        ui.separator();
        // Orden y alineación de la capa PRIMARIA seleccionada — deshabilitados
        // enteros (el propio botón del submenú) sin selección, en vez de
        // mostrar el submenú vacío o con todo gris dentro.
        let sel = state.selection.primary();
        ui.add_enabled_ui(sel.is_some(), |ui| {
            ui.menu_button("Layers", |ui| {
                let Some(id) = sel else {
                    return;
                };
                // Bring to Front/Move Forward no tendrían efecto si ya está
                // en el extremo del frente (`current == last`); Move
                // Backward/Send to Back igual en el del fondo (`current ==
                // 0`) — se deshabilitan en vez de dejarlos ahí sin más,
                // para que el diseño lo demuestre en vez de solo no-opear.
                let range = sibling_position(state, id);
                let can_go_forward = range.is_some_and(|(_, current, last)| current < last);
                let can_go_backward = range.is_some_and(|(_, current, _)| current > 0);
                let mut z = |ui: &mut egui::Ui, label: &str, enabled: bool, to: ZOrder| {
                    if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
                        reorder_layer(state, id, to);
                        ui.close();
                    }
                };
                z(ui, "Bring to Front", can_go_forward, ZOrder::Front);
                z(ui, "Move Forward", can_go_forward, ZOrder::Forward);
                z(ui, "Move Backward", can_go_backward, ZOrder::Backward);
                z(ui, "Send to Back", can_go_backward, ZOrder::Back);
            });
        });
        ui.add_enabled_ui(sel.is_some(), |ui| {
            ui.menu_button("Align to Page", |ui| {
                let Some(id) = sel else {
                    return;
                };
                let Ok(page) = state.doc.page() else {
                    return;
                };
                let (page_w, page_h) = (page.width, page.height);
                let Some(t) = state.doc.layer(id).ok().map(|l| l.transform) else {
                    return;
                };
                // `selectable_label`, no `button`: resalta la opción que YA
                // coincide con la posición actual de la capa — mismo widget
                // que ya usa este archivo para "elegido entre varias
                // opciones" (alineación de texto, más abajo en este mismo
                // módulo).
                let mut a = |ui: &mut egui::Ui, label: &str, after: Transform| {
                    if ui.selectable_label(after == t, label).clicked() {
                        apply_alignment(state, id, after);
                        ui.close();
                    }
                };
                a(
                    ui,
                    "Left",
                    canvas_core::align_horizontal(&t, page_w, canvas_core::HAlign::Left),
                );
                a(
                    ui,
                    "Center",
                    canvas_core::align_horizontal(&t, page_w, canvas_core::HAlign::Center),
                );
                a(
                    ui,
                    "Right",
                    canvas_core::align_horizontal(&t, page_w, canvas_core::HAlign::Right),
                );
                ui.separator();
                a(
                    ui,
                    "Top",
                    canvas_core::align_vertical(&t, page_h, canvas_core::VAlign::Top),
                );
                a(
                    ui,
                    "Middle",
                    canvas_core::align_vertical(&t, page_h, canvas_core::VAlign::Middle),
                );
                a(
                    ui,
                    "Bottom",
                    canvas_core::align_vertical(&t, page_h, canvas_core::VAlign::Bottom),
                );
                ui.separator();
                let centered_h =
                    canvas_core::align_horizontal(&t, page_w, canvas_core::HAlign::Center);
                let centered =
                    canvas_core::align_vertical(&centered_h, page_h, canvas_core::VAlign::Middle);
                a(ui, "Center on page", centered);
            });
        });
        ui.separator();
        // Estos tres, a diferencia de los de arriba, se resuelven AQUÍ
        // MISMO con `state` directamente — no tocan disco ni el resto de
        // `App`, así que no hace falta pasarlos por `CanvasAction`/`main.rs`.
        let bg_active = state.background_active();
        let bg_can_toggle = bg_active || state.background_source().is_some();
        let mut bg_on = bg_active;
        if ui
            .add_enabled(
                bg_can_toggle,
                egui::Checkbox::new(&mut bg_on, "Blurred background"),
            )
            .clicked()
        {
            state.set_blurred_background(bg_on);
            ui.close();
        }
        let crop_eligible = state
            .selection
            .primary()
            .and_then(|id| state.doc.layer(id).ok())
            .is_some_and(|l| matches!(l.content, LayerContent::Image(_)));
        let mut crop_on = state.crop_mode;
        if ui
            .add_enabled(crop_eligible, egui::Checkbox::new(&mut crop_on, "Crop"))
            .clicked()
        {
            state.crop_mode = crop_on;
            ui.close();
        }
        if ui.button("Size").clicked() {
            state.size_popup = state.doc.page().ok().map(|p| (p.width, p.height));
            ui.close();
        }
    });

    if action.is_none() {
        action = replace_url_popup_ui(state, ui.ctx());
    }

    if rect.width() < 1.0 || rect.height() < 1.0 {
        return action;
    }

    let page_dims = match state.doc.page() {
        Ok(p) => (p.width, p.height),
        Err(_) => (1.0, 1.0),
    };

    // La ranura activa siempre conoce su tamaño real (es el documento que se
    // está editando; no hace falta esperar a `DeckProbed`) — mantenerla al
    // día aquí cubre tanto la primera carga como un cambio de tamaño de
    // página desde el panel, sin ningún caso especial.
    let mut sizes_changed = false;
    if let Some(slot) = deck.slots.get_mut(deck.active) {
        if slot.page != Some(page_dims) {
            slot.page = Some(page_dims);
            sizes_changed = true;
        }
    }
    // Defensa en profundidad: cualquier ranura YA cargada (no solo la
    // activa) conoce su tamaño real por su propio documento, sin depender
    // de que `DeckProbed` haya llegado ni de que el sondeo funcione para su
    // formato. Sin esto, un lienzo mayor que la estimación de `Slot::size()`
    // se pinta fuera de su `rect` de layout y se come el hueco con el
    // vecino. Acumulador local porque no se puede escribir
    // `deck.layout_dirty` mientras `&mut deck.slots` sigue prestado por el
    // bucle.
    for slot in &mut deck.slots {
        if let SlotContent::Ready(d) = &slot.content {
            if let Ok(p) = d.doc.page() {
                let real = (p.width, p.height);
                if slot.page != Some(real) {
                    slot.page = Some(real);
                    sizes_changed = true;
                }
            }
        }
    }
    deck.layout_dirty |= sizes_changed;
    if deck.layout_dirty {
        // Recolocar puede desplazar el origen del lienzo ACTIVO como efecto
        // secundario (el centrado en el eje transversal usa el máximo
        // ancho/alto de TODAS las ranuras: aprender el tamaño real de un
        // vecino cambia ese máximo). Compensar el pan para que el lienzo
        // activo se quede clavado en pantalla — el usuario no pidió mover
        // nada. No aplica en el primer frame: `needs_fit`/`AutoFit` van a
        // reescribir el pan entero de todos modos, y no-op con una sola
        // ranura (su origen activo siempre es `(0,0)`).
        let before = deck.active_origin();
        deck.relayout();
        if !state.viewport.needs_fit {
            let after = deck.active_origin();
            let (dx, dy) = (after.0 - before.0, after.1 - before.1);
            if dx != 0.0 || dy != 0.0 {
                state.viewport.pan -= egui::vec2(
                    (dx * state.viewport.zoom) as f32,
                    (dy * state.viewport.zoom) as f32,
                );
            }
        }
    }

    // Salto pedido por la tira, el teclado, un clic directo sobre otro
    // lienzo o "Añadir lienzo": centra sobre el nuevo lienzo activo sin
    // tocar el zoom. También arma `AutoFit::Active` — sin esto, si el modo
    // seguía en `All` (el usuario acababa de pulsar `Ctrl+Alt+0`), el
    // primer redimensionado después de este centrado volvía a encajar TODA
    // la baraja (`resized` más abajo) y deshacía el centrado, con el efecto
    // de "vuelve a la vista de siempre" — el nuevo centrado puntual pasa a
    // ser la referencia, no una excepción que el próximo resize revierta.
    if let Some(target) = state.viewport.center_request.take() {
        state.viewport.center_on(target, rect.size());
        state.viewport.auto_fit = AutoFit::Active;
    }

    // Ajustar el lienzo activo: Ctrl/Cmd+0 o primer frame. Ajustar TODA la
    // baraja: Ctrl+Alt+0 (más específico primero, mismo patrón que
    // redo/undo en `handle_shortcuts`).
    let fit_all = ui.ctx().input_mut(|i| {
        i.consume_shortcut(&egui::KeyboardShortcut::new(
            egui::Modifiers::COMMAND | egui::Modifiers::ALT,
            egui::Key::Num0,
        ))
    });
    let fit_active = ui.ctx().input_mut(|i| {
        i.consume_shortcut(&egui::KeyboardShortcut::new(
            egui::Modifiers::COMMAND,
            egui::Key::Num0,
        ))
    });
    // Reajuste automático si el área de dibujo cambió de tamaño desde el
    // frame anterior (ventana maximizada/restaurada, panel lateral
    // arrastrado): repite el último ajuste automático (`Ctrl+0`/
    // `Ctrl+Alt+0`) mientras siga armado — se desarma en cuanto el usuario
    // hace zoom o paneo a mano (`Viewport::manual_view_change`). SIEMPRE se
    // sella el tamaño, no solo cuando cambia, para no quedar desincronizado.
    let resized = state.viewport.note_size(rect.size());
    if fit_all {
        state.viewport.fit(deck.bounds(), rect.size(), AutoFit::All);
    } else if fit_active || state.viewport.needs_fit {
        state
            .viewport
            .fit(deck.active_rect(), rect.size(), AutoFit::Active);
    } else if resized {
        match state.viewport.auto_fit {
            AutoFit::Active => {
                state
                    .viewport
                    .fit(deck.active_rect(), rect.size(), AutoFit::Active);
            }
            AutoFit::All => {
                state.viewport.fit(deck.bounds(), rect.size(), AutoFit::All);
            }
            AutoFit::Off => {}
        }
    }

    // Zoom pedido desde el menú, anclado al centro del lienzo.
    if let Some(factor) = state.pending_zoom_factor.take() {
        state.viewport.zoom_at(rect.size() / 2.0, factor);
    }

    // Rueda: desplaza a lo largo del eje PRIMARIO de la baraja (Shift = eje
    // transversal); Ctrl+rueda y el pellizco hacen zoom, anclados al cursor.
    // Es uniforme también con un solo lienzo — una excepción "con un archivo
    // hace zoom" se sentiría como un fallo, no como una regla.
    if response.hovered() {
        let (raw_scroll, pinch, pointer, ctrl, shift) = ui.ctx().input(|i| {
            (
                i.smooth_scroll_delta,
                i.zoom_delta(),
                i.pointer.hover_pos(),
                i.modifiers.command,
                i.modifiers.shift,
            )
        });
        if ctrl && raw_scroll.y != 0.0 {
            let factor = (f64::from(raw_scroll.y) * 0.0025).exp();
            let anchor = pointer.map_or(rect.size() / 2.0, |p| p - rect.min);
            state.viewport.zoom_at(anchor, factor);
        } else if raw_scroll != egui::Vec2::ZERO {
            // El ratón manda un solo eje de rueda (`raw_scroll.y`); a qué
            // componente del pan va depende de cuál sea el eje primario de
            // la baraja — Shift pide el transversal. Un trackpad que ya
            // manda X (pellizco de dos dedos, Shift+rueda que el propio SO
            // convierte) se respeta tal cual, sin remapear.
            let is_horizontal = matches!(deck.axis, DeckAxis::Horizontal);
            let swap = shift != is_horizontal;
            let delta = if swap && raw_scroll.x == 0.0 {
                egui::vec2(raw_scroll.y, 0.0)
            } else {
                raw_scroll
            };
            // `+=`, no `-=`: mismo signo que el paneo por arrastre de más
            // abajo (`pan += drag_delta`) y que `egui::ScrollArea`
            // (`offset -= scroll_delta`, con `offset` en el sentido
            // contrario a `pan` — es la posición del contenido en pantalla,
            // no cuánto se ha desplazado dentro de él). Con `-=` la rueda
            // quedaba invertida respecto al resto de la propia app.
            state.viewport.manual_view_change();
            state.viewport.pan += delta;
        }
        if (pinch - 1.0).abs() > 1e-4 {
            let anchor = pointer.map_or(rect.size() / 2.0, |p| p - rect.min);
            state.viewport.zoom_at(anchor, f64::from(pinch));
        }
    }

    // Paneo: botón central, o espacio + arrastre primario.
    let space_down = ui.ctx().input(|i| i.key_down(egui::Key::Space));
    let panning = response.dragged_by(egui::PointerButton::Middle)
        || (space_down && response.dragged_by(egui::PointerButton::Primary));
    if panning {
        state.viewport.manual_view_change();
        state.viewport.pan += response.drag_delta();
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    } else if space_down && response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }

    // Origen del lienzo activo en el espacio de baraja, en puntos de
    // pantalla. `slot_rect` es `rect` desplazado ese origen — pasado en vez
    // de `rect` a `layer_interaction` y a los cuatro ayudantes de
    // coordenadas (`page_to_screen` y compañía, más abajo), consigue que un
    // lienzo que no esté en el origen de la baraja se seleccione/arrastre/
    // redimensione igual que si fuera el único, sin tocar ni una línea de
    // esa lógica.
    let (origin_x, origin_y) = deck.active_origin();
    let slot_offset = egui::vec2(
        (origin_x * state.viewport.zoom) as f32,
        (origin_y * state.viewport.zoom) as f32,
    );
    let slot_rect = egui::Rect::from_min_size(rect.min + slot_offset, rect.size());

    // Qué ranuras están a la vista (con margen de precarga), y sella cuándo
    // se vieron por última vez (política de descarte LRU). Calculado AQUÍ
    // (antes de resolver la pulsación) porque el hit-test de la cabecera de
    // cada lienzo, más abajo, solo tiene sentido sobre ranuras visibles —
    // el resto del uso de `visible` (carga perezosa, descarte, escena,
    // `draw_slot_chrome`) sigue más adelante sin cambios, solo reutiliza
    // este mismo cálculo en vez de repetirlo.
    let (x0, y0) = screen_to_page(&state.viewport, rect, rect.min);
    let (x1, y1) = screen_to_page(&state.viewport, rect, rect.max);
    let view_deck_rect = Deck::dilate(DeckRect {
        x: x0,
        y: y0,
        w: x1 - x0,
        h: y1 - y0,
    });
    let visible = deck.visible_indices(view_deck_rect);
    deck.mark_visible(&visible);

    // Pulsación sobre un lienzo que no es el activo: lo activa (el
    // intercambio en sí lo aplica `deck::apply_jump`, fuera de este módulo,
    // para no mutar el documento activo a mitad de este mismo frame). Se
    // decide en el frame de la PULSACIÓN, no en el de soltar limpio
    // (`response.clicked()`, que solo se cumple si el puntero no se movió
    // más allá del umbral de arrastre de egui entre pulsar y soltar — un
    // clic humano real casi nunca es tan quieto). Cuando egui clasifica esa
    // pulsación como arrastre en vez de clic, `clicked()` no llega nunca, y
    // `layer_interaction` SÍ corría: como usa `slot_rect` (siempre el
    // espacio de la ranura ACTIVA), un clic con el más mínimo temblor sobre
    // OTRO lienzo agarraba y movía una capa del documento activo — la
    // «Position X» que cambiaba sola en el panel de propiedades sin que el
    // usuario tocase ninguna capa.
    if ui.input(|i| i.pointer.primary_pressed()) && !space_down && response.contains_pointer() {
        if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
            // Cabecera de CUALQUIER lienzo visible (activo o no) se
            // comprueba PRIMERO, en espacio de pantalla — antes del hit-test
            // en espacio de página de más abajo, que solo conoce el cuerpo
            // del lienzo, no su cabecera (que vive por encima, fuera de su
            // `DeckRect`). Un acierto aquí consume la pulsación entera: no
            // cae al hit-test de activación ni a `layer_interaction`.
            let mut header_hit = false;
            for &idx in &visible {
                if header_hit {
                    break;
                }
                let Some((id, is_placeholder, s_rect)) = deck
                    .slots
                    .get(idx)
                    .map(|s| (s.id, s.is_placeholder, s.rect))
                else {
                    continue;
                };
                let top_left = page_to_screen(&state.viewport, rect, s_rect.x, s_rect.y);
                let top_right =
                    page_to_screen(&state.viewport, rect, s_rect.x + s_rect.w, s_rect.y);
                let Some(header) = slot_header_layout(top_left.x, top_right.x, top_left.y) else {
                    continue;
                };
                if !is_placeholder && header.name.contains(pos) {
                    // Ver `draw_rename_overlay`: el propio cuadro de texto
                    // se dibuja ahí, en un `egui::Area` de primer plano, no
                    // aquí — aquí solo se arma el estado y se le pide el
                    // foco.
                    let stem = deck
                        .slots
                        .get(idx)
                        .and_then(|s| s.path.file_stem())
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    deck.rename_edit = Some((id, stem));
                    ui.memory_mut(|m| {
                        m.request_focus(egui::Id::new(("canvas_slot_rename", id)));
                    });
                    header_hit = true;
                } else if header.prev.contains(pos) {
                    deck.move_slot(id, MoveDir::Prev);
                    header_hit = true;
                } else if header.next.contains(pos) {
                    deck.move_slot(id, MoveDir::Next);
                    header_hit = true;
                } else if header.lock.contains(pos) {
                    if let Some(s) = deck.slots.get_mut(idx) {
                        s.locked = !s.locked;
                    }
                    header_hit = true;
                } else if header.dup.contains(pos) {
                    action = Some(CanvasAction::Duplicate(id));
                    header_hit = true;
                } else if header.del.contains(pos) {
                    action = Some(CanvasAction::Delete(id));
                    header_hit = true;
                }
            }
            if header_hit {
                state.press_on_other_slot = true;
            } else {
                let (dx, dy) = screen_to_page(&state.viewport, rect, pos);
                let target = deck.slots.iter().position(|s| {
                    dx >= s.rect.x
                        && dx <= s.rect.x + s.rect.w
                        && dy >= s.rect.y
                        && dy <= s.rect.y + s.rect.h
                });
                if let Some(idx) = target {
                    if idx != deck.active {
                        deck.jump_to = Some(idx);
                        // A petición del usuario: cambiar de activo desde el
                        // área central SIEMPRE centra la vista sobre él, en
                        // vez de dejarlo donde cayera (antes solo la tira o
                        // el teclado pedían recentrado) — así no hace falta
                        // ir a buscarlo tras el salto.
                        deck.jump_center = true;
                        state.press_on_other_slot = true;
                    }
                } else if deck.folder.is_some()
                    && dx >= deck.add_zone.x
                    && dx <= deck.add_zone.x + deck.add_zone.w
                    && dy >= deck.add_zone.y
                    && dy <= deck.add_zone.y + deck.add_zone.h
                {
                    // Pulsación sobre la zona "+" al final de la baraja: crea
                    // y activa un lienzo en blanco, igual que
                    // `App::add_canvas` (botón de la tira) pero resuelto
                    // aquí mismo — es una operación puramente en memoria, no
                    // toca disco ni el watcher, así que no hace falta pasar
                    // por `main.rs`.
                    if let Some(idx) =
                        deck.push_placeholder((deck.add_zone.w, deck.add_zone.h), new_canvas_ext)
                    {
                        deck.jump_to = Some(idx);
                        deck.jump_center = true;
                        state.press_on_other_slot = true;
                    }
                }
            }
        }
    }

    // Selección, arrastre y redimensionado (si no se está paneando, el
    // gesto en curso no pertenece a la baraja, y el diseño activo no está
    // bloqueado — `Slot::locked`, cabecera del lienzo).
    let active_locked = deck.slots.get(deck.active).is_some_and(|s| s.locked);
    if !panning && !space_down && !state.press_on_other_slot && !active_locked {
        layer_interaction(state, ui, &response, slot_rect);
    }

    // La marca dura el gesto entero (pulsar, arrastrar, soltar) y se limpia
    // cuando el botón ya no está pulsado — DESPUÉS de la guardia de arriba,
    // para que el frame en que se suelta (donde egui emite `clicked`/
    // `drag_stopped`) siga protegido.
    if !ui.input(|i| i.pointer.primary_down()) {
        state.press_on_other_slot = false;
    }

    // Render vello → textura del tamaño físico del lienzo.
    let ppp = ui.ctx().pixels_per_point();
    let width = (rect.width() * ppp).round().max(1.0) as u32;
    let height = (rect.height() * ppp).round().max(1.0) as u32;
    let surface = CanvasSurface::ensure(surface_slot, rs, width, height);

    // Transformación del espacio de BARAJA a píxeles físicos del lienzo, sin
    // desplazar por ninguna ranura en particular: cada lienzo visible añade
    // su propio origen antes de renderizarse (más abajo).
    let base_view = Affine::translate((
        f64::from(state.viewport.pan.x * ppp),
        f64::from(state.viewport.pan.y * ppp),
    )) * Affine::scale(state.viewport.zoom * f64::from(ppp));

    // Carga perezosa: pide las ranuras `Idle` visibles (o del radio de
    // precarga alrededor de la activa) que quepan en el presupuesto de
    // cargas en vuelo.
    if let Some(folder) = deck.folder.clone() {
        for path in deck.request_loads(&visible) {
            loader::spawn_load_slot(folder.clone(), path, tx.clone(), ui.ctx().clone());
        }
    }
    // Descarte: libera memoria de ranuras lejanas, limpias y sin guardado en
    // curso, por encima del presupuesto.
    for scope in deck.evict() {
        renderer.forget_scope(scope);
    }

    // Una sola escena para todos los lienzos visibles ya cargados (activo o
    // `Ready`); el resto se pinta encima como miniatura/placeholder, con el
    // `Painter` normal de egui (más abajo, en `draw_slot_chrome`) — ya están
    // en GPU desde la galería, no hace falta subirlas de nuevo a vello.
    let mut scene = vello::Scene::new();
    for &idx in &visible {
        let Some(slot) = deck.slots.get(idx) else {
            continue;
        };
        let scope = FxScope(slot.id);
        let view = base_view * Affine::translate(slot.rect.origin());
        if idx == deck.active {
            sync_and_append(
                &mut scene,
                renderer,
                rs,
                &state.doc,
                &state.images,
                scope,
                view,
            );
        } else if let SlotContent::Ready(doc) = &slot.content {
            sync_and_append(&mut scene, renderer, rs, &doc.doc, &doc.images, scope, view);
        }
    }
    if let Err(e) = surface.render(rs, renderer, &scene) {
        tracing::error!("fallo renderizando el lienzo: {e}");
    }

    ui.painter().image(
        surface.egui_id(),
        rect,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );

    for &idx in &visible {
        draw_slot_chrome(state, deck, idx, ui, rect);
    }
    draw_add_zone(state, deck, ui, rect);
    draw_header_tooltips(deck, ui, rect, &state.viewport, &visible);

    if state.show_grid {
        draw_grid(state, ui, slot_rect, rect, page_dims);
    }
    draw_selection_overlay(state, ui, slot_rect, rect);
    if state.show_rulers {
        draw_rulers(state, ui, slot_rect, rect);
    }

    // Renombrado en curso desde una cabecera (si lo hay): `egui::Area` de
    // primer plano, PASO DE UI SEPARADO del `response` gigante de arriba —
    // ver la doc de `draw_rename_overlay`.
    if let Some(a) = draw_rename_overlay(state, deck, ui, rect) {
        action = Some(a);
    }
    size_popup_ui(state, ui.ctx());
    action
}

/// Sincroniza los efectos GPU de un documento y lo añade a `scene` con su
/// propia transformación de vista. Un `fn` normal, no un cierre: dentro del
/// bucle de `canvas_ui` se llama con `renderer`/`scene` prestados de forma
/// disjunta en cada iteración, y un cierre que capturase ambos a la vez
/// complicaría el préstamo sin necesidad.
#[allow(clippy::too_many_arguments)]
fn sync_and_append(
    scene: &mut vello::Scene,
    renderer: &mut CanvasRenderer,
    rs: &RenderState,
    doc: &Document,
    images: &ImageMap,
    scope: FxScope,
    view: Affine,
) {
    if let Ok(page) = doc.page() {
        let fx_targets: Vec<(LayerId, canvas_core::Effects)> =
            page.layers.iter().map(|l| (l.id, l.effects)).collect();
        for (id, effects) in fx_targets {
            if let Some(source) = images.get(&id) {
                renderer.sync_layer_effects(&rs.device, &rs.queue, scope, id, source, &effects);
            }
        }
    }
    let blurred = renderer.blur_overrides(scope);
    canvas_render::append_document(scene, doc, images, &blurred, view, true);
}

/// Marco (acento en la activa, débil en las demás), nombre de archivo, y —
/// si la ranura todavía no está cargada — su miniatura o un glifo de
/// estado, encima del blit de vello.
fn draw_slot_chrome(state: &EditorState, deck: &Deck, idx: usize, ui: &egui::Ui, rect: egui::Rect) {
    let Some(slot) = deck.slots.get(idx) else {
        return;
    };
    let is_active = idx == deck.active;
    let tl = page_to_screen(&state.viewport, rect, slot.rect.x, slot.rect.y);
    let br = page_to_screen(
        &state.viewport,
        rect,
        slot.rect.x + slot.rect.w,
        slot.rect.y + slot.rect.h,
    );
    let screen_rect = egui::Rect::from_min_max(tl, br);
    let painter = ui.painter();

    if !matches!(slot.content, SlotContent::Ready(_) | SlotContent::Active) {
        if let Some(tex) = &slot.thumb {
            painter.image(
                tex.id(),
                screen_rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        } else {
            painter.rect_filled(screen_rect, 0.0, ui.visuals().extreme_bg_color);
        }
        if let SlotContent::Failed(message) = &slot.content {
            // Un fallo de carga de fondo SÍ se explica, aunque haya
            // miniatura: es la única pista de por qué este lienzo no abre.
            painter.text(
                screen_rect.center(),
                egui::Align2::CENTER_CENTER,
                "⚠",
                egui::FontId::proportional(28.0),
                ui.visuals().error_fg_color,
            );
            let mut short = message.clone();
            if short.chars().count() > 60 {
                short = format!("{}…", short.chars().take(59).collect::<String>());
            }
            painter.text(
                screen_rect.center() + egui::vec2(0.0, 22.0),
                egui::Align2::CENTER_TOP,
                short,
                egui::FontId::proportional(10.0),
                ui.visuals().error_fg_color,
            );
        } else if slot.thumb.is_none() {
            let glyph = if slot.thumb_failed {
                "⚠"
            } else if slot.kind == ItemKind::Design {
                "🖹"
            } else {
                "⏳"
            };
            painter.text(
                screen_rect.center(),
                egui::Align2::CENTER_CENTER,
                glyph,
                egui::FontId::proportional(28.0),
                ui.visuals().weak_text_color(),
            );
        }
    }

    let stroke = if is_active {
        egui::Stroke::new(2.0, ACCENT)
    } else {
        egui::Stroke::new(1.0, ui.visuals().weak_text_color().gamma_multiply(0.6))
    };
    painter.rect_stroke(screen_rect, 0.0, stroke, egui::StrokeKind::Outside);

    draw_slot_header(deck, slot, ui, screen_rect);
}

/// Cabecera de un lienzo, justo encima de su `screen_rect`: nombre a la
/// izquierda (editable — ver `draw_rename_overlay`, que se encarga del
/// cuadro de texto en sí) y, a la derecha, mover/bloquear/duplicar/borrar.
/// Duplicar/borrar se ocultan en una ranura PROVISIONAL (`is_placeholder`):
/// no tienen sentido sin archivo en disco. Pintada con el `Painter` normal
/// de egui, no widgets — el hit-test vive en el bloque de pulsación de
/// `canvas_ui`, sobre el mismo `slot_header_layout` que esta función usa
/// para pintar, así que ambos nunca pueden desalinearse.
///
/// Los 5 botones son formas dibujadas a mano (triángulos, arco, rects),
/// NO texto/emoji: un carácter Unicode como ▲/▼ puede simplemente no estar
/// en la fuente que trae `egui` integrada — pasó de verdad (las flechas
/// dejaron de verse, mientras que otros glifos ya usados en la app seguían
/// bien) — y un dibujo a mano no depende de qué cubra ninguna fuente.
fn draw_slot_header(deck: &Deck, slot: &Slot, ui: &egui::Ui, screen_rect: egui::Rect) {
    let Some(header) =
        slot_header_layout(screen_rect.left(), screen_rect.right(), screen_rect.top())
    else {
        return;
    };
    let painter = ui.painter();
    // Fondo propio con contraste real (antes era texto suelto directamente
    // sobre el lienzo — "iconos grises sobre fondo gris" cuando la imagen de
    // debajo también era clara/gris) + un borde débil para separarla del
    // lienzo cuando el fondo coincide en tono.
    painter.rect_filled(header.bar, 4.0, ui.visuals().extreme_bg_color);
    painter.rect_stroke(
        header.bar,
        4.0,
        egui::Stroke::new(1.0, ui.visuals().weak_text_color().gamma_multiply(0.6)),
        egui::StrokeKind::Outside,
    );
    let icon_color = ui.visuals().strong_text_color();

    let renaming = deck
        .rename_edit
        .as_ref()
        .is_some_and(|(id, _)| *id == slot.id);
    if !renaming {
        let mut name = slot.name.clone();
        let max_chars = ((header.name.width() / 6.5) as usize).max(4);
        if name.chars().count() > max_chars {
            name = format!(
                "{}…",
                name.chars()
                    .take(max_chars.saturating_sub(1))
                    .collect::<String>()
            );
        }
        painter.text(
            header.name.left_center() + egui::vec2(4.0, 0.0),
            egui::Align2::LEFT_CENTER,
            name,
            egui::FontId::proportional(11.0),
            ui.visuals().text_color(),
        );
    }

    // Flechas de mover en la dirección real de apilado: arriba/abajo con la
    // baraja en vertical, izquierda/derecha en horizontal.
    let (prev_dir, next_dir) = match deck.axis {
        DeckAxis::Vertical => (IconDir::Up, IconDir::Down),
        DeckAxis::Horizontal => (IconDir::Left, IconDir::Right),
    };
    draw_triangle_icon(painter, header.prev, prev_dir, icon_color);
    draw_triangle_icon(painter, header.next, next_dir, icon_color);
    draw_lock_icon(painter, header.lock, slot.locked, icon_color);
    draw_duplicate_icon(
        painter,
        header.dup,
        icon_color,
        ui.visuals().extreme_bg_color,
    );
    draw_delete_icon(painter, header.del, egui::Color32::from_rgb(220, 70, 70));
}

/// Dirección de `draw_triangle_icon`.
#[derive(Clone, Copy)]
enum IconDir {
    Up,
    Down,
    Left,
    Right,
}

/// Triángulo relleno apuntando en `dir`, centrado en `rect`.
fn draw_triangle_icon(
    painter: &egui::Painter,
    rect: egui::Rect,
    dir: IconDir,
    color: egui::Color32,
) {
    let c = rect.center();
    let s = rect.width().min(rect.height()) * 0.28;
    let points = match dir {
        IconDir::Up => vec![
            c + egui::vec2(0.0, -s),
            c + egui::vec2(-s, s * 0.8),
            c + egui::vec2(s, s * 0.8),
        ],
        IconDir::Down => vec![
            c + egui::vec2(0.0, s),
            c + egui::vec2(-s, -s * 0.8),
            c + egui::vec2(s, -s * 0.8),
        ],
        IconDir::Left => vec![
            c + egui::vec2(-s, 0.0),
            c + egui::vec2(s * 0.8, -s),
            c + egui::vec2(s * 0.8, s),
        ],
        IconDir::Right => vec![
            c + egui::vec2(s, 0.0),
            c + egui::vec2(-s * 0.8, -s),
            c + egui::vec2(-s * 0.8, s),
        ],
    };
    painter.add(egui::Shape::convex_polygon(
        points,
        color,
        egui::Stroke::NONE,
    ));
}

/// Puntos a lo largo de un arco de circunferencia. Convención pensada para
/// que `start = -FRAC_PI_2`/`end = FRAC_PI_2` trace un semicírculo superior
/// de izquierda a derecha pasando por arriba (coordenadas de pantalla, eje Y
/// hacia abajo) — el arco del candado.
fn arc_points(
    center: egui::Pos2,
    radius: f32,
    start: f32,
    end: f32,
    segments: usize,
) -> Vec<egui::Pos2> {
    (0..=segments)
        .map(|i| {
            let t = start + (end - start) * (i as f32 / segments as f32);
            center + egui::vec2(t.sin(), -t.cos()) * radius
        })
        .collect()
}

/// Candado: cuerpo relleno (rect redondeado) + arco. Cerrado, el arco se
/// apoya simétrico sobre el cuerpo; abierto, se desplaza hacia arriba y a
/// la derecha, dejando el lado izquierdo suelto.
fn draw_lock_icon(painter: &egui::Painter, rect: egui::Rect, locked: bool, color: egui::Color32) {
    let half_pi = std::f32::consts::FRAC_PI_2;
    let body_w = rect.width() * 0.5;
    let body_h = rect.height() * 0.42;
    let body = egui::Rect::from_center_size(
        rect.center() + egui::vec2(0.0, body_h * 0.35),
        egui::vec2(body_w, body_h),
    );
    painter.rect_filled(body, 1.0, color);
    let shackle_r = body_w * 0.42;
    let stroke = egui::Stroke::new(1.3, color);
    let shackle_center = egui::pos2(body.center().x, body.top());
    let center = if locked {
        shackle_center
    } else {
        shackle_center + egui::vec2(shackle_r * 0.55, -shackle_r * 0.35)
    };
    let pts = arc_points(center, shackle_r, -half_pi, half_pi, 10);
    painter.add(egui::Shape::line(pts, stroke));
}

/// Duplicar: dos rects redondeados solapados (icono estándar de "copiar").
/// `bg` es el fondo de la propia cabecera — rellena el rect de delante para
/// que de verdad tape la esquina del de detrás, en vez de dejar ambos
/// contornos cruzándose sin más.
fn draw_duplicate_icon(
    painter: &egui::Painter,
    rect: egui::Rect,
    color: egui::Color32,
    bg: egui::Color32,
) {
    let s = rect.width().min(rect.height()) * 0.36;
    let stroke = egui::Stroke::new(1.2, color);
    let back = egui::Rect::from_center_size(
        rect.center() + egui::vec2(-s * 0.28, -s * 0.28),
        egui::vec2(s, s),
    );
    let front = egui::Rect::from_center_size(
        rect.center() + egui::vec2(s * 0.28, s * 0.28),
        egui::vec2(s, s),
    );
    painter.rect_stroke(back, 1.0, stroke, egui::StrokeKind::Outside);
    painter.rect_filled(front, 1.0, bg);
    painter.rect_stroke(front, 1.0, stroke, egui::StrokeKind::Outside);
}

/// Borrar: cubo de basura simple — cuerpo, tapa y un par de ranuras.
fn draw_delete_icon(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let stroke = egui::Stroke::new(1.2, color);
    let w = rect.width() * 0.42;
    let h = rect.height() * 0.4;
    let body =
        egui::Rect::from_center_size(rect.center() + egui::vec2(0.0, h * 0.2), egui::vec2(w, h));
    painter.rect_stroke(body, 0.5, stroke, egui::StrokeKind::Outside);
    painter.line_segment(
        [
            egui::pos2(body.left() - w * 0.15, body.top()),
            egui::pos2(body.right() + w * 0.15, body.top()),
        ],
        stroke,
    );
    for i in [-1.0_f32, 1.0] {
        let x = rect.center().x + i * w * 0.22;
        painter.line_segment(
            [
                egui::pos2(x, body.top() + h * 0.18),
                egui::pos2(x, body.bottom() - h * 0.12),
            ],
            stroke,
        );
    }
}

/// Alto de la cabecera de un lienzo (nombre + botones), en px de pantalla —
/// constante frente al zoom, como el resto del texto de `draw_slot_chrome`.
const HEADER_H: f32 = 20.0;
/// Ancho de cada botón cuadrado de la cabecera cuando hay sitio de sobra.
const HEADER_BTN_W: f32 = 20.0;
/// Suelo: por debajo de esto un botón deja de ser legible/pulsable con
/// precisión — mejor que se quede aquí a que seis encimen sin límite.
const HEADER_BTN_MIN: f32 = 12.0;
/// Ancho de cabecera por debajo del cual ni se pinta ni se comprueba el
/// clic: con 5 botones al suelo (`HEADER_BTN_MIN * 5`) más algo de nombre,
/// menos que esto es un lienzo tan alejado que la cabecera sería ruido
/// ilegible superpuesto a otra cosa — mismo criterio que usan las
/// miniaturas de la tira, que a partir de cierto tamaño mínimo dejan de
/// intentar mostrar detalle y muestran solo un glifo.
const HEADER_MIN_VISIBLE_W: f32 = 70.0;

/// Rects (en espacio de PANTALLA) de la cabecera de un lienzo: la barra
/// entera (para el fondo), el nombre y los 5 botones, calculados a partir
/// del borde superior de su `screen_rect`. Una sola fuente de verdad
/// compartida por pintado (`draw_slot_header`, `draw_rename_overlay`,
/// `draw_header_tooltips`) y hit-test (`canvas_ui`) — así nunca se
/// desalinean entre sí.
struct SlotHeader {
    bar: egui::Rect,
    name: egui::Rect,
    prev: egui::Rect,
    next: egui::Rect,
    lock: egui::Rect,
    dup: egui::Rect,
    del: egui::Rect,
}

/// `None` si la cabecera es demasiado angosta en pantalla para pintarse o
/// pulsarse con sentido (ver `HEADER_MIN_VISIBLE_W`) — el llamador la omite
/// entera en vez de intentar encajarla.
fn slot_header_layout(left: f32, right: f32, top: f32) -> Option<SlotHeader> {
    let bar = egui::Rect::from_min_max(egui::pos2(left, top - HEADER_H), egui::pos2(right, top));
    if bar.width() < HEADER_MIN_VISIBLE_W {
        return None;
    }
    // Ancho de botón bien definido: se encoge en proporción al ancho real
    // del lienzo en pantalla (con suelo `HEADER_BTN_MIN`) en vez de quedar
    // fijo — así los 5 botones SIEMPRE caben dentro de la propia cabecera,
    // nunca se salen sobre el lienzo vecino, y en cuanto hay sitio de sobra
    // (zoom normal o mayor) se quedan clavados en `HEADER_BTN_W` sin seguir
    // creciendo ni encogiendo con el zoom.
    let btn_w = (bar.width() / 5.0).clamp(HEADER_BTN_MIN, HEADER_BTN_W);
    let buttons_w = btn_w * 5.0;
    let name_right = (bar.right() - buttons_w).max(bar.left());
    let name = egui::Rect::from_min_max(bar.left_top(), egui::pos2(name_right, bar.bottom()));
    let btn = |i: f32| {
        let x0 = name_right + btn_w * i;
        egui::Rect::from_min_max(
            egui::pos2(x0, bar.top()),
            egui::pos2(x0 + btn_w, bar.bottom()),
        )
    };
    Some(SlotHeader {
        bar,
        name,
        prev: btn(0.0),
        next: btn(1.0),
        lock: btn(2.0),
        dup: btn(3.0),
        del: btn(4.0),
    })
}

/// Tooltip de un botón de cabecera al pasar el ratón por encima. Los rects
/// de la cabecera son pintados a mano (`Painter`, ver la doc de
/// `draw_slot_header`), no widgets egui — no hay `Response` del que colgar
/// `on_hover_text`, así que el propio tooltip se pinta a mano también,
/// sobre los MISMOS rects que ya usa el hit-test de clic en `canvas_ui`
/// (nunca puede desalinearse de lo que en verdad es pulsable).
fn draw_header_tooltips(
    deck: &Deck,
    ui: &egui::Ui,
    rect: egui::Rect,
    viewport: &Viewport,
    visible: &[usize],
) {
    let Some(pos) = ui.input(|i| i.pointer.hover_pos()) else {
        return;
    };
    if !rect.contains(pos) {
        return;
    }
    let (move_prev, move_next) = match deck.axis {
        DeckAxis::Vertical => ("Move up", "Move down"),
        DeckAxis::Horizontal => ("Move left", "Move right"),
    };
    for &idx in visible {
        let Some(slot) = deck.slots.get(idx) else {
            continue;
        };
        let s_rect = slot.rect;
        let top_left = page_to_screen(viewport, rect, s_rect.x, s_rect.y);
        let top_right = page_to_screen(viewport, rect, s_rect.x + s_rect.w, s_rect.y);
        let Some(header) = slot_header_layout(top_left.x, top_right.x, top_left.y) else {
            continue;
        };
        let label = if header.prev.contains(pos) {
            Some(move_prev)
        } else if header.next.contains(pos) {
            Some(move_next)
        } else if header.lock.contains(pos) {
            Some(if slot.locked { "Unlock" } else { "Lock" })
        } else if header.dup.contains(pos) {
            Some("Duplicate")
        } else if header.del.contains(pos) {
            Some("Delete")
        } else if !slot.is_placeholder && header.name.contains(pos) {
            Some("Rename")
        } else {
            None
        };
        if let Some(text) = label {
            paint_tooltip(ui, pos, text);
            return;
        }
    }
}

/// Etiqueta pegada al cursor, mismo estilo (fondo + borde) que la cabecera
/// que la disparó.
fn paint_tooltip(ui: &egui::Ui, pos: egui::Pos2, text: &str) {
    let painter = ui.painter();
    let font = egui::FontId::proportional(11.0);
    let galley = painter.layout_no_wrap(text.to_owned(), font, ui.visuals().text_color());
    let pad = egui::vec2(6.0, 4.0);
    let box_rect =
        egui::Rect::from_min_size(pos + egui::vec2(12.0, 16.0), galley.size() + pad * 2.0);
    painter.rect_filled(box_rect, 4.0, ui.visuals().extreme_bg_color);
    painter.rect_stroke(
        box_rect,
        4.0,
        egui::Stroke::new(1.0, ui.visuals().weak_text_color()),
        egui::StrokeKind::Outside,
    );
    painter.galley(box_rect.min + pad, galley, ui.visuals().text_color());
}

/// Si hay un renombrado en curso (`deck.rename_edit`, pulsado desde la
/// cabecera de un lienzo), dibuja su cuadro de texto en un `egui::Area` de
/// primer plano anclado a esa cabecera — un paso de UI SEPARADO del
/// `response` gigante del lienzo (como ya hacen los modales existentes de
/// esta app), para que arrastrar dentro del cuadro (seleccionar texto) no
/// compita con el arrastre de capa por el mismo puntero. Mismo patrón
/// Escape-antes-que-`lost_focus()` que `gallery.rs::gallery_cell` y
/// `layers_panel.rs::rename_edit_ui`.
fn draw_rename_overlay(
    state: &EditorState,
    deck: &mut Deck,
    ui: &egui::Ui,
    rect: egui::Rect,
) -> Option<CanvasAction> {
    let id = deck.rename_edit.as_ref()?.0;
    let idx = deck.find_by_id(id)?;
    let s_rect = deck.slots[idx].rect;
    let top_left = page_to_screen(&state.viewport, rect, s_rect.x, s_rect.y);
    let top_right = page_to_screen(&state.viewport, rect, s_rect.x + s_rect.w, s_rect.y);
    // Si el zoom cambió mientras se renombraba y la cabecera ya no cabe
    // (`HEADER_MIN_VISIBLE_W`), cancela en vez de dejar el cuadro de texto
    // colgado sin dónde anclarse.
    let Some(header) = slot_header_layout(top_left.x, top_right.x, top_left.y) else {
        deck.rename_edit = None;
        return None;
    };

    let mut cancel = false;
    let mut commit = false;
    let text_id = egui::Id::new(("canvas_slot_rename", id));
    egui::Area::new(text_id.with("area"))
        .order(egui::Order::Foreground)
        .fixed_pos(header.name.left_top())
        .show(ui.ctx(), |ui| {
            if let Some((_, text)) = deck.rename_edit.as_mut() {
                let resp = ui.add_sized(
                    header.name.size(),
                    egui::TextEdit::singleline(text).id(text_id),
                );
                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    cancel = true;
                } else if resp.lost_focus() {
                    commit = true;
                }
            }
        });

    if cancel {
        deck.rename_edit = None;
        return None;
    }
    if commit {
        if let Some((id, text)) = deck.rename_edit.take() {
            let new_stem = text.trim().to_owned();
            let original_stem = deck
                .find_by_id(id)
                .and_then(|idx| deck.slots[idx].path.file_stem())
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            if !new_stem.is_empty() && new_stem != original_stem {
                return Some(CanvasAction::Rename(id, new_stem));
            }
        }
    }
    None
}

/// Zona "+" al final de la baraja, en el área central: mismo estilo que
/// `deck_strip::strip_add_cell` (borde discontinuo, glifo "✚", etiqueta) pero
/// en coordenadas de pantalla del propio lienzo, no de celda de tira. Solo
/// se pinta si hay una carpeta detrás de la baraja (un archivo suelto no
/// tiene dónde materializar el nuevo diseño) y si `deck.add_zone` cae dentro
/// de lo visible.
fn draw_add_zone(state: &EditorState, deck: &Deck, ui: &egui::Ui, rect: egui::Rect) {
    if deck.folder.is_none() {
        return;
    }
    let tl = page_to_screen(&state.viewport, rect, deck.add_zone.x, deck.add_zone.y);
    let br = page_to_screen(
        &state.viewport,
        rect,
        deck.add_zone.x + deck.add_zone.w,
        deck.add_zone.y + deck.add_zone.h,
    );
    let screen_rect = egui::Rect::from_min_max(tl, br);
    if !ui.is_rect_visible(screen_rect) {
        return;
    }
    let painter = ui.painter();
    painter.rect_stroke(
        screen_rect.shrink(4.0),
        6.0,
        egui::Stroke::new(1.5, ui.visuals().weak_text_color()),
        egui::StrokeKind::Inside,
    );
    let glyph_size = (screen_rect.width().min(screen_rect.height()) * 0.15).clamp(18.0, 56.0);
    painter.text(
        screen_rect.center() - egui::vec2(0.0, glyph_size * 0.4),
        egui::Align2::CENTER_CENTER,
        "✚",
        egui::FontId::proportional(glyph_size),
        ui.visuals().weak_text_color(),
    );
    painter.text(
        screen_rect.center() + egui::vec2(0.0, glyph_size * 0.6),
        egui::Align2::CENTER_CENTER,
        "Add canvas",
        egui::FontId::proportional(13.0),
        ui.visuals().weak_text_color(),
    );
}

const HANDLE_SIZE: f32 = 9.0;
const ACCENT: egui::Color32 = egui::Color32::from_rgb(0, 122, 255);

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

fn layer_interaction(
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

/// Propiedades del contenido de una capa de texto o forma, con edición en
/// vivo y consolidación en UN paso de deshacer.
fn content_properties_ui(state: &mut EditorState, ui: &mut egui::Ui, sel: LayerId) {
    let Ok(layer) = state.doc.layer(sel) else {
        return;
    };
    let original = layer.content.clone();
    let mut edited = original.clone();
    let mut changed = false;
    let mut commit = false;

    match &mut edited {
        LayerContent::Text(text) => {
            ui.label("Text");
            let r = ui.add(
                egui::TextEdit::multiline(&mut text.text)
                    .desired_rows(2)
                    .desired_width(f32::INFINITY),
            );
            changed |= r.changed();
            commit |= r.lost_focus();

            ui.horizontal(|ui| {
                ui.label("Font");
                let r = ui
                    .add(egui::TextEdit::singleline(&mut text.family).hint_text("System default"));
                changed |= r.changed();
                commit |= r.lost_focus();
            });
            ui.horizontal(|ui| {
                ui.label("Size");
                let r = ui.add(
                    egui::DragValue::new(&mut text.size)
                        .range(4.0..=800.0)
                        .speed(1.0),
                );
                changed |= r.changed();
                commit |= r.drag_stopped() || r.lost_focus();

                let bold = text.weight >= 600;
                if ui
                    .selectable_label(bold, "B")
                    .on_hover_text("Bold")
                    .clicked()
                {
                    text.weight = if bold { 400 } else { 700 };
                    changed = true;
                    commit = true;
                }
                if ui
                    .selectable_label(text.italic, "I")
                    .on_hover_text("Italic")
                    .clicked()
                {
                    text.italic = !text.italic;
                    changed = true;
                    commit = true;
                }
                let mut color = egui::Color32::from_rgba_unmultiplied(
                    text.color[0],
                    text.color[1],
                    text.color[2],
                    text.color[3],
                );
                if ui.color_edit_button_srgba(&mut color).changed() {
                    text.color = color.to_array();
                    changed = true;
                    commit = true;
                }
            });
            ui.horizontal(|ui| {
                ui.label("Spacing");
                let r = ui.add(
                    egui::DragValue::new(&mut text.letter_spacing)
                        .range(-20.0..=60.0)
                        .speed(0.2)
                        .max_decimals(1),
                );
                changed |= r.changed();
                commit |= r.drag_stopped() || r.lost_focus();
                ui.label("Line");
                let r = ui.add(
                    egui::DragValue::new(&mut text.line_height)
                        .range(0.5..=3.0)
                        .speed(0.02)
                        .max_decimals(2),
                );
                changed |= r.changed();
                commit |= r.drag_stopped() || r.lost_focus();
            });
            ui.horizontal(|ui| {
                for (align, label) in [
                    (canvas_core::TextAlign::Left, "Left"),
                    (canvas_core::TextAlign::Center, "Center"),
                    (canvas_core::TextAlign::Right, "Right"),
                ] {
                    if ui.selectable_label(text.align == align, label).clicked() {
                        text.align = align;
                        changed = true;
                        commit = true;
                    }
                }
            });
            ui.add_space(8.0);
        }
        LayerContent::Shape(shape) => {
            ui.label("Shape");
            ui.horizontal(|ui| {
                ui.label("Fill");
                let mut fill = egui::Color32::from_rgba_unmultiplied(
                    shape.fill[0],
                    shape.fill[1],
                    shape.fill[2],
                    shape.fill[3],
                );
                if ui.color_edit_button_srgba(&mut fill).changed() {
                    shape.fill = fill.to_array();
                    changed = true;
                    commit = true;
                }
                ui.label("Stroke");
                let mut stroke = egui::Color32::from_rgba_unmultiplied(
                    shape.stroke[0],
                    shape.stroke[1],
                    shape.stroke[2],
                    shape.stroke[3],
                );
                if ui.color_edit_button_srgba(&mut stroke).changed() {
                    shape.stroke = stroke.to_array();
                    changed = true;
                    commit = true;
                }
                let r = ui.add(
                    egui::DragValue::new(&mut shape.stroke_width)
                        .range(0.0..=100.0)
                        .speed(0.5)
                        .max_decimals(1),
                );
                changed |= r.changed();
                commit |= r.drag_stopped() || r.lost_focus();
            });
            if shape.kind == canvas_core::ShapeKind::Rect {
                ui.horizontal(|ui| {
                    ui.label("Corner radius");
                    let r = ui.add(
                        egui::DragValue::new(&mut shape.corner_radius)
                            .range(0.0..=500.0)
                            .speed(1.0)
                            .max_decimals(0),
                    );
                    changed |= r.changed();
                    commit |= r.drag_stopped() || r.lost_focus();
                });
            }
            ui.add_space(8.0);
        }
        _ => return,
    }

    if changed && edited != original {
        if state.content_edit.is_none() {
            state.content_edit = Some((sel, original));
        }
        if let Ok(l) = state.doc.layer_mut(sel) {
            l.content = edited;
        }
    }
    if commit {
        if let Some((id, before)) = state.content_edit.take() {
            let after = state
                .doc
                .layer(id)
                .map(|l| l.content.clone())
                .unwrap_or_else(|_| before.clone());
            if after != before {
                state.push_undo_step(Box::new(canvas_core::SetContent {
                    layer: id,
                    before,
                    after,
                }));
            }
        }
    }
}

fn format_dims(t: &Transform) -> String {
    format!(
        "{} × {} px",
        t.width.round() as i64,
        t.height.round() as i64
    )
}

/// Etiqueta flotante junto al cursor durante un gesto (dimensiones, grados).
fn show_drag_tag(ui: &egui::Ui, pos: egui::Pos2, text: String) {
    let painter = ui.painter();
    let galley =
        painter.layout_no_wrap(text, egui::FontId::proportional(12.0), egui::Color32::WHITE);
    let tag_pos = pos + egui::vec2(14.0, 16.0);
    let bg = egui::Rect::from_min_size(tag_pos, galley.size() + egui::vec2(10.0, 6.0));
    painter.rect_filled(bg, 4.0, egui::Color32::from_black_alpha(190));
    painter.galley(tag_pos + egui::vec2(5.0, 3.0), galley, egui::Color32::WHITE);
}

/// Recuadro de selección (rotado), manejadores, manejador de rotación y guías
/// magnéticas, pintados por encima del lienzo.
/// `coord`: origen de coordenadas página→pantalla (el lienzo activo puede no
/// estar en el origen de la baraja, ver `canvas_ui`). `clip`: rect real del
/// viewport en pantalla — recorte del `Painter` y extremos de las guías/
/// manejadores que deben llegar de borde a borde del lienzo visible, no del
/// lienzo activo si estuviera desplazado.
fn draw_selection_overlay(state: &EditorState, ui: &egui::Ui, coord: egui::Rect, clip: egui::Rect) {
    let painter = ui.painter_at(clip);

    // Guías magnéticas activas (líneas que cruzan todo el lienzo).
    let guide_stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(255, 64, 129));
    for &gx in &state.snap_guides.0 {
        let x = page_to_screen(&state.viewport, coord, gx, 0.0).x;
        painter.line_segment(
            [egui::pos2(x, clip.top()), egui::pos2(x, clip.bottom())],
            guide_stroke,
        );
    }
    for &gy in &state.snap_guides.1 {
        let y = page_to_screen(&state.viewport, coord, 0.0, gy).y;
        painter.line_segment(
            [egui::pos2(clip.left(), y), egui::pos2(clip.right(), y)],
            guide_stroke,
        );
    }

    // Contorno fino (sin manejadores) para las capas seleccionadas que NO
    // son la primaria: la primaria es la única que manda en el panel y los
    // gestos, pero el resto de la selección múltiple sigue siendo visible.
    for &id in state.selection.ids() {
        if Some(id) == state.selection.primary() {
            continue;
        }
        let Ok(layer) = state.doc.layer(id) else {
            continue;
        };
        let [tl, tr, bl, br] = layer_corners_screen(&state.viewport, coord, &layer.transform);
        painter.add(egui::Shape::closed_line(
            vec![tl, tr, br, bl],
            egui::Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(0, 122, 255, 140)),
        ));
    }

    let Some(sel) = state.selection.primary() else {
        return;
    };
    let Ok(layer) = state.doc.layer(sel) else {
        return;
    };
    let t = &layer.transform;
    let accent = if state.crop_mode {
        egui::Color32::from_rgb(255, 149, 0) // naranja: modo recorte
    } else {
        ACCENT
    };

    // Contorno rotado: sup-izq → sup-der → inf-der → inf-izq.
    let [tl, tr, bl, br] = layer_corners_screen(&state.viewport, coord, t);
    painter.add(egui::Shape::closed_line(
        vec![tl, tr, br, bl],
        egui::Stroke::new(1.5, accent),
    ));

    // Manejador de rotación: línea + círculo (no en modo recorte).
    if !state.crop_mode {
        let top_center = egui::pos2((tl.x + tr.x) / 2.0, (tl.y + tr.y) / 2.0);
        let handle = rotation_handle_screen(&state.viewport, coord, t);
        painter.line_segment([top_center, handle], egui::Stroke::new(1.0, accent));
        painter.circle_filled(handle, HANDLE_SIZE / 2.0, egui::Color32::WHITE);
        painter.circle_stroke(handle, HANDLE_SIZE / 2.0, egui::Stroke::new(1.5, accent));
    }

    // Manejadores de esquina (cuadrados centrados en las esquinas rotadas).
    for corner in [tl, tr, bl, br] {
        let hrect = egui::Rect::from_center_size(corner, egui::Vec2::splat(HANDLE_SIZE));
        painter.rect_filled(hrect, 2.0, egui::Color32::WHITE);
        painter.rect_stroke(
            hrect,
            2.0,
            egui::Stroke::new(1.5, accent),
            egui::StrokeKind::Inside,
        );
    }

    if state.crop_mode {
        show_drag_tag(
            ui,
            egui::pos2(tl.x, tl.y - 34.0),
            "Crop: drag the corners; click Done to finish".to_owned(),
        );
    }
}

/// Cuadrícula adaptativa sobre la página (paso elegido para ~24 px de
/// pantalla como mínimo entre líneas).
fn draw_grid(
    state: &EditorState,
    ui: &egui::Ui,
    coord: egui::Rect,
    clip: egui::Rect,
    page: (f64, f64),
) {
    const STEPS: [f64; 10] = [1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0, 1000.0];
    let zoom = state.viewport.zoom;
    let Some(step) = STEPS.iter().copied().find(|s| s * zoom >= 24.0) else {
        return;
    };
    let painter = ui.painter_at(clip);
    let stroke = egui::Stroke::new(1.0, ui.visuals().text_color().gamma_multiply(0.15));
    let (pw, ph) = page;

    let mut x = 0.0;
    while x <= pw {
        let a = page_to_screen(&state.viewport, coord, x, 0.0);
        let b = page_to_screen(&state.viewport, coord, x, ph);
        painter.line_segment([a, b], stroke);
        x += step;
    }
    let mut y = 0.0;
    while y <= ph {
        let a = page_to_screen(&state.viewport, coord, 0.0, y);
        let b = page_to_screen(&state.viewport, coord, pw, y);
        painter.line_segment([a, b], stroke);
        y += step;
    }
}

/// Reglas superior e izquierda con marcas y números en píxeles de página.
/// `coord`/`clip`: ver `draw_selection_overlay`. Las barras de las reglas y
/// el rango visible que miden son siempre del viewport real (`clip`); solo
/// las marcas y números usan `coord` para traducir a píxeles de página.
fn draw_rulers(state: &EditorState, ui: &egui::Ui, coord: egui::Rect, clip: egui::Rect) {
    const THICKNESS: f32 = 18.0;
    const STEPS: [f64; 10] = [1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0, 1000.0];
    let zoom = state.viewport.zoom;
    let Some(step) = STEPS.iter().copied().find(|s| s * zoom >= 56.0) else {
        return;
    };
    let painter = ui.painter_at(clip);
    let bg = ui.visuals().extreme_bg_color.gamma_multiply(0.9);
    let fg = ui.visuals().text_color().gamma_multiply(0.75);
    let tick_stroke = egui::Stroke::new(1.0, fg);
    let font = egui::FontId::proportional(9.5);

    let top = egui::Rect::from_min_max(clip.min, egui::pos2(clip.max.x, clip.min.y + THICKNESS));
    let left = egui::Rect::from_min_max(clip.min, egui::pos2(clip.min.x + THICKNESS, clip.max.y));
    painter.rect_filled(top, 0.0, bg);
    painter.rect_filled(left, 0.0, bg);

    // Rango de página visible en el lienzo.
    let (x0, y0) = screen_to_page(&state.viewport, coord, clip.min);
    let (x1, y1) = screen_to_page(&state.viewport, coord, clip.max);

    let mut x = (x0 / step).floor() * step;
    while x <= x1 {
        let sx = page_to_screen(&state.viewport, coord, x, 0.0).x;
        painter.line_segment(
            [
                egui::pos2(sx, top.bottom() - 6.0),
                egui::pos2(sx, top.bottom()),
            ],
            tick_stroke,
        );
        painter.text(
            egui::pos2(sx + 3.0, top.top() + 1.0),
            egui::Align2::LEFT_TOP,
            format!("{x:.0}"),
            font.clone(),
            fg,
        );
        x += step;
    }
    let mut y = (y0 / step).floor() * step;
    while y <= y1 {
        let sy = page_to_screen(&state.viewport, coord, 0.0, y).y;
        painter.line_segment(
            [
                egui::pos2(left.right() - 6.0, sy),
                egui::pos2(left.right(), sy),
            ],
            tick_stroke,
        );
        painter.text(
            egui::pos2(left.left() + 1.0, sy + 2.0),
            egui::Align2::LEFT_TOP,
            format!("{y:.0}"),
            font.clone(),
            fg,
        );
        y += step;
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use canvas_core::{ImageContent, ShapeContent};
    use canvas_io::LoadedImage;

    use super::*;

    fn loaded_image(width: u32, height: u32) -> LoadedImage {
        LoadedImage {
            rgba: vec![0u8; (width * height * 4) as usize],
            width,
            height,
        }
    }

    #[test]
    fn pasting_into_an_empty_canvas_expands_and_adds_a_blurred_background() {
        let mut state = EditorState::new_blank(1080.0, 1080.0);
        state.add_image_layer("Pasted Image", None, loaded_image(540, 960));

        let page = state.doc.page().unwrap();
        assert_eq!(page.layers.len(), 2);

        // Fondo: al fondo de la pila, cubre la página entera y desenfocado.
        let bg = &page.layers[0];
        assert_eq!(bg.name, "Blurred background");
        assert_eq!(bg.effects.blur_radius, 50.0);
        assert!(bg.transform.width >= page.width - 1e-6);
        assert!(bg.transform.height >= page.height - 1e-6);
        assert_eq!(Some(bg.id), state.background_layer);

        // Imagen: se amplía hasta tocar el alto (9:16 en página 1:1), centrada.
        let fg = &page.layers[1];
        assert!((fg.transform.height - page.height).abs() < 1e-6);
        assert!(fg.transform.width < page.width);
        assert!((fg.transform.x - (page.width - fg.transform.width) / 2.0).abs() < 1e-6);
        assert_eq!(state.selection.primary(), Some(fg.id));

        // Un solo Ctrl+Z deja el lienzo vacío otra vez (imagen + fondo).
        state.undo();
        assert!(state.doc.page().unwrap().layers.is_empty());
        assert!(state.selection.primary().is_none());
    }

    #[test]
    fn pasting_a_square_image_into_a_matching_square_canvas_skips_the_background() {
        let mut state = EditorState::new_blank(1000.0, 1000.0);
        state.add_image_layer("Pasted Image", None, loaded_image(500, 500));

        let page = state.doc.page().unwrap();
        assert_eq!(page.layers.len(), 1);
        assert_eq!(state.background_layer, None);
        let fg = &page.layers[0];
        assert_eq!((fg.transform.width, fg.transform.height), (1000.0, 1000.0));
    }

    #[test]
    fn replacing_image_preserves_transform_and_undo_restores_the_old_layer() {
        let mut state = EditorState::new_blank(1000.0, 1000.0);
        state.add_image_layer("Original", None, loaded_image(500, 500));
        let original_layer = state.doc.page().unwrap().layers[0].clone();

        state
            .replace_image_layer(original_layer.id, None, loaded_image(250, 500))
            .unwrap();

        let page = state.doc.page().unwrap();
        assert_eq!(page.layers.len(), 1);
        let replaced = &page.layers[0];
        assert_ne!(replaced.id, original_layer.id);
        assert_eq!(replaced.name, original_layer.name);
        assert_eq!(replaced.transform, original_layer.transform);
        assert_eq!(state.selection.primary(), Some(replaced.id));
        match &replaced.content {
            LayerContent::Image(content) => {
                assert_eq!((content.natural_width, content.natural_height), (250, 500));
                assert_eq!(content.crop, None);
            }
            _ => panic!("replacement must stay an image layer"),
        }

        state.undo();

        let page = state.doc.page().unwrap();
        assert_eq!(page.layers.len(), 1);
        let restored = &page.layers[0];
        assert_eq!(restored.id, original_layer.id);
        assert_eq!(restored.transform, original_layer.transform);
        match &restored.content {
            LayerContent::Image(content) => {
                assert_eq!((content.natural_width, content.natural_height), (500, 500));
            }
            _ => panic!("undo must restore the original image layer"),
        }
        assert!(state.images.contains_key(&original_layer.id));
    }

    /// Deshacer un borrado de archivo real (`GlobalStep::Delete`, apilado
    /// vía `record_delete` tras un «Delete» del usuario) no pertenece a
    /// ninguna ranura: se resuelve de inmediato — dejando la restauración
    /// pedida en `pending_restore` — sin esperar ningún salto de baraja, y
    /// sin dejar rastro en `global_redo` (no se "rehace" volver a borrar).
    #[test]
    fn undoing_a_delete_step_requests_a_restore_without_touching_redo() {
        let mut state = EditorState::new_blank(10.0, 10.0);
        state.pending_creation = false; // no es lo que se está probando aquí
        let record = DeleteRecord {
            path: PathBuf::from("C:/photos/cat.png"),
            sidecar: Some(PathBuf::from("C:/photos/.canvas/cat.png.canvas")),
        };
        state.record_delete(record.clone());
        assert!(state.can_undo());

        state.undo();

        assert_eq!(state.pending_restore, Some(record));
        assert!(!state.can_undo(), "el paso de borrado ya se resolvió");
        assert!(
            !state.can_redo(),
            "deshacer un borrado no deja nada que rehacer"
        );
    }

    /// El borrado que dispara `finish_pending_global_undo` al deshacer una
    /// creación (`GlobalStep::Create`) queda marcado como "no venía de un
    /// clic del usuario" (`pending_delete_from_undo`) — es la señal que lee
    /// `main.rs` para NO apilarle a su vez un `GlobalStep::Delete`: si lo
    /// hiciera, un `Ctrl+Z` más adelante podría "deshacer el deshacer" y
    /// restaurar un lienzo que el propio usuario decidió descartar.
    #[test]
    fn finishing_a_pending_create_undo_marks_the_delete_as_not_user_initiated() {
        let mut state = EditorState::new_blank(10.0, 10.0);
        state.active_slot_id = 1;
        state.pending_creation = false;
        state.pending_global_undo = Some(GlobalStep::Create(1));

        assert!(!state.pending_delete_from_undo);
        state.finish_pending_global_undo();

        assert!(state.delete_requested);
        assert!(state.pending_delete_from_undo);
    }

    /// Un lienzo recién creado (`new_blank`) nace con `pending_creation`
    /// activo y SIN nada que deshacer todavía: hasta que no se edita de
    /// verdad, "crear" no es un paso — evita que un relleno automático de la
    /// baraja que nadie llega a tocar se cuele como un paso de deshacer
    /// fantasma (la causa del bug: registrar la creación en cada sitio
    /// donde podía aparecer una ranura, en vez de en su primera edición
    /// real, dejaba huecos y duplicados según la carrera entre clics del
    /// usuario y el relleno asíncrono).
    #[test]
    fn a_freshly_created_canvas_has_nothing_to_undo_until_its_first_edit() {
        let state = EditorState::new_blank(10.0, 10.0);
        assert!(state.pending_creation);
        assert!(!state.can_undo());
    }

    /// La primera edición real de un lienzo recién creado antepone su
    /// `GlobalStep::Create` en la pila global: dos `Ctrl+Z` deshacen esa
    /// UNA edición y LUEGO piden borrar la ranura entera — no las dos
    /// cosas de golpe con un solo `Ctrl+Z` (el bug reportado).
    #[test]
    fn first_edit_of_a_freshly_created_canvas_records_its_creation_too() {
        let mut state = EditorState::new_blank(10.0, 10.0);
        let id = state
            .doc
            .add_layer(
                "a",
                Transform::new(0.0, 0.0, 1.0, 1.0),
                LayerContent::Image(ImageContent {
                    source_path: None,
                    natural_width: 1,
                    natural_height: 1,
                    crop: None,
                }),
            )
            .unwrap();
        state.active_slot_id = 2;

        state.doc.layer_mut(id).unwrap().name = "b".to_string();
        state.push_undo_step(Box::new(canvas_core::Rename {
            layer: id,
            before: "a".to_string(),
            after: "b".to_string(),
        }));
        assert!(
            !state.pending_creation,
            "se consume en la primera edición real"
        );

        // Ctrl+Z #1: deshace SOLO la edición; la ranura sigue viva.
        state.undo();
        assert!(state.pending_global_undo.is_none());
        assert_eq!(state.doc.layer(id).unwrap().name, "a");
        assert!(!state.delete_requested);
        assert!(
            state.can_undo(),
            "todavía queda el paso de creación por deshacer"
        );

        // Ctrl+Z #2: ahora sí, pide borrar la ranura entera.
        state.undo();
        assert_eq!(state.pending_global_undo, Some(GlobalStep::Create(2)));
        state.finish_pending_global_undo();
        assert!(
            state.delete_requested,
            "debe pedir borrar la ranura recién creada"
        );
        assert!(
            !state.can_undo(),
            "crear no deja nada más que deshacer detrás"
        );
    }

    /// Simula "editar el diseño 1, luego el 3, luego el 1 otra vez" con
    /// `active_slot_id` (una sola `EditorState`/`Document` de sobra para
    /// probar el ORDEN cruzado — el salto real de baraja lo cubre
    /// `deck::apply_jump` por separado). Comprueba que deshacer tres veces
    /// reproduce el orden cronológico real (1, 3, 1) pidiendo el salto
    /// correspondiente cada vez que le toca a un diseño que no es el
    /// activo, y que rehacer reconstruye el mismo cruce en sentido inverso.
    #[test]
    fn global_undo_and_redo_replay_steps_in_true_chronological_order_across_designs() {
        let mut state = EditorState::new_blank(10.0, 10.0);
        let id = state
            .doc
            .add_layer(
                "a",
                Transform::new(0.0, 0.0, 1.0, 1.0),
                LayerContent::Image(ImageContent {
                    source_path: None,
                    natural_width: 1,
                    natural_height: 1,
                    crop: None,
                }),
            )
            .unwrap();
        // Este test es sobre el orden entre `Edit`, no sobre `Create` (ya
        // cubierto aparte) — se apaga para no mezclar ambos.
        state.pending_creation = false;
        let rename = |state: &mut EditorState, before: &str, after: &str| {
            state.doc.layer_mut(id).unwrap().name = after.to_string();
            state.push_undo_step(Box::new(canvas_core::Rename {
                layer: id,
                before: before.to_string(),
                after: after.to_string(),
            }));
        };
        let name = |state: &EditorState| state.doc.layer(id).unwrap().name.clone();

        state.active_slot_id = 1;
        rename(&mut state, "a", "b"); // diseño 1
        state.active_slot_id = 3;
        rename(&mut state, "b", "c"); // diseño 3
        state.active_slot_id = 1;
        rename(&mut state, "c", "d"); // diseño 1 otra vez

        // El paso más reciente es del diseño activo (1): deshace en el sitio.
        state.undo();
        assert!(state.pending_global_undo.is_none());
        assert_eq!(name(&state), "c");

        // El siguiente le toca al diseño 3: pide el salto SIN tocar el
        // documento todavía.
        state.undo();
        assert_eq!(state.pending_global_undo, Some(GlobalStep::Edit(3)));
        assert_eq!(name(&state), "c");

        // `main.rs` completó el salto de baraja al diseño 3.
        state.active_slot_id = 3;
        state.finish_pending_global_undo();
        assert_eq!(name(&state), "b");

        // Queda el primer paso, del diseño 1: pide saltar de vuelta.
        state.undo();
        assert_eq!(state.pending_global_undo, Some(GlobalStep::Edit(1)));
        state.active_slot_id = 1;
        state.finish_pending_global_undo();
        assert_eq!(name(&state), "a");
        assert!(!state.can_undo());

        // Rehacer reproduce el mismo cruce de diseños en sentido inverso.
        state.redo();
        assert_eq!(name(&state), "b");
        state.redo();
        assert_eq!(state.pending_global_redo, Some(GlobalStep::Edit(3)));
        state.active_slot_id = 3;
        state.finish_pending_global_redo();
        assert_eq!(name(&state), "c");
        state.redo();
        assert_eq!(state.pending_global_redo, Some(GlobalStep::Edit(1)));
        state.active_slot_id = 1;
        state.finish_pending_global_redo();
        assert_eq!(name(&state), "d");
        assert!(!state.can_redo());
    }

    #[test]
    fn pasting_into_a_non_empty_canvas_keeps_the_old_contain_behavior() {
        let mut state = EditorState::new_blank(1080.0, 1080.0);
        // Deja el lienzo no vacío con una capa que no es una imagen.
        state.insert_layer_centered(
            "Rect",
            100.0,
            100.0,
            LayerContent::Shape(ShapeContent::default()),
        );

        state.add_image_layer("Pasted Image", None, loaded_image(540, 960));

        let page = state.doc.page().unwrap();
        assert_eq!(page.layers.len(), 2);
        assert_eq!(state.background_layer, None);

        // Nunca se amplía sobre un lienzo no vacío: 960 < 1080, cabe sin
        // escalar, y "contain" no se aplica (comportamiento de siempre).
        let fg = &page.layers[1];
        assert_eq!((fg.transform.width, fg.transform.height), (540.0, 960.0));
    }
}
