//! Estado del editor: el documento activo, sus imágenes, y el
//! deshacer/rehacer local y global — el modelo que `canvas_ui` y el panel de
//! propiedades leen y mutan, sin la parte de UI en sí.

use std::path::PathBuf;

use canvas_core::{
    contain_transform, cover_transform, Command, CoreError, Document, History, ImageContent,
    InsertLayer, Layer, LayerContent, LayerId, RemoveLayer, Selection, SetTransform, Transform,
};
use canvas_io::LoadedImage;
use canvas_render::{image_data_from_rgba, ImageMap};
use eframe::egui;

use super::{Gesture, Viewport};

pub struct EditorState {
    pub doc: Document,
    pub history: History,
    pub images: ImageMap,
    pub selection: Selection,
    pub viewport: Viewport,
    /// Proporción bloqueada al redimensionar (por defecto sí; `Shift` la libera).
    pub aspect_lock: bool,
    pub(super) gesture: Gesture,
    /// Edición en curso desde el panel (campos numéricos): capa y transform
    /// original, para consolidar en un solo comando al terminar.
    pub(super) panel_edit: Option<(LayerId, Transform)>,
    /// Edición en curso del tamaño de página (campos An/Al de la sección
    /// Página): dimensiones originales, para consolidar al terminar.
    pub(super) page_edit: Option<(f64, f64)>,
    /// Ventanita flotante "Size" del menú contextual del lienzo: `Some((w,h))`
    /// mientras está abierta, con los valores en edición (no confirmados
    /// hasta pulsar Apply — no participa en `is_idle()`, mismo criterio que
    /// `Deck::rename_edit`, que tampoco bloquea saltar de lienzo).
    pub(super) size_popup: Option<(f64, f64)>,
    /// Ventana para pegar una URL y reemplazar la imagen seleccionada.
    pub(super) replace_url_popup: Option<(LayerId, String)>,
    /// Capa de «fondo desenfocado» activa, si la hay. `pub(crate)` porque el
    /// panel de capas (otro módulo) necesita fijarla como fila no arrastrable
    /// y excluirla de "Agrupar".
    pub(crate) background_layer: Option<LayerId>,
    /// Ajuste de desenfoque en curso (slider): capa y radio original.
    pub(super) blur_edit: Option<(LayerId, f32)>,
    /// Ajuste de color en curso (sliders): capa y efectos originales, para
    /// consolidar los 6 sliders en un solo paso de deshacer.
    pub(super) color_edit: Option<(LayerId, canvas_core::Effects)>,
    /// Edición de contenido en curso (texto/forma): capa y contenido original.
    pub(super) content_edit: Option<(LayerId, LayerContent)>,
    /// Ajuste de sombra en curso: capa y sombra original.
    pub(super) shadow_edit: Option<(LayerId, Option<canvas_core::Shadow>)>,
    /// Hay un guardado en curso en un hilo de trabajo.
    pub saving: bool,
    /// Hay una exportación en curso en un hilo de trabajo.
    pub exporting: bool,
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
    /// Sin efecto si `is_design`: un diseño siempre guarda sus capas.
    pub sidecar_enabled: bool,
    /// El documento ES un `.canvas` autónomo: `Ctrl+S` lo reescribe sin
    /// rasterizar. «Export…» sigue produciendo PNG/JPEG/SVG/PDF.
    pub is_design: bool,
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
    pub(super) snap_guides: (Vec<f64>, Vec<f64>),
    /// Renombrado en curso en el panel de capas: capa, texto editable y
    /// nombre original (para poder cancelar sin comando con Escape).
    pub rename_edit: Option<(LayerId, String, String)>,
    /// Renombrado en curso del ARCHIVO abierto (lápiz junto al nombre en el
    /// panel de propiedades): solo el nombre base editable, sin extensión.
    pub file_rename_edit: Option<String>,
    /// Nuevo nombre de archivo confirmado, pendiente de aplicar en disco.
    pub file_rename_requested: Option<String>,
    /// El usuario confirmó (diálogo nativo ya mostrado) borrar el archivo
    /// abierto.
    pub delete_requested: bool,
    /// Nació en blanco (`new_blank`/`new_blank_image`) y todavía no se ha
    /// guardado ni una vez: el archivo en disco (si lo hay — la reserva de
    /// nombre de una ranura provisional deja uno de 0 bytes) no tiene
    /// píxeles del usuario que un primer `Ctrl+S` pudiera destruir, así que
    /// ese primer guardado se salta el modal de aviso de sobrescritura. Se
    /// apaga al recibir `Saved`.
    pub born_blank: bool,
    /// Nació en blanco (`new_blank`/`new_blank_image`) y su creación
    /// TODAVÍA no se ha registrado en el deshacer global. Se consume (pasa a
    /// `false`) en la primera llamada real a `push_undo_step`/
    /// `apply_undo_step` de este lienzo, que antepone un
    /// `GlobalStep::Create` — así el paso "esto se creó" aparece en el
    /// momento exacto de la primera edición, sin importar cuántas ranuras
    /// "+" fantasma haya de por medio (relleno automático de la baraja que
    /// el usuario nunca llega a tocar no genera ningún paso). Viaja con la
    /// ranura vía `SlotDoc`/`take_slot`/`put_slot`, igual que `born_blank`.
    pub(crate) pending_creation: bool,
    /// Petición de saltar a otro lienzo de la baraja (`PageUp`/`PageDown`/
    /// `Home`/`End`); la app resuelve el destino contra `Deck` y pregunta si
    /// hace falta por cambios sin guardar, igual que «Back to gallery».
    pub deck_nav: Option<DeckNav>,
    /// La pulsación primaria en curso empezó sobre un lienzo que NO era el
    /// activo: pertenece a la baraja (activar ese lienzo), no a las capas
    /// del documento activo. Vive aquí y no en una local de `canvas_ui`
    /// porque tiene que sobrevivir a TODOS los frames del gesto — pulsar,
    /// arrastrar y soltar — no solo al frame en que se detecta, y una local
    /// no sobrevive entre frames. Deliberadamente FUERA de `is_idle()`: la
    /// bandera existe justo para que `apply_jump` SÍ pueda saltar mientras
    /// el botón sigue pulsado.
    pub(crate) press_on_other_slot: bool,
    /// Id de ranura de la baraja a la que pertenece el documento activo
    /// ahora mismo. Campo "de sesión" (como `viewport`): NO viaja en
    /// `SlotDoc`, `main.rs` lo refresca cada frame contra `deck.active`.
    /// Sirve para etiquetar cada paso de deshacer en `global_undo`/
    /// `global_redo` con su diseño de origen.
    pub(crate) active_slot_id: u64,
    /// Pila global de deshacer: un paso por entrada, en el mismo orden
    /// cronológico en que ocurrieron SIN IMPORTAR de qué diseño eran — a
    /// diferencia de `history` (local, solo el diseño activo). El comando en
    /// sí (para `Edit`) sigue viviendo en el `History` local de esa ranura;
    /// esto solo registra el ORDEN cruzado para que `Ctrl+Z` sepa cuál
    /// deshacer a continuación y en qué diseño.
    global_undo: Vec<GlobalStep>,
    /// Simétrico de `global_undo` para `Ctrl+Y`/rehacer.
    global_redo: Vec<GlobalStep>,
    /// Paso global pedido cuyo diseño destino NO es el activo: `undo()`/
    /// `redo()` dejan esto en vez de tocar nada, y `main.rs` pide el salto
    /// de baraja; en cuanto ese diseño pasa a ser el activo llama a
    /// `finish_pending_global_undo`/`_redo`, que hace el paso de verdad.
    pub pending_global_undo: Option<GlobalStep>,
    /// Simétrico de `pending_global_undo` para rehacer.
    pub pending_global_redo: Option<GlobalStep>,
    /// Deshacer un `GlobalStep::Delete` deja aquí qué restaurar de la
    /// papelera de reciclaje — `main.rs` lo recoge cada frame, lanza el
    /// hilo de restauración y lo limpia. No pertenece a ninguna ranura (el
    /// archivo ya no existía en la baraja), así que no hace falta esperar
    /// ningún salto, a diferencia de `pending_global_undo`.
    pub pending_restore: Option<DeleteRecord>,
    /// El `delete_requested` actual es consecuencia de deshacer un
    /// `GlobalStep::Create` (ver `finish_pending_global_undo`), no un clic
    /// directo del usuario en «Delete»: `main.rs` lo lee para NO apilar un
    /// `GlobalStep::Delete` por ese borrado — si lo hiciera, un `Ctrl+Z`
    /// más adelante podría "deshacer el deshacer" y restaurar un lienzo que
    /// el propio usuario decidió descartar.
    pub(crate) pending_delete_from_undo: bool,
}

/// Un paso del deshacer/rehacer GLOBAL (`EditorState::global_undo`/
/// `global_redo`), con el id de ranura al que pertenece (o, para `Delete`,
/// la ruta borrada). `Clone` en vez de `Copy`: `Delete` carga rutas.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) enum GlobalStep {
    /// Un comando normal, ya apilado en el `History` local de esa ranura —
    /// deshacerlo/rehacerlo es `History::undo`/`redo` sobre ese diseño.
    Edit(u64),
    /// La ranura se creó en este punto de la sesión (botón "+", zona "+" del
    /// lienzo, duplicar una provisional). Deshacerlo BORRA la ranura entera
    /// (vía el mismo camino que el botón «Delete»: papelera de reciclaje si
    /// ya tenía archivo, descarte en memoria si seguía siendo provisional).
    /// Sin simétrico en `global_redo`: crear no se "rehace" borrando otra vez.
    Create(u64),
    /// Se borró un archivo real (botón «Delete»/cabecera de un lienzo de
    /// fondo — nunca el borrado que ya ocurre al deshacer un `Create`, ver
    /// `EditorState::pending_delete_from_undo`). Deshacerlo restaura el
    /// archivo desde la papelera de reciclaje — no pertenece a ninguna
    /// ranura de la baraja (esa ya no existe), así que no hace falta saltar
    /// a ningún sitio primero. Sin simétrico en `global_redo`: tampoco se
    /// "rehace" volviendo a borrar.
    Delete(DeleteRecord),
}

/// Lo necesario para restaurar desde la papelera de reciclaje un archivo
/// borrado por el usuario — ver `GlobalStep::Delete`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct DeleteRecord {
    pub path: PathBuf,
    /// El sidecar `.canvas` que se borró junto al archivo, si tenía uno.
    pub sidecar: Option<PathBuf>,
}

impl GlobalStep {
    /// Solo válido para `Edit`/`Create` (los únicos que participan en el
    /// salto de baraja `pending_global_undo`/`_redo` resuelve en
    /// `main.rs`): `Delete` se resuelve de inmediato en `undo()`, sin pasar
    /// nunca por ese campo.
    pub(crate) fn slot_id(&self) -> u64 {
        match self {
            GlobalStep::Edit(id) | GlobalStep::Create(id) => *id,
            GlobalStep::Delete(_) => {
                unreachable!("Delete nunca se deja en pending_global_undo/_redo")
            }
        }
    }
}

/// Dirección de salto entre lienzos de la baraja, pedida por teclado.
#[derive(Clone, Copy)]
pub enum DeckNav {
    Next,
    Prev,
    First,
    Last,
}

impl EditorState {
    /// Constructor común: los tres puntos de entrada (imagen nueva, proyecto
    /// en blanco, restaurado desde sidecar) solo difieren en el documento, sus
    /// píxeles, la selección inicial y el fondo desenfocado.
    fn base(
        doc: Document,
        images: ImageMap,
        selection: Selection,
        background_layer: Option<LayerId>,
    ) -> Self {
        Self {
            doc,
            history: History::default(),
            images,
            selection,
            viewport: Viewport::default(),
            aspect_lock: true,
            gesture: Gesture::None,
            panel_edit: None,
            page_edit: None,
            size_popup: None,
            replace_url_popup: None,
            background_layer,
            blur_edit: None,
            color_edit: None,
            content_edit: None,
            shadow_edit: None,
            saving: false,
            exporting: false,
            save_error: None,
            from_gallery: None,
            return_requested: false,
            save_clicked: false,
            save_as_clicked: false,
            settings_clicked: false,
            sidecar_enabled: true,
            is_design: false,
            source_metadata: None,
            external_change: false,
            reload_requested: false,
            pending_zoom_factor: None,
            show_grid: false,
            show_rulers: false,
            crop_mode: false,
            snap_guides: (Vec::new(), Vec::new()),
            rename_edit: None,
            file_rename_edit: None,
            file_rename_requested: None,
            delete_requested: false,
            born_blank: false,
            pending_creation: false,
            deck_nav: None,
            press_on_other_slot: false,
            active_slot_id: 0,
            global_undo: Vec::new(),
            global_redo: Vec::new(),
            pending_global_undo: None,
            pending_global_redo: None,
            pending_restore: None,
            pending_delete_from_undo: false,
        }
    }

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
        Ok(Self::base(doc, images, Selection::single(id), None))
    }

    /// Proyecto nuevo en blanco, como diseño autónomo `.canvas`: el primer
    /// guardado no rasteriza nada, sigue siendo un `.canvas` de pleno derecho.
    pub fn new_blank(width: f64, height: f64) -> Self {
        let mut doc = Document::new(width, height);
        if let Ok(page) = doc.page_mut() {
            page.background = Some([255, 255, 255, 255]);
        }
        let mut state = Self::base(doc, ImageMap::new(), Selection::default(), None);
        state.is_design = true;
        state.born_blank = true;
        state.pending_creation = true;
        state
    }

    /// Proyecto nuevo en blanco respaldado por un raster real (PNG/JPEG/
    /// WebP): el primer guardado hornea la página y escribe el archivo más
    /// su sidecar, por el mismo camino (`start_save`) que cualquier imagen
    /// editada. `sidecar_enabled` se fuerza a `true` — sin él, ese primer
    /// guardado escribiría un raster en blanco y perdería silenciosamente
    /// las capas que el usuario acabe de dibujar, aunque el ajuste global de
    /// sidecar estuviera desactivado.
    pub fn new_blank_image(width: f64, height: f64) -> Self {
        let mut doc = Document::new(width, height);
        if let Ok(page) = doc.page_mut() {
            page.background = Some([255, 255, 255, 255]);
        }
        let mut state = Self::base(doc, ImageMap::new(), Selection::default(), None);
        state.sidecar_enabled = true;
        state.born_blank = true;
        state.pending_creation = true;
        state
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
        let selection = selected.map_or_else(Selection::default, Selection::single);
        Self::base(doc, images, selection, background_layer)
    }

    /// Diseño autónomo restaurado desde su propio `.canvas`: como
    /// `from_restored`, salvo que aquí `path` es el diseño mismo, no la
    /// imagen que acompaña. Un `.canvas` duplicado puede traer un
    /// `source_path` incrustado que sigue apuntando al original — inocuo,
    /// porque `from_restored` lo sobrescribe con la ruta realmente abierta.
    pub fn from_design(path: PathBuf, restored: canvas_io::RestoredDocument) -> Self {
        let mut state = Self::from_restored(path, restored);
        state.is_design = true;
        state
    }

    /// Datos para que el hilo de guardado escriba el `.canvas`: documento
    /// clonado y píxeles RGBA de cada capa. `preview` queda en `None`: este
    /// método no tiene acceso a la GPU, así que quien la necesite (el
    /// horneado de guardado) la rellena después.
    pub fn sidecar_payload(&self) -> canvas_io::CanvasPayload {
        let images = self
            .images
            .iter()
            .map(|(id, data)| (id.raw(), data.data.data().to_vec(), data.width, data.height))
            .collect();
        canvas_io::CanvasPayload {
            document: self.doc.clone(),
            images,
            background_layer: self.background_layer.map(|id| id.raw()),
            preview: None,
        }
    }

    /// Añade una imagen como capa nueva (deshacible) y la selecciona.
    /// `source` es `None` cuando la imagen viene del portapapeles del
    /// sistema (no tiene un archivo de origen en disco).
    ///
    /// Sobre un lienzo VACÍO (sin ninguna capa, el caso de un diseño nuevo),
    /// la imagen se AMPLÍA para tocar el borde que antes llegue («contain»,
    /// estilo CapCut/Canva) en vez de solo encajarla si es mayor que la
    /// página; si con eso no cubre la página entera, se añade también un
    /// fondo desenfocado automático — misma receta que el checkbox «Blurred
    /// background» (`set_blurred_background`), en el mismo paso de deshacer.
    /// Sobre un lienzo con contenido, el comportamiento es el de siempre:
    /// centrada, sin ampliar y sin fondo.
    pub fn add_image_layer(
        &mut self,
        name: impl Into<String>,
        source: Option<PathBuf>,
        img: LoadedImage,
    ) {
        let Ok(page) = self.doc.page() else { return };
        let (pw, ph) = (page.width, page.height);
        let empty = page.layers.is_empty();
        let index = page.layers.len();

        let (nw, nh) = (f64::from(img.width), f64::from(img.height));
        let transform = if empty {
            contain_transform(nw, nh, pw, ph)
        } else {
            let scale = (pw / nw).min(ph / nh).min(1.0);
            let (w, h) = (nw * scale, nh * scale);
            Transform::new((pw - w) / 2.0, (ph - h) / 2.0, w, h)
        };
        // Con el mismo aspecto que la página, "contain" ya la cubre entera:
        // ese margen es solo tolerancia de redondeo, no hueco real.
        let needs_background =
            empty && !(transform.width >= pw * 0.999 && transform.height >= ph * 0.999);

        let content = ImageContent {
            source_path: source,
            natural_width: img.width,
            natural_height: img.height,
            crop: None,
        };
        let pixels = image_data_from_rgba(img.rgba, img.width, img.height);
        let id = self.doc.allocate_layer_id();
        let layer = Layer::new(id, name, transform, LayerContent::Image(content.clone()));

        let mut commands: Vec<Box<dyn canvas_core::Command>> = Vec::new();
        let mut bg_id = None;
        if needs_background {
            let new_bg_id = self.doc.allocate_layer_id();
            let mut bg = Layer::new(
                new_bg_id,
                "Blurred background",
                cover_transform(nw, nh, pw, ph),
                LayerContent::Image(content),
            );
            bg.effects.blur_radius = 50.0;
            commands.push(Box::new(InsertLayer {
                index: 0,
                layer: bg,
            }));
            bg_id = Some(new_bg_id);
        }
        commands.push(Box::new(InsertLayer {
            index: index + usize::from(bg_id.is_some()),
            layer,
        }));

        if let Err(e) =
            self.apply_undo_step(Box::new(canvas_core::Composite::new("Add image", commands)))
        {
            tracing::error!("añadir capa falló: {e}");
            return;
        }
        if let Some(bg_id) = bg_id {
            self.images.insert(bg_id, pixels.clone());
            self.background_layer = Some(bg_id);
        }
        self.images.insert(id, pixels);
        self.selection = Selection::single(id);
    }

    fn replace_image_content(
        &mut self,
        target: LayerId,
        content: ImageContent,
        pixels: vello::peniko::ImageData,
    ) -> Result<(), String> {
        let (index, old_layer) = {
            let page = self.doc.page().map_err(|e| e.to_string())?;
            let index = page
                .index_of(target)
                .ok_or_else(|| "Selected image was not found".to_owned())?;
            let layer = page.layers[index].clone();
            if !matches!(layer.content, LayerContent::Image(_)) {
                return Err("Selected layer is not an image".to_owned());
            }
            (index, layer)
        };

        let new_id = self.doc.allocate_layer_id();
        let mut new_layer = old_layer;
        new_layer.id = new_id;
        new_layer.content = LayerContent::Image(content);

        self.apply_undo_step(Box::new(canvas_core::Composite::new(
            "Replace image",
            vec![
                Box::new(RemoveLayer::new(target)),
                Box::new(InsertLayer {
                    index,
                    layer: new_layer,
                }),
            ],
        )))
        .map_err(|e| e.to_string())?;

        self.images.insert(new_id, pixels);
        if self.background_layer == Some(target) {
            self.background_layer = Some(new_id);
        }
        self.selection = Selection::single(new_id);
        self.crop_mode = false;
        Ok(())
    }

    pub fn replace_image_layer(
        &mut self,
        target: LayerId,
        source: Option<PathBuf>,
        img: LoadedImage,
    ) -> Result<(), String> {
        let content = ImageContent {
            source_path: source,
            natural_width: img.width,
            natural_height: img.height,
            crop: None,
        };
        let pixels = image_data_from_rgba(img.rgba, img.width, img.height);
        self.replace_image_content(target, content, pixels)
    }

    pub(super) fn replace_image_from_layer(
        &mut self,
        target: LayerId,
        source: LayerId,
    ) -> Result<(), String> {
        let (content, pixels) = {
            let layer = self.doc.layer(source).map_err(|e| e.to_string())?;
            let LayerContent::Image(content) = &layer.content else {
                return Err("Source layer is not an image".to_owned());
            };
            let pixels = self
                .images
                .get(&source)
                .cloned()
                .ok_or_else(|| "Source image pixels are not loaded".to_owned())?;
            (content.clone(), pixels)
        };
        self.replace_image_content(target, content, pixels)
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
        if let Err(e) = self.apply_undo_step(Box::new(InsertLayer { index, layer })) {
            tracing::error!("insertar capa falló: {e}");
            return;
        }
        self.selection = Selection::single(id);
        self.crop_mode = false;
    }

    /// ¿Está activa (y viva, tras posibles deshacer) la capa de fondo?
    pub(super) fn background_active(&self) -> bool {
        self.background_layer
            .is_some_and(|id| self.doc.layer(id).is_ok())
    }

    /// Capa de imagen que serviría de fuente para el fondo desenfocado.
    pub(super) fn background_source(&self) -> Option<LayerId> {
        let is_candidate = |l: &Layer| {
            matches!(l.content, LayerContent::Image(_)) && Some(l.id) != self.background_layer
        };
        // La seleccionada si vale; si no, la capa de imagen más alta.
        if let Some(sel) = self.selection.primary() {
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
    pub(super) fn set_blurred_background(&mut self, on: bool) {
        if !on {
            if let Some(id) = self.background_layer.take() {
                if let Err(e) = self.apply_undo_step(Box::new(RemoveLayer::new(id))) {
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

        if let Err(e) = self.apply_undo_step(Box::new(canvas_core::Composite::new(
            "Blurred background",
            commands,
        ))) {
            tracing::error!("añadir fondo falló: {e}");
            return;
        }
        self.images.insert(id, pixels);
        self.background_layer = Some(id);
    }

    /// Recoloca la capa de fondo para que cubra la página actual. Devuelve el
    /// comando (ya aplicado al documento) para integrarlo en un `Composite`.
    pub(super) fn resync_background_cover(&mut self) -> Option<Box<dyn canvas_core::Command>> {
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
    ///
    /// `paste_requested` es la señal de Ctrl+V/Shift+Insert capturada por
    /// `paste_hook` a nivel de mensajes de Windows: en Win32, egui-winit se
    /// traga esa combinación sin emitir `Event::Paste` cuando el
    /// portapapeles solo tiene un bitmap (ver `paste_hook.rs`), así que en
    /// esa plataforma es la única señal fiable.
    pub fn handle_shortcuts(
        &mut self,
        ctx: &egui::Context,
        paste_requested: bool,
        deck_renaming: bool,
    ) {
        use egui::{Event, Key, KeyboardShortcut, Modifiers};
        // Deshacer/rehacer se evalúan primero y con su propia guarda: un
        // `TextEdit` con foco propio (renombrar una capa, editar su texto, o
        // renombrar una ranura de la baraja) debe quedarse con Ctrl+Z para su
        // propio undo, no el del documento. `ctx.text_edit_focused()` es
        // DEMASIADO ancho para eso — en egui 0.35 también es `true` mientras
        // se edita un `DragValue` del panel de propiedades por teclado (usa
        // un `TextEdit` interno con el mismo id), lo que dejaba Ctrl+Z muerto
        // tras tocar X/Y/W/H/Scale hasta hacer clic en otro sitio. Por eso
        // aquí se miran las banderas propias del editor en vez de esa guarda
        // global.
        let editing_own_text = self.rename_edit.is_some()
            || self.file_rename_edit.is_some()
            || self.content_edit.is_some()
            || deck_renaming;
        if !editing_own_text {
            // El orden importa: Ctrl+Shift+Z debe consumirse antes que Ctrl+Z.
            let redo = ctx.input_mut(|i| {
                i.consume_shortcut(&KeyboardShortcut::new(
                    Modifiers::COMMAND | Modifiers::SHIFT,
                    Key::Z,
                )) || i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::Y))
            });
            let undo = ctx.input_mut(|i| {
                i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::Z))
            });
            if redo {
                self.redo();
            } else if undo {
                self.undo();
            }
        }

        // El resto de atajos (portapapeles, Supr, navegación de baraja…) sí
        // le siguen cediendo el paso a cualquier `TextEdit` con foco — ese es
        // el caso general que `text_edit_focused()` describe bien.
        if ctx.text_edit_focused() {
            return;
        }

        // Ctrl+Shift+G (desagrupar) antes que Ctrl+G (agrupar): mismo patrón
        // que redo/undo arriba, lo más específico primero.
        let ungroup = ctx.input_mut(|i| {
            i.consume_shortcut(&KeyboardShortcut::new(
                Modifiers::COMMAND | Modifiers::SHIFT,
                Key::G,
            ))
        });
        let group = ctx
            .input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::G)));
        if ungroup {
            crate::layers_panel::ungroup_selection(self);
        } else if group {
            crate::layers_panel::group_selection(self);
        }

        // Ctrl+X/C no llegan como pulsaciones de tecla normales: winit los
        // intercepta para la integración con el portapapeles del SO y egui
        // los entrega como `Event::Cut`/`Copy`, así que `consume_shortcut`
        // nunca los ve — hay que mirar los eventos crudos.
        let (want_cut, want_copy, event_paste) = ctx.input(|i| {
            let mut cut = false;
            let mut copy = false;
            let mut paste = false;
            for ev in &i.events {
                match ev {
                    Event::Cut => cut = true,
                    Event::Copy => copy = true,
                    Event::Paste(_) => paste = true,
                    _ => {}
                }
            }
            (cut, copy, paste)
        });
        if want_cut {
            crate::clipboard::cut(self);
        }
        if want_copy {
            crate::clipboard::copy(self);
        }
        // En Windows, `Event::Paste` no llega cuando el portapapeles solo
        // trae un bitmap (ver el doc del parámetro y `paste_hook.rs`): ahí
        // se usa la señal del hook en su lugar. Fuera de Windows
        // `paste_requested` siempre es `false` (el hook es un no-op) y
        // `Event::Paste` sigue siendo la única señal.
        let want_paste = if cfg!(windows) {
            paste_requested
        } else {
            event_paste
        };
        if want_paste && !crate::clipboard::paste(self) {
            self.save_error = Some(crate::clipboard::PASTE_EMPTY_MSG.to_owned());
        }
        if ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::D)))
        {
            crate::clipboard::duplicate(self);
        }
        if ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::A)))
        {
            crate::clipboard::select_all(self);
        }
        let delete = ctx.input_mut(|i| {
            i.consume_shortcut(&KeyboardShortcut::new(Modifiers::NONE, Key::Delete))
                || i.consume_shortcut(&KeyboardShortcut::new(Modifiers::NONE, Key::Backspace))
        });
        if delete {
            crate::clipboard::delete_selected(self);
        }

        // Navegación entre lienzos de la baraja. `Ctrl+PageUp/Down` es un
        // alias (memoria muscular de pestañas de navegador); las flechas se
        // dejan libres a propósito para el futuro «mover capa con teclado».
        if ctx.input_mut(|i| {
            i.consume_shortcut(&KeyboardShortcut::new(Modifiers::NONE, Key::PageDown))
                || i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::PageDown))
        }) {
            self.deck_nav = Some(DeckNav::Next);
        } else if ctx.input_mut(|i| {
            i.consume_shortcut(&KeyboardShortcut::new(Modifiers::NONE, Key::PageUp))
                || i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::PageUp))
        }) {
            self.deck_nav = Some(DeckNav::Prev);
        } else if ctx
            .input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::NONE, Key::Home)))
        {
            self.deck_nav = Some(DeckNav::First);
        } else if ctx
            .input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::NONE, Key::End)))
        {
            self.deck_nav = Some(DeckNav::Last);
        }
    }

    /// Aplica `cmd` al documento y lo apila como paso de deshacer del diseño
    /// activo — igual que `History::apply`, pero además registra el paso en
    /// la pila GLOBAL cruzada entre diseños (`global_undo`). Todo comando
    /// real (fuera del truco interno de `push_placeholder` en `deck.rs`,
    /// que no es una edición visible del usuario) debe pasar por aquí o por
    /// `push_undo_step`, nunca por `self.history` directamente — si no, ese
    /// paso quedaría invisible para el `Ctrl+Z` global.
    pub(crate) fn apply_undo_step(&mut self, cmd: Box<dyn Command>) -> Result<(), CoreError> {
        self.history.apply(&mut self.doc, cmd)?;
        self.record_edit_step();
        Ok(())
    }

    /// Apila un comando cuyo efecto YA está reflejado en el documento (fin
    /// de un gesto continuo) — ver `apply_undo_step`.
    pub(crate) fn push_undo_step(&mut self, cmd: Box<dyn Command>) {
        self.history.push_applied(cmd);
        self.record_edit_step();
    }

    /// Apila un borrado de archivo real como paso deshacible
    /// (`GlobalStep::Delete`) — llamado por `main.rs` tras un
    /// `spawn_document_delete` que el usuario pidió directamente (nunca el
    /// que ya ocurre como consecuencia de deshacer un `Create`, ver
    /// `pending_delete_from_undo`).
    pub(crate) fn record_delete(&mut self, record: DeleteRecord) {
        self.global_undo.push(GlobalStep::Delete(record));
        self.global_redo.clear();
    }

    /// Registra en la pila global el paso que `apply_undo_step`/
    /// `push_undo_step` ya reflejaron en el `History` local. Si este lienzo
    /// nació esta sesión y su creación aún no se había anotado
    /// (`pending_creation`), antepone un `GlobalStep::Create` — así "crear
    /// este lienzo" aparece como paso deshacible en el momento EXACTO de su
    /// primera edición real, sin importar cuántas ranuras "+" de relleno
    /// automático haya habido de por medio que el usuario nunca llegó a
    /// tocar (esas no generan ningún paso: nadie las editó).
    fn record_edit_step(&mut self) {
        if std::mem::take(&mut self.pending_creation) {
            self.global_undo
                .push(GlobalStep::Create(self.active_slot_id));
        }
        self.global_undo.push(GlobalStep::Edit(self.active_slot_id));
        self.global_redo.clear();
    }

    /// ¿Hay algo que deshacer/rehacer en TODA la sesión (no solo en el
    /// diseño activo)? Gobierna si el menú/atajo Undo-Redo están activos.
    pub fn can_undo(&self) -> bool {
        !self.global_undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.global_redo.is_empty()
    }

    /// Deshace la acción más reciente de TODA la sesión (menú Edit, clic
    /// derecho o Ctrl+Z) — no solo del diseño activo. Un `Edit` del diseño
    /// activo se deshace en el sitio; un `Create` SIEMPRE pasa por
    /// `pending_global_undo` (incluso si su ranura ya es la activa, para
    /// compartir un único camino con «otro diseño») como señal para que
    /// `main.rs` pida el salto de baraja y, en cuanto toque, llame a
    /// `finish_pending_global_undo`. Un `Delete` no pertenece a ninguna
    /// ranura (el archivo ya no existe): se resuelve de inmediato, sin
    /// esperar ningún salto.
    pub fn undo(&mut self) {
        let Some(step) = self.global_undo.last().cloned() else {
            tracing::info!("deshacer: nada que deshacer");
            return;
        };
        match step {
            GlobalStep::Edit(slot_id) if slot_id == self.active_slot_id => self.undo_local(),
            GlobalStep::Delete(record) => {
                self.global_undo.pop();
                self.pending_restore = Some(record);
            }
            _ => self.pending_global_undo = Some(step),
        }
    }

    /// Rehace el último paso deshecho de TODA la sesión (menú Edit, clic
    /// derecho o Ctrl+Y) — simétrico a `undo`. `GlobalStep::Create` nunca
    /// debería aparecer aquí (deshacer una creación no deja rastro en
    /// `global_redo`); el `match` de `finish_pending_global_redo` lo cubre
    /// solo por si acaso, no como camino esperado.
    pub fn redo(&mut self) {
        let Some(step) = self.global_redo.last().cloned() else {
            tracing::info!("rehacer: nada que rehacer");
            return;
        };
        match step {
            GlobalStep::Edit(slot_id) if slot_id == self.active_slot_id => self.redo_local(),
            GlobalStep::Delete(_) => {
                // No debería pasar: `undo()` nunca deja un `Delete` en
                // `global_redo` (no se "rehace" volver a borrar).
                tracing::warn!("rehacer global: entrada de borrado inesperada, se descarta");
                self.global_redo.pop();
            }
            _ => self.pending_global_redo = Some(step),
        }
    }

    /// Deshace de verdad en el documento activo (asume que ya es el diseño
    /// correcto) y mantiene las pilas globales en sincronía con la local.
    fn undo_local(&mut self) {
        match self.history.undo(&mut self.doc) {
            Ok(true) => {
                tracing::info!("deshacer OK");
                self.global_undo.pop();
                self.global_redo.push(GlobalStep::Edit(self.active_slot_id));
            }
            Ok(false) => tracing::info!("deshacer: nada que deshacer"),
            Err(e) => {
                tracing::error!("deshacer falló: {e}");
                self.save_error = Some(format!("Undo failed: {e}"));
                // Se descarta también la entrada global: insistir en un
                // comando cuyo revert falla dejaría el deshacer global
                // atascado para siempre en el mismo paso.
                self.global_undo.pop();
            }
        }
        self.forget_deleted_selection();
    }

    /// Simétrico de `undo_local` para rehacer.
    fn redo_local(&mut self) {
        match self.history.redo(&mut self.doc) {
            Ok(true) => {
                tracing::info!("rehacer OK");
                self.global_redo.pop();
                self.global_undo.push(GlobalStep::Edit(self.active_slot_id));
            }
            Ok(false) => tracing::info!("rehacer: nada que rehacer"),
            Err(e) => {
                tracing::error!("rehacer falló: {e}");
                self.save_error = Some(format!("Redo failed: {e}"));
                self.global_redo.pop();
            }
        }
        self.forget_deleted_selection();
    }

    /// `main.rs` llama a esto en cuanto el diseño pedido por
    /// `pending_global_undo` ya es el activo. Un `Edit` se deshace en el
    /// sitio; un `Create` no tiene "documento que revertir" — en vez de eso
    /// pide borrar la ranura entera por el mismo camino que el botón
    /// «Delete» (`delete_requested`, que `main.rs` ya sabe resolver contra
    /// una provisional o un archivo real).
    pub(crate) fn finish_pending_global_undo(&mut self) {
        let Some(step) = self.pending_global_undo.take() else {
            return;
        };
        match step {
            GlobalStep::Edit(_) => self.undo_local(),
            GlobalStep::Create(_) => {
                self.global_undo.pop();
                // Marca este borrado como "consecuencia de deshacer una
                // creación", no una decisión directa del usuario: evita que
                // genere su propio `GlobalStep::Delete` (si lo hiciera,
                // podrías "deshacer el deshacer" y restaurar un lienzo que
                // tú mismo acabas de decidir descartar).
                self.pending_delete_from_undo = true;
                self.delete_requested = true;
            }
            GlobalStep::Delete(_) => {
                unreachable!("undo() resuelve Delete de inmediato, nunca lo deja pendiente")
            }
        }
    }

    /// Simétrico de `finish_pending_global_undo` para rehacer. `Create`
    /// nunca debería llegar aquí (ver `redo`) — si pasara, se descarta con
    /// aviso en vez de intentar nada.
    pub(crate) fn finish_pending_global_redo(&mut self) {
        let Some(step) = self.pending_global_redo.take() else {
            return;
        };
        match step {
            GlobalStep::Edit(_) => self.redo_local(),
            GlobalStep::Create(_) => {
                tracing::warn!("rehacer global: entrada de creación inesperada, se descarta");
                self.global_redo.pop();
            }
            GlobalStep::Delete(_) => {
                unreachable!("Delete nunca se deja en pending_global_redo")
            }
        }
    }

    /// La ranura pedida por un deshacer/rehacer cruzado ya no existe
    /// (archivo borrado entre medias, por ejemplo): descarta esa entrada de
    /// la pila global correspondiente y avisa, sin encadenar automáticamente
    /// con la siguiente — un `Ctrl+Z` más la vuelve a pedir si hace falta.
    pub(crate) fn discard_pending_global_undo(&mut self) {
        if self.pending_global_undo.take().is_some() {
            tracing::warn!("deshacer global: la ranura pedida ya no existe, se descarta ese paso");
            self.global_undo.pop();
        }
    }
    pub(crate) fn discard_pending_global_redo(&mut self) {
        if self.pending_global_redo.take().is_some() {
            tracing::warn!("rehacer global: la ranura pedida ya no existe, se descarta ese paso");
            self.global_redo.pop();
        }
    }

    /// Olvida de la selección los ids que ya no existen en el documento
    /// (tras deshacer/rehacer un borrado, o después de cortar/borrar).
    pub(crate) fn forget_deleted_selection(&mut self) {
        if let Ok(page) = self.doc.page() {
            self.selection.retain_existing(page);
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

    /// ¿Hay algún gesto o edición de panel a medias, o un guardado/
    /// exportación en curso? Solo entonces es seguro cambiar de lienzo
    /// activo (`deck::apply_jump`): gracias a esta guardia los temporales de
    /// gesto no necesitan viajar en `SlotDoc`, porque siempre están vacíos
    /// en el instante del intercambio. `saving`/`exporting` son igual de
    /// importantes: un guardado en curso ya capturó su propio RGBA por
    /// valor (cambiar de activo no le tocaría los píxeles escritos), pero
    /// `AppMsg::Saved` opera sobre "lo que sea que esté activo cuando
    /// llegue" — si eso cambiase a mitad de guardado, marcaría como
    /// guardado el documento EQUIVOCADO.
    pub(crate) fn is_idle(&self) -> bool {
        matches!(self.gesture, Gesture::None)
            && self.panel_edit.is_none()
            && self.page_edit.is_none()
            && self.blur_edit.is_none()
            && self.color_edit.is_none()
            && self.content_edit.is_none()
            && self.shadow_edit.is_none()
            && self.rename_edit.is_none()
            && self.file_rename_edit.is_none()
            && !self.saving
            && !self.exporting
    }

    /// Extrae el lienzo activo a un `SlotDoc` para guardarlo en su ranura de
    /// la baraja, dejando `self` con un documento de relleno. Solo se llama
    /// con `is_idle() == true` (comprobado por el llamador, `deck::apply_jump`).
    pub(crate) fn take_slot(&mut self) -> crate::deck::SlotDoc {
        let bytes = self
            .images
            .values()
            .map(|img| img.width as usize * img.height as usize * 4)
            .sum();
        crate::deck::SlotDoc {
            doc: std::mem::replace(&mut self.doc, Document::new(1.0, 1.0)),
            history: std::mem::take(&mut self.history),
            images: std::mem::take(&mut self.images),
            selection: std::mem::take(&mut self.selection),
            background_layer: self.background_layer.take(),
            sidecar_enabled: self.sidecar_enabled,
            is_design: self.is_design,
            source_metadata: self.source_metadata.take(),
            saving: self.saving,
            save_error: self.save_error.take(),
            external_change: self.external_change,
            born_blank: self.born_blank,
            pending_creation: self.pending_creation,
            bytes,
        }
    }

    /// Instala un `SlotDoc` como lienzo activo (lo contrario de `take_slot`).
    pub(crate) fn put_slot(&mut self, slot: crate::deck::SlotDoc) {
        self.doc = slot.doc;
        self.history = slot.history;
        self.images = slot.images;
        self.selection = slot.selection;
        self.background_layer = slot.background_layer;
        self.sidecar_enabled = slot.sidecar_enabled;
        self.is_design = slot.is_design;
        self.source_metadata = slot.source_metadata;
        self.saving = slot.saving;
        self.save_error = slot.save_error;
        self.external_change = slot.external_change;
        self.born_blank = slot.born_blank;
        self.pending_creation = slot.pending_creation;
        // Los gestos y ediciones de panel se quedan a sus valores por
        // defecto (vacíos): `is_idle()` garantizaba que ya lo estaban antes
        // del salto. La selección y el fondo desenfocado ya llegan del slot;
        // el resto de campos "de sesión" (viewport, grid, crop_mode…) son
        // intencionalmente compartidos por toda la baraja, no por lienzo.
        self.forget_deleted_selection();
    }
}
