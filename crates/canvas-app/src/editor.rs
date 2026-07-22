//! Estado y UI del editor: el lienzo con zoom/paneo y el panel de propiedades.

use std::path::PathBuf;

use canvas_core::{
    cover_transform, resize_rotated_from_corner, snap_translation, trim_crop_from_corner,
    uncrop_transform, CoreError, Corner, CropRect, Document, History, ImageContent, InsertLayer,
    Layer, LayerContent, LayerId, RemoveLayer, SetCrop, SetPageSize, SetTransform, Transform,
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

    /// Vuelve a ajustar la página a la ventana en el próximo frame.
    pub fn request_fit(&mut self) {
        self.needs_fit = true;
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
    /// Edición en curso del tamaño de página (campos An/Al de la sección
    /// Página): dimensiones originales, para consolidar al terminar.
    page_edit: Option<(f64, f64)>,
    /// Capa de «fondo desenfocado» activa, si la hay.
    background_layer: Option<LayerId>,
    /// Ajuste de desenfoque en curso (slider): capa y radio original.
    blur_edit: Option<(LayerId, f32)>,
    /// Ajuste de color en curso (sliders): capa y efectos originales, para
    /// consolidar los 6 sliders en un solo paso de deshacer.
    color_edit: Option<(LayerId, canvas_core::Effects)>,
    /// Edición de contenido en curso (texto/forma): capa y contenido original.
    content_edit: Option<(LayerId, LayerContent)>,
    /// Ajuste de sombra en curso: capa y sombra original.
    shadow_edit: Option<(LayerId, Option<canvas_core::Shadow>)>,
    /// Hay un guardado en curso en un hilo de trabajo.
    pub saving: bool,
    /// Último error de guardado, visible hasta descartarlo.
    pub save_error: Option<String>,
    /// Galería de la que se abrió este documento, si procede de una.
    pub from_gallery: Option<PathBuf>,
    /// El usuario ha pulsado «Volver a la galería»; la app decide cómo.
    pub return_requested: bool,
    /// Botón «Guardar» del panel pulsado (equivale a Ctrl+S).
    pub save_clicked: bool,
    /// Botón «Guardar como…» del panel pulsado (equivale a Ctrl+Shift+S).
    pub save_as_clicked: bool,
    /// Botón «Settings» del panel pulsado; la app abre la ventana de ajustes.
    pub settings_clicked: bool,
    /// Escribir el sidecar `.canvas` al guardar (preserva la editabilidad).
    pub sidecar_enabled: bool,
    /// ICC/EXIF del archivo original, para reinsertarlos al guardar.
    pub source_metadata: Option<canvas_io::ImageMetadata>,
    /// El archivo fuente cambió en disco fuera de la app (watcher).
    pub external_change: bool,
    /// El usuario pidió recargar desde disco en el banner de cambio externo.
    pub reload_requested: bool,
    /// Zoom pedido desde el menú (factor); se aplica anclado al centro del
    /// lienzo en el próximo frame, cuando se conoce su rect.
    pub pending_zoom_factor: Option<f64>,
    /// Cuadrícula y reglas (menú View).
    pub show_grid: bool,
    pub show_rulers: bool,
    /// Modo recorte activo: las esquinas recortan en vez de redimensionar.
    pub crop_mode: bool,
    /// Guías de alineación magnéticas activas durante un arrastre
    /// (posiciones de página: verticales, horizontales).
    snap_guides: (Vec<f64>, Vec<f64>),
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
            .unwrap_or_else(|| "Image".to_owned());
        let id = doc.add_layer(
            name,
            Transform::new(0.0, 0.0, w, h),
            LayerContent::Image(ImageContent {
                source_path: Some(path),
                natural_width: img.width,
                natural_height: img.height,
                crop: None,
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
            page_edit: None,
            background_layer: None,
            blur_edit: None,
            color_edit: None,
            content_edit: None,
            shadow_edit: None,
            saving: false,
            save_error: None,
            from_gallery: None,
            return_requested: false,
            save_clicked: false,
            save_as_clicked: false,
            settings_clicked: false,
            sidecar_enabled: true,
            source_metadata: None,
            external_change: false,
            reload_requested: false,
            pending_zoom_factor: None,
            show_grid: false,
            show_rulers: false,
            crop_mode: false,
            snap_guides: (Vec::new(), Vec::new()),
        })
    }

    /// Proyecto nuevo en blanco (página blanca, sin capas).
    pub fn new_blank(width: f64, height: f64) -> Self {
        let mut doc = Document::new(width, height);
        if let Ok(page) = doc.page_mut() {
            page.background = Some([255, 255, 255, 255]);
        }
        Self {
            doc,
            history: History::default(),
            images: ImageMap::new(),
            selected: None,
            viewport: Viewport::default(),
            aspect_lock: true,
            gesture: Gesture::None,
            panel_edit: None,
            page_edit: None,
            background_layer: None,
            blur_edit: None,
            color_edit: None,
            content_edit: None,
            shadow_edit: None,
            saving: false,
            save_error: None,
            from_gallery: None,
            return_requested: false,
            save_clicked: false,
            save_as_clicked: false,
            settings_clicked: false,
            sidecar_enabled: true,
            source_metadata: None,
            external_change: false,
            reload_requested: false,
            pending_zoom_factor: None,
            show_grid: false,
            show_rulers: false,
            crop_mode: false,
            snap_guides: (Vec::new(), Vec::new()),
        }
    }

    /// Documento restaurado desde un sidecar `.canvas`: las capas siguen
    /// siendo editables tal y como se guardaron (nada de fondo aplanado).
    pub fn from_restored(path: PathBuf, restored: canvas_io::RestoredDocument) -> Self {
        let mut doc = restored.document;
        doc.source_path = Some(path);
        let mut images = ImageMap::new();
        for (raw, pixels) in restored.images {
            images.insert(
                LayerId::from_raw(raw),
                image_data_from_rgba(pixels.rgba, pixels.width, pixels.height),
            );
        }
        let background_layer = restored.background_layer.map(LayerId::from_raw);
        // Selecciona la capa más alta que no sea el fondo desenfocado.
        let selected = doc.page().ok().and_then(|p| {
            p.layers
                .iter()
                .rev()
                .find(|l| Some(l.id) != background_layer)
                .or_else(|| p.layers.last())
                .map(|l| l.id)
        });
        Self {
            doc,
            history: History::default(), // recién abierto = sin cambios
            images,
            selected,
            viewport: Viewport::default(),
            aspect_lock: true,
            gesture: Gesture::None,
            panel_edit: None,
            page_edit: None,
            background_layer,
            blur_edit: None,
            color_edit: None,
            content_edit: None,
            shadow_edit: None,
            saving: false,
            save_error: None,
            from_gallery: None,
            return_requested: false,
            save_clicked: false,
            save_as_clicked: false,
            settings_clicked: false,
            sidecar_enabled: true,
            source_metadata: None,
            external_change: false,
            reload_requested: false,
            pending_zoom_factor: None,
            show_grid: false,
            show_rulers: false,
            crop_mode: false,
            snap_guides: (Vec::new(), Vec::new()),
        }
    }

    /// Datos para que el hilo de guardado escriba el sidecar: documento
    /// clonado y píxeles RGBA de cada capa.
    pub fn sidecar_payload(&self) -> crate::loader::SidecarPayload {
        let images = self
            .images
            .iter()
            .map(|(id, data)| (id.raw(), data.data.data().to_vec(), data.width, data.height))
            .collect();
        crate::loader::SidecarPayload {
            document: self.doc.clone(),
            images,
            background_layer: self.background_layer.map(|id| id.raw()),
        }
    }

    /// Añade una imagen como capa nueva encima de las existentes (deshacible),
    /// encajada en la página si es mayor que ella, y la selecciona.
    pub fn add_image_layer(&mut self, path: PathBuf, img: LoadedImage) {
        let Ok(page) = self.doc.page() else { return };
        let (pw, ph) = (page.width, page.height);
        let index = page.layers.len();

        let (nw, nh) = (f64::from(img.width), f64::from(img.height));
        let scale = (pw / nw).min(ph / nh).min(1.0);
        let (w, h) = (nw * scale, nh * scale);
        let transform = Transform::new((pw - w) / 2.0, (ph - h) / 2.0, w, h);

        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Image".to_owned());
        let id = self.doc.allocate_layer_id();
        let layer = Layer::new(
            id,
            name,
            transform,
            LayerContent::Image(ImageContent {
                source_path: Some(path),
                natural_width: img.width,
                natural_height: img.height,
                crop: None,
            }),
        );
        if let Err(e) = self
            .history
            .apply(&mut self.doc, Box::new(InsertLayer { index, layer }))
        {
            tracing::error!("añadir capa falló: {e}");
            return;
        }
        self.images
            .insert(id, image_data_from_rgba(img.rgba, img.width, img.height));
        self.selected = Some(id);
    }

    /// Inserta una capa nueva (texto o forma) centrada en la página,
    /// deshacible, y la selecciona.
    pub fn insert_layer_centered(&mut self, name: &str, w: f64, h: f64, content: LayerContent) {
        let Ok(page) = self.doc.page() else { return };
        let (pw, ph) = (page.width, page.height);
        let index = page.layers.len();
        let id = self.doc.allocate_layer_id();
        let layer = Layer::new(
            id,
            name,
            Transform::new((pw - w) / 2.0, (ph - h) / 2.0, w, h),
            content,
        );
        if let Err(e) = self
            .history
            .apply(&mut self.doc, Box::new(InsertLayer { index, layer }))
        {
            tracing::error!("insertar capa falló: {e}");
            return;
        }
        self.selected = Some(id);
        self.crop_mode = false;
    }

    /// ¿Está activa (y viva, tras posibles deshacer) la capa de fondo?
    fn background_active(&self) -> bool {
        self.background_layer
            .is_some_and(|id| self.doc.layer(id).is_ok())
    }

    /// Capa de imagen que serviría de fuente para el fondo desenfocado.
    fn background_source(&self) -> Option<LayerId> {
        let is_candidate = |l: &Layer| {
            matches!(l.content, LayerContent::Image(_)) && Some(l.id) != self.background_layer
        };
        // La seleccionada si vale; si no, la capa de imagen más alta.
        if let Some(sel) = self.selected {
            if let Ok(l) = self.doc.layer(sel) {
                if is_candidate(l) {
                    return Some(sel);
                }
            }
        }
        self.doc
            .page()
            .ok()?
            .layers
            .iter()
            .rev()
            .find(|l| is_candidate(l))
            .map(|l| l.id)
    }

    /// Activa/desactiva el fondo desenfocado (capa «cover» de la imagen
    /// fuente con blur 50 por defecto, insertada en el fondo de la pila).
    fn set_blurred_background(&mut self, on: bool) {
        if !on {
            if let Some(id) = self.background_layer.take() {
                if let Err(e) = self
                    .history
                    .apply(&mut self.doc, Box::new(RemoveLayer::new(id)))
                {
                    tracing::error!("quitar fondo falló: {e}");
                }
                // El ImageData se queda en el mapa a propósito: deshacer el
                // RemoveLayer recupera la capa y necesita sus píxeles.
            }
            return;
        }

        let Some(source_id) = self.background_source() else {
            return;
        };
        let Ok(source) = self.doc.layer(source_id) else {
            return;
        };
        let LayerContent::Image(content) = source.content.clone() else {
            return;
        };
        let source_t = source.transform;
        let Some(pixels) = self.images.get(&source_id).cloned() else {
            return;
        };
        let Ok(page) = self.doc.page() else { return };
        let (pw, ph) = (page.width, page.height);

        let mut commands: Vec<Box<dyn canvas_core::Command>> = Vec::new();

        // Si la imagen fuente tapa la página entera, el fondo no se vería:
        // encájala centrada (estilo CapCut) como parte del mismo paso.
        let covers_page = source_t.x <= 0.0
            && source_t.y <= 0.0
            && source_t.x + source_t.width >= pw
            && source_t.y + source_t.height >= ph;
        if covers_page {
            let (nw, nh) = (
                f64::from(content.natural_width),
                f64::from(content.natural_height),
            );
            let mut scale = (pw / nw).min(ph / nh);
            // Si el aspecto coincide con la página, «contain» seguiría
            // tapándola entera y el fondo no se vería: deja un margen.
            if nw * scale >= pw * 0.999 && nh * scale >= ph * 0.999 {
                scale *= 0.85;
            }
            let (w, h) = (nw * scale, nh * scale);
            commands.push(Box::new(SetTransform {
                layer: source_id,
                before: source_t,
                after: Transform::new((pw - w) / 2.0, (ph - h) / 2.0, w, h),
            }));
        }

        let transform = cover_transform(
            f64::from(content.natural_width),
            f64::from(content.natural_height),
            pw,
            ph,
        );
        let id = self.doc.allocate_layer_id();
        let mut layer = Layer::new(
            id,
            "Blurred background",
            transform,
            LayerContent::Image(content),
        );
        layer.effects.blur_radius = 50.0;
        commands.push(Box::new(InsertLayer { index: 0, layer }));

        if let Err(e) = self.history.apply(
            &mut self.doc,
            Box::new(canvas_core::Composite::new("Blurred background", commands)),
        ) {
            tracing::error!("añadir fondo falló: {e}");
            return;
        }
        self.images.insert(id, pixels);
        self.background_layer = Some(id);
    }

    /// Recoloca la capa de fondo para que cubra la página actual. Devuelve el
    /// comando (ya aplicado al documento) para integrarlo en un `Composite`.
    fn resync_background_cover(&mut self) -> Option<Box<dyn canvas_core::Command>> {
        let id = self.background_layer.filter(|_| self.background_active())?;
        let (pw, ph) = self.doc.page().map(|p| (p.width, p.height)).ok()?;
        let layer = self.doc.layer(id).ok()?;
        let LayerContent::Image(img) = &layer.content else {
            return None;
        };
        let before = layer.transform;
        let after = cover_transform(
            f64::from(img.natural_width),
            f64::from(img.natural_height),
            pw,
            ph,
        );
        if after == before {
            return None;
        }
        self.doc.layer_mut(id).ok()?.transform = after;
        Some(Box::new(SetTransform {
            layer: id,
            before,
            after,
        }))
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
            self.redo();
        } else if undo {
            self.undo();
        }
    }

    /// Deshace el último comando (menú Edit o Ctrl+Z).
    pub fn undo(&mut self) {
        if let Err(e) = self.history.undo(&mut self.doc) {
            tracing::error!("deshacer falló: {e}");
        }
    }

    /// Rehace el último comando deshecho (menú Edit o Ctrl+Y).
    pub fn redo(&mut self) {
        if let Err(e) = self.history.redo(&mut self.doc) {
            tracing::error!("rehacer falló: {e}");
        }
    }

    pub fn file_name(&self) -> String {
        self.doc
            .source_path
            .as_deref()
            .and_then(|p| p.file_name())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".to_owned())
    }

    pub fn is_dirty(&self) -> bool {
        self.history.is_dirty()
    }
}

/// Panel derecho: propiedades de la capa seleccionada.
pub fn properties_ui(state: &mut EditorState, ui: &mut egui::Ui) {
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

    if let Some(sel) = state.selected {
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
    });
    ui.checkbox(&mut state.sidecar_enabled, "Editable sidecar (.canvas)")
        .on_hover_text(
            "Writes a .canvas file next to the image so the layers stay \
             editable when you reopen it. Turn it off if you don't want \
             extra files in your folders.",
        );
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
    ui.weak("Wheel: zoom · Space/middle button: pan · Ctrl+0: fit");
    ui.weak("Ctrl+S: save · Ctrl+Shift+S: save as");
    ui.add_space(4.0);
    if ui.small_button("⚙ Settings").clicked() {
        state.settings_clicked = true;
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
        egui::ComboBox::from_id_salt("page_presets")
            .selected_text("Presets")
            .width(72.0)
            .show_ui(ui, |ui| {
                let mut preset = |ui: &mut egui::Ui, label: String, pw: f64, ph: f64| {
                    if ui.selectable_label(false, label).clicked() {
                        w = pw;
                        h = ph;
                        changed = true;
                        commit = true;
                    }
                };
                preset(ui, "1920 × 1080".into(), 1920.0, 1080.0);
                preset(ui, "1080 × 1920".into(), 1080.0, 1920.0);
                preset(ui, "1080 × 1080".into(), 1080.0, 1080.0);
                if let Some((iw, ih)) = image_size {
                    preset(ui, format!("Image ({} × {})", iw as i64, ih as i64), iw, ih);
                }
            });
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
                state
                    .history
                    .push_applied(Box::new(canvas_core::Composite::new(
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
                    state.history.push_applied(Box::new(canvas_core::SetBlur {
                        layer: id,
                        before,
                        after,
                    }));
                }
            }
        }
        if current_blur > 0.0 && ui.button("Remove").clicked() {
            if let Err(e) = state.history.apply(
                &mut state.doc,
                Box::new(canvas_core::SetBlur {
                    layer: target,
                    before: current_blur,
                    after: 0.0,
                }),
            ) {
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
        if let Err(e) = state.history.apply(
            &mut state.doc,
            Box::new(canvas_core::SetEffects {
                layer: sel,
                before,
                after: neutral,
            }),
        ) {
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
                state
                    .history
                    .push_applied(Box::new(canvas_core::SetEffects {
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
        if let Err(e) = state.history.apply(
            &mut state.doc,
            Box::new(canvas_core::SetShadow {
                layer: sel,
                before: current,
                after,
            }),
        ) {
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
                state.history.push_applied(Box::new(canvas_core::SetShadow {
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
    let original = layer.transform;
    let natural = match &layer.content {
        LayerContent::Image(img) => (f64::from(img.natural_width), f64::from(img.natural_height)),
        LayerContent::Svg(svg) => (f64::from(svg.natural_width), f64::from(svg.natural_height)),
        LayerContent::Text(_) | LayerContent::Shape(_) => (0.0, 0.0),
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
            if let Err(e) = state.history.apply(
                &mut state.doc,
                Box::new(canvas_core::Composite::new(
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
                )),
            ) {
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
    if commit || force_commit {
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

    // Zoom pedido desde el menú, anclado al centro del lienzo.
    if let Some(factor) = state.pending_zoom_factor.take() {
        state.viewport.zoom_at(rect.size() / 2.0, factor);
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

    // Sincroniza los efectos GPU de cada capa antes de montar la escena.
    if let Ok(page) = state.doc.page() {
        let fx_targets: Vec<(LayerId, canvas_core::Effects)> =
            page.layers.iter().map(|l| (l.id, l.effects)).collect();
        for (id, effects) in fx_targets {
            if let Some(source) = state.images.get(&id) {
                renderer.sync_layer_effects(&rs.device, &rs.queue, id, source, &effects);
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

    if state.show_grid {
        draw_grid(state, ui, rect, page_dims);
    }
    draw_selection_overlay(state, ui, rect);
    if state.show_rulers {
        draw_rulers(state, ui, rect);
    }
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

/// Esquinas de la capa (rotadas) en pantalla: [sup-izq, sup-der, inf-izq, inf-der].
fn layer_corners_screen(vp: &Viewport, rect: egui::Rect, t: &Transform) -> [egui::Pos2; 4] {
    t.corners().map(|(x, y)| page_to_screen(vp, rect, x, y))
}

/// Posición en pantalla del manejador de rotación (por encima del centro del
/// borde superior, en la dirección local de la capa).
fn rotation_handle_screen(vp: &Viewport, rect: egui::Rect, t: &Transform) -> egui::Pos2 {
    const OFFSET_SCREEN: f64 = 26.0;
    let theta = t.rotation.to_radians();
    let (sin, cos) = theta.sin_cos();
    let (cx, cy) = t.center();
    // Centro del borde superior + prolongación hacia fuera (en px de página).
    let reach = t.height / 2.0 + OFFSET_SCREEN / vp.zoom;
    let px = cx + reach * sin;
    let py = cy - reach * cos;
    page_to_screen(vp, rect, px, py)
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
            if let Some(sel) = state.selected {
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
                if hit != state.selected {
                    state.crop_mode = false;
                }
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
                            let others: Vec<Transform> = page
                                .layers
                                .iter()
                                .filter(|l| l.id != layer && l.visible)
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
                        state.history.push_applied(Box::new(SetTransform {
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
                        state
                            .history
                            .push_applied(Box::new(canvas_core::Composite::new(
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
            if hit != state.selected {
                state.crop_mode = false;
            }
            state.selected = hit;
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
                state
                    .history
                    .push_applied(Box::new(canvas_core::SetContent {
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
fn draw_selection_overlay(state: &EditorState, ui: &egui::Ui, rect: egui::Rect) {
    let painter = ui.painter_at(rect);

    // Guías magnéticas activas (líneas que cruzan todo el lienzo).
    let guide_stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(255, 64, 129));
    for &gx in &state.snap_guides.0 {
        let x = page_to_screen(&state.viewport, rect, gx, 0.0).x;
        painter.line_segment(
            [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
            guide_stroke,
        );
    }
    for &gy in &state.snap_guides.1 {
        let y = page_to_screen(&state.viewport, rect, 0.0, gy).y;
        painter.line_segment(
            [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
            guide_stroke,
        );
    }

    let Some(sel) = state.selected else { return };
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
    let [tl, tr, bl, br] = layer_corners_screen(&state.viewport, rect, t);
    painter.add(egui::Shape::closed_line(
        vec![tl, tr, br, bl],
        egui::Stroke::new(1.5, accent),
    ));

    // Manejador de rotación: línea + círculo (no en modo recorte).
    if !state.crop_mode {
        let top_center = egui::pos2((tl.x + tr.x) / 2.0, (tl.y + tr.y) / 2.0);
        let handle = rotation_handle_screen(&state.viewport, rect, t);
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
fn draw_grid(state: &EditorState, ui: &egui::Ui, rect: egui::Rect, page: (f64, f64)) {
    const STEPS: [f64; 10] = [1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0, 1000.0];
    let zoom = state.viewport.zoom;
    let Some(step) = STEPS.iter().copied().find(|s| s * zoom >= 24.0) else {
        return;
    };
    let painter = ui.painter_at(rect);
    let stroke = egui::Stroke::new(1.0, ui.visuals().text_color().gamma_multiply(0.15));
    let (pw, ph) = page;

    let mut x = 0.0;
    while x <= pw {
        let a = page_to_screen(&state.viewport, rect, x, 0.0);
        let b = page_to_screen(&state.viewport, rect, x, ph);
        painter.line_segment([a, b], stroke);
        x += step;
    }
    let mut y = 0.0;
    while y <= ph {
        let a = page_to_screen(&state.viewport, rect, 0.0, y);
        let b = page_to_screen(&state.viewport, rect, pw, y);
        painter.line_segment([a, b], stroke);
        y += step;
    }
}

/// Reglas superior e izquierda con marcas y números en píxeles de página.
fn draw_rulers(state: &EditorState, ui: &egui::Ui, rect: egui::Rect) {
    const THICKNESS: f32 = 18.0;
    const STEPS: [f64; 10] = [1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0, 1000.0];
    let zoom = state.viewport.zoom;
    let Some(step) = STEPS.iter().copied().find(|s| s * zoom >= 56.0) else {
        return;
    };
    let painter = ui.painter_at(rect);
    let bg = ui.visuals().extreme_bg_color.gamma_multiply(0.9);
    let fg = ui.visuals().text_color().gamma_multiply(0.75);
    let tick_stroke = egui::Stroke::new(1.0, fg);
    let font = egui::FontId::proportional(9.5);

    let top = egui::Rect::from_min_max(rect.min, egui::pos2(rect.max.x, rect.min.y + THICKNESS));
    let left = egui::Rect::from_min_max(rect.min, egui::pos2(rect.min.x + THICKNESS, rect.max.y));
    painter.rect_filled(top, 0.0, bg);
    painter.rect_filled(left, 0.0, bg);

    // Rango de página visible en el lienzo.
    let (x0, y0) = screen_to_page(&state.viewport, rect, rect.min);
    let (x1, y1) = screen_to_page(&state.viewport, rect, rect.max);

    let mut x = (x0 / step).floor() * step;
    while x <= x1 {
        let sx = page_to_screen(&state.viewport, rect, x, 0.0).x;
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
        let sy = page_to_screen(&state.viewport, rect, 0.0, y).y;
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
