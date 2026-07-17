//! Estado y UI del editor: el lienzo con zoom/paneo y el panel de propiedades.

use std::path::PathBuf;

use canvas_core::{
    resize_from_corner, CoreError, Corner, Document, History, ImageContent, LayerContent, LayerId,
    SetTransform, Transform,
};
use canvas_io::LoadedImage;
use canvas_render::{image_data_from_rgba, CanvasRenderer, ImageMap};
use eframe::egui;
use eframe::egui_wgpu::RenderState;
use vello::kurbo::Affine;

use crate::surface::CanvasSurface;

const MIN_ZOOM: f64 = 0.02;
const MAX_ZOOM: f64 = 32.0;

pub struct Viewport {
    /// Factor página → puntos de pantalla.
    pub zoom: f64,
    /// Desplazamiento del origen de la página, en puntos, relativo al lienzo.
    pub pan: egui::Vec2,
    needs_fit: bool,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            pan: egui::Vec2::ZERO,
            needs_fit: true,
        }
    }
}

impl Viewport {
    fn fit(&mut self, page: (f64, f64), avail: egui::Vec2) {
        const MARGIN: f32 = 24.0;
        let (pw, ph) = page;
        if pw <= 0.0 || ph <= 0.0 {
            return;
        }
        let usable_w = f64::from((avail.x - 2.0 * MARGIN).max(32.0));
        let usable_h = f64::from((avail.y - 2.0 * MARGIN).max(32.0));
        self.zoom = (usable_w / pw).min(usable_h / ph).clamp(MIN_ZOOM, MAX_ZOOM);
        self.pan = egui::vec2(
            ((f64::from(avail.x) - pw * self.zoom) / 2.0) as f32,
            ((f64::from(avail.y) - ph * self.zoom) / 2.0) as f32,
        );
        self.needs_fit = false;
    }

    fn zoom_at(&mut self, anchor: egui::Vec2, factor: f64) {
        let new_zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        let applied = new_zoom / self.zoom;
        self.pan = anchor - (anchor - self.pan) * applied as f32;
        self.zoom = new_zoom;
    }
}

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
}

pub struct EditorState {
    pub doc: Document,
    pub history: History,
    pub images: ImageMap,
    pub selected: Option<LayerId>,
    pub viewport: Viewport,
    /// Proporción bloqueada al redimensionar (por defecto sí; `Shift` la libera).
    pub aspect_lock: bool,
    gesture: Gesture,
    /// Edición en curso desde el panel (campos numéricos): capa y transform
    /// original, para consolidar en un solo comando al terminar.
    panel_edit: Option<(LayerId, Transform)>,
    /// Ajuste de desenfoque en curso (slider): capa y radio original.
    blur_edit: Option<(LayerId, f32)>,
    /// Hay un guardado en curso en un hilo de trabajo.
    pub saving: bool,
    /// Último error de guardado, visible hasta descartarlo.
    pub save_error: Option<String>,
    /// Galería de la que se abrió este documento, si procede de una.
    pub from_gallery: Option<PathBuf>,
    /// El usuario ha pulsado «Volver a la galería»; la app decide cómo.
    pub return_requested: bool,
}

impl EditorState {
    /// Documento nuevo a partir de una imagen: página a sus dimensiones
    /// reales y la imagen como capa a tamaño completo.
    pub fn from_image(path: PathBuf, img: LoadedImage) -> Result<Self, CoreError> {
        let (w, h) = (f64::from(img.width), f64::from(img.height));
        let mut doc = Document::new(w, h);
        doc.source_path = Some(path.clone());
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Imagen".to_owned());
        let id = doc.add_layer(
            name,
            Transform::new(0.0, 0.0, w, h),
            LayerContent::Image(ImageContent {
                source_path: Some(path),
                natural_width: img.width,
                natural_height: img.height,
            }),
        )?;
        let mut images = ImageMap::new();
        images.insert(id, image_data_from_rgba(img.rgba, img.width, img.height));
        Ok(Self {
            doc,
            history: History::default(),
            images,
            selected: Some(id),
            viewport: Viewport::default(),
            aspect_lock: true,
            gesture: Gesture::None,
            panel_edit: None,
            blur_edit: None,
            saving: false,
            save_error: None,
            from_gallery: None,
            return_requested: false,
        })
    }

    /// Atajos de edición globales del editor (deshacer/rehacer).
    pub fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        use egui::{Key, KeyboardShortcut, Modifiers};
        // El orden importa: Ctrl+Shift+Z debe consumirse antes que Ctrl+Z.
        let redo = ctx.input_mut(|i| {
            i.consume_shortcut(&KeyboardShortcut::new(
                Modifiers::COMMAND | Modifiers::SHIFT,
                Key::Z,
            )) || i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::Y))
        });
        let undo = ctx
            .input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::Z)));
        if redo {
            if let Err(e) = self.history.redo(&mut self.doc) {
                tracing::error!("rehacer falló: {e}");
            }
        } else if undo {
            if let Err(e) = self.history.undo(&mut self.doc) {
                tracing::error!("deshacer falló: {e}");
            }
        }
    }

    pub fn file_name(&self) -> String {
        self.doc
            .source_path
            .as_deref()
            .and_then(|p| p.file_name())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Sin título".to_owned())
    }

    pub fn is_dirty(&self) -> bool {
        self.history.is_dirty()
    }
}

/// Panel derecho: propiedades de la capa seleccionada.
pub fn properties_ui(state: &mut EditorState, ui: &mut egui::Ui) {
    ui.add_space(8.0);
    if state.from_gallery.is_some() && ui.button("⏴ Volver a la galería").clicked() {
        state.return_requested = true;
    }
    ui.heading(state.file_name());
    let page_dims = match state.doc.page() {
        Ok(p) => (p.width, p.height),
        Err(_) => (0.0, 0.0),
    };
    ui.weak(format!(
        "{} × {} px",
        page_dims.0 as i64, page_dims.1 as i64
    ));
    ui.separator();

    if let Some(sel) = state.selected {
        if state.doc.layer(sel).is_ok() {
            layer_properties_ui(state, ui, sel, page_dims);
        }
    } else {
        ui.weak("Ninguna capa seleccionada.");
        ui.weak("Haz clic sobre la imagen para seleccionarla.");
    }

    ui.separator();
    if state.saving {
        ui.horizontal(|ui| {
            ui.add(egui::Spinner::new());
            ui.label("Guardando…");
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
    ui.weak("Rueda: zoom · Espacio/botón central: paneo · Ctrl+0: ajustar");
    ui.weak("Ctrl+S: guardar · Ctrl+Shift+S: guardar como");
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
    let original = layer.transform;
    let natural = match &layer.content {
        LayerContent::Image(img) => (f64::from(img.natural_width), f64::from(img.natural_height)),
    };
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
    ui.label("Posición");
    ui.horizontal(|ui| {
        ui.label("X");
        changed |= track(ui.add(egui::DragValue::new(&mut t.x).speed(1.0).max_decimals(1)));
        ui.label("Y");
        changed |= track(ui.add(egui::DragValue::new(&mut t.y).speed(1.0).max_decimals(1)));
    });

    ui.add_space(6.0);

    // --- Tamaño ---
    ui.horizontal(|ui| {
        ui.label("Tamaño");
        let lock_icon = if state.aspect_lock { "🔒" } else { "🔓" };
        if ui
            .selectable_label(state.aspect_lock, lock_icon)
            .on_hover_text("Proporción bloqueada (Shift al arrastrar la invierte)")
            .clicked()
        {
            state.aspect_lock = !state.aspect_lock;
        }
    });
    let ratio = original.aspect_ratio();
    ui.horizontal(|ui| {
        ui.label("An");
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
        ui.label("Al");
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
            ui.label("Escala");
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

    ui.add_space(8.0);

    // --- Desenfoque (no destructivo, vista previa en vivo) ---
    ui.label("Desenfoque");
    {
        let current_blur = state
            .doc
            .layer(sel)
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
                    state.blur_edit = Some((sel, current_blur));
                }
                if let Ok(l) = state.doc.layer_mut(sel) {
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
                        state.history.push_applied(Box::new(canvas_core::SetBlur {
                            layer: id,
                            before,
                            after,
                        }));
                    }
                }
            }
            if current_blur > 0.0 && ui.button("Quitar").clicked() {
                if let Err(e) = state.history.apply(
                    &mut state.doc,
                    Box::new(canvas_core::SetBlur {
                        layer: sel,
                        before: current_blur,
                        after: 0.0,
                    }),
                ) {
                    tracing::error!("quitar desenfoque falló: {e}");
                }
            }
        });
    }

    ui.add_space(8.0);

    // --- Alineación respecto a la página ---
    ui.label("Alinear en la página");
    let mut aligned: Option<Transform> = None;
    ui.horizontal(|ui| {
        if ui.button("⏴ Izq").clicked() {
            aligned = Some(canvas_core::align_horizontal(
                &t,
                page_w,
                canvas_core::HAlign::Left,
            ));
        }
        if ui.button("↔ Centro").clicked() {
            aligned = Some(canvas_core::align_horizontal(
                &t,
                page_w,
                canvas_core::HAlign::Center,
            ));
        }
        if ui.button("Der ⏵").clicked() {
            aligned = Some(canvas_core::align_horizontal(
                &t,
                page_w,
                canvas_core::HAlign::Right,
            ));
        }
    });
    ui.horizontal(|ui| {
        if ui.button("⏶ Arriba").clicked() {
            aligned = Some(canvas_core::align_vertical(
                &t,
                page_h,
                canvas_core::VAlign::Top,
            ));
        }
        if ui.button("↕ Medio").clicked() {
            aligned = Some(canvas_core::align_vertical(
                &t,
                page_h,
                canvas_core::VAlign::Middle,
            ));
        }
        if ui.button("Abajo ⏷").clicked() {
            aligned = Some(canvas_core::align_vertical(
                &t,
                page_h,
                canvas_core::VAlign::Bottom,
            ));
        }
    });
    if ui.button("◎ Centrar en la página").clicked() {
        let centered = canvas_core::align_horizontal(&t, page_w, canvas_core::HAlign::Center);
        aligned = Some(canvas_core::align_vertical(
            &centered,
            page_h,
            canvas_core::VAlign::Middle,
        ));
    }

    // --- Aplicar cambios ---
    if let Some(after) = aligned {
        // Botón de alineación: comando inmediato (consolidando cualquier
        // edición de campo pendiente como parte del mismo paso).
        let before = state.panel_edit.take().map_or(original, |(_, b)| b);
        if after != before {
            if let Err(e) = state.history.apply(
                &mut state.doc,
                Box::new(SetTransform {
                    layer: sel,
                    before,
                    after,
                }),
            ) {
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
    if commit {
        if let Some((id, before)) = state.panel_edit.take() {
            if let Ok(l) = state.doc.layer(id) {
                let after = l.transform;
                if after != before {
                    state.history.push_applied(Box::new(SetTransform {
                        layer: id,
                        before,
                        after,
                    }));
                }
            }
        }
    }
}

/// El lienzo: gestiona zoom/paneo, renderiza el documento con vello y lo pinta.
pub fn canvas_ui(
    state: &mut EditorState,
    ui: &mut egui::Ui,
    rs: &RenderState,
    renderer: &mut CanvasRenderer,
    surface_slot: &mut Option<CanvasSurface>,
) {
    let avail = ui.available_size();
    let (rect, response) = ui.allocate_exact_size(avail, egui::Sense::click_and_drag());
    if rect.width() < 1.0 || rect.height() < 1.0 {
        return;
    }

    let page_dims = match state.doc.page() {
        Ok(p) => (p.width, p.height),
        Err(_) => (1.0, 1.0),
    };

    // Ajustar a ventana: Ctrl/Cmd+0 o primer frame.
    let fit_requested = ui.ctx().input_mut(|i| {
        i.consume_shortcut(&egui::KeyboardShortcut::new(
            egui::Modifiers::COMMAND,
            egui::Key::Num0,
        ))
    });
    if fit_requested || state.viewport.needs_fit {
        state.viewport.fit(page_dims, rect.size());
    }

    // Zoom con rueda (y pellizco), anclado al cursor.
    if response.hovered() {
        let (scroll, pinch, pointer) = ui.ctx().input(|i| {
            (
                i.smooth_scroll_delta.y,
                i.zoom_delta(),
                i.pointer.hover_pos(),
            )
        });
        let mut factor = f64::from(pinch);
        if scroll != 0.0 {
            factor *= (f64::from(scroll) * 0.0025).exp();
        }
        if (factor - 1.0).abs() > 1e-4 {
            let anchor = pointer.map_or(rect.size() / 2.0, |p| p - rect.min);
            state.viewport.zoom_at(anchor, factor);
        }
    }

    // Paneo: botón central, o espacio + arrastre primario.
    let space_down = ui.ctx().input(|i| i.key_down(egui::Key::Space));
    let panning = response.dragged_by(egui::PointerButton::Middle)
        || (space_down && response.dragged_by(egui::PointerButton::Primary));
    if panning {
        state.viewport.pan += response.drag_delta();
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    } else if space_down && response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }

    // Selección, arrastre y redimensionado (si no se está paneando).
    if !panning && !space_down {
        layer_interaction(state, ui, &response, rect);
    }

    // Render vello → textura del tamaño físico del lienzo.
    let ppp = ui.ctx().pixels_per_point();
    let width = (rect.width() * ppp).round().max(1.0) as u32;
    let height = (rect.height() * ppp).round().max(1.0) as u32;
    let surface = CanvasSurface::ensure(surface_slot, rs, width, height);

    // Sincroniza el desenfoque GPU de cada capa antes de montar la escena.
    if let Ok(page) = state.doc.page() {
        let blur_targets: Vec<(LayerId, f32)> = page
            .layers
            .iter()
            .map(|l| (l.id, l.effects.blur_radius))
            .collect();
        for (id, radius) in blur_targets {
            if let Some(source) = state.images.get(&id) {
                renderer.sync_layer_blur(&rs.device, &rs.queue, id, source, radius);
            }
        }
    }

    let view = Affine::translate((
        f64::from(state.viewport.pan.x * ppp),
        f64::from(state.viewport.pan.y * ppp),
    )) * Affine::scale(state.viewport.zoom * f64::from(ppp));
    let blurred = renderer.blur_overrides();
    let scene = canvas_render::build_scene(&state.doc, &state.images, &blurred, view, true);
    if let Err(e) = surface.render(rs, renderer, &scene) {
        tracing::error!("fallo renderizando el lienzo: {e}");
    }

    ui.painter().image(
        surface.egui_id(),
        rect,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );

    draw_selection_overlay(state, ui, rect);
}

const HANDLE_SIZE: f32 = 9.0;
const ACCENT: egui::Color32 = egui::Color32::from_rgb(0, 122, 255);

fn page_to_screen(vp: &Viewport, rect: egui::Rect, x: f64, y: f64) -> egui::Pos2 {
    rect.min + vp.pan + egui::vec2((x * vp.zoom) as f32, (y * vp.zoom) as f32)
}

fn screen_to_page(vp: &Viewport, rect: egui::Rect, pos: egui::Pos2) -> (f64, f64) {
    let local = pos - rect.min - vp.pan;
    (f64::from(local.x) / vp.zoom, f64::from(local.y) / vp.zoom)
}

/// Rect en pantalla de una capa.
fn layer_screen_rect(vp: &Viewport, rect: egui::Rect, t: &Transform) -> egui::Rect {
    egui::Rect::from_min_max(
        page_to_screen(vp, rect, t.x, t.y),
        page_to_screen(vp, rect, t.x + t.width, t.y + t.height),
    )
}

/// Los cuatro manejadores de esquina de un rect de pantalla.
fn corner_handles(r: egui::Rect) -> [(Corner, egui::Rect); 4] {
    let h = |p: egui::Pos2| egui::Rect::from_center_size(p, egui::Vec2::splat(HANDLE_SIZE));
    [
        (Corner::TopLeft, h(r.left_top())),
        (Corner::TopRight, h(r.right_top())),
        (Corner::BottomLeft, h(r.left_bottom())),
        (Corner::BottomRight, h(r.right_bottom())),
    ]
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
    if let (Some(pos), Some(sel)) = (pointer, state.selected) {
        if let Ok(layer) = state.doc.layer(sel) {
            let srect = layer_screen_rect(&state.viewport, rect, &layer.transform);
            let on_handle = corner_handles(srect)
                .into_iter()
                .find(|(_, hr)| hr.expand(2.0).contains(pos));
            if let Some((corner, _)) = on_handle {
                let icon = match corner {
                    Corner::TopLeft | Corner::BottomRight => egui::CursorIcon::ResizeNwSe,
                    Corner::TopRight | Corner::BottomLeft => egui::CursorIcon::ResizeNeSw,
                };
                ui.ctx().set_cursor_icon(icon);
            } else if srect.contains(pos) && matches!(state.gesture, Gesture::None) {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Move);
            }
        }
    }

    // Inicio de gesto.
    if response.drag_started_by(egui::PointerButton::Primary) {
        if let Some(pos) = response.interact_pointer_pos() {
            state.gesture = Gesture::None;
            // ¿Sobre un manejador de la selección actual?
            if let Some(sel) = state.selected {
                if let Ok(layer) = state.doc.layer(sel) {
                    let srect = layer_screen_rect(&state.viewport, rect, &layer.transform);
                    if let Some((corner, _)) = corner_handles(srect)
                        .into_iter()
                        .find(|(_, hr)| hr.expand(2.0).contains(pos))
                    {
                        state.gesture = Gesture::Resize {
                            layer: sel,
                            corner,
                            start: layer.transform,
                            origin: pos,
                        };
                    }
                }
            }
            // Si no, ¿sobre una capa? (selecciona y empieza a mover)
            if matches!(state.gesture, Gesture::None) {
                let (px, py) = screen_to_page(&state.viewport, rect, pos);
                let hit = state.doc.page().ok().and_then(|p| p.layer_at(px, py));
                state.selected = hit;
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
                    if let Ok(l) = state.doc.layer_mut(layer) {
                        l.transform.x = start.x + dx;
                        l.transform.y = start.y + dy;
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
                    let t = resize_from_corner(&start, corner, dx, dy, keep_aspect, 1.0);
                    if let Ok(l) = state.doc.layer_mut(layer) {
                        l.transform = t;
                    }
                    // Dimensiones en píxeles junto al cursor mientras se arrastra.
                    show_dims_tag(ui, pos, &t);
                }
                Gesture::None => {}
            }
        }
    }

    // Fin de gesto: consolida en UN comando de deshacer.
    if response.drag_stopped_by(egui::PointerButton::Primary) {
        match std::mem::replace(&mut state.gesture, Gesture::None) {
            Gesture::Move { layer, start, .. } | Gesture::Resize { layer, start, .. } => {
                if let Ok(l) = state.doc.layer(layer) {
                    let after = l.transform;
                    if after != start {
                        state.history.push_applied(Box::new(SetTransform {
                            layer,
                            before: start,
                            after,
                        }));
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
            state.selected = state.doc.page().ok().and_then(|p| p.layer_at(px, py));
        }
    }
}

/// Etiqueta «ancho × alto px» junto al cursor durante el redimensionado.
fn show_dims_tag(ui: &egui::Ui, pos: egui::Pos2, t: &Transform) {
    let text = format!(
        "{} × {} px",
        t.width.round() as i64,
        t.height.round() as i64
    );
    let painter = ui.painter();
    let galley =
        painter.layout_no_wrap(text, egui::FontId::proportional(12.0), egui::Color32::WHITE);
    let tag_pos = pos + egui::vec2(14.0, 16.0);
    let bg = egui::Rect::from_min_size(tag_pos, galley.size() + egui::vec2(10.0, 6.0));
    painter.rect_filled(bg, 4.0, egui::Color32::from_black_alpha(190));
    painter.galley(tag_pos + egui::vec2(5.0, 3.0), galley, egui::Color32::WHITE);
}

/// Recuadro de selección y manejadores, pintados por encima del lienzo.
fn draw_selection_overlay(state: &EditorState, ui: &egui::Ui, rect: egui::Rect) {
    let Some(sel) = state.selected else { return };
    let Ok(layer) = state.doc.layer(sel) else {
        return;
    };
    let srect = layer_screen_rect(&state.viewport, rect, &layer.transform);
    let painter = ui.painter_at(rect);

    painter.rect_stroke(
        srect,
        0.0,
        egui::Stroke::new(1.5, ACCENT),
        egui::StrokeKind::Outside,
    );
    for (_, hrect) in corner_handles(srect) {
        painter.rect_filled(hrect, 2.0, egui::Color32::WHITE);
        painter.rect_stroke(
            hrect,
            2.0,
            egui::Stroke::new(1.5, ACCENT),
            egui::StrokeKind::Inside,
        );
    }
}
