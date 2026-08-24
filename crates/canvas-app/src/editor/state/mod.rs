//! Estado del editor: el documento activo, sus imagenes, y el
//! deshacer/rehacer local y global - el modelo que `canvas_ui` y el panel de
//! propiedades leen y mutan, sin la parte de UI en si.

use std::path::PathBuf;

use canvas_core::{Document, History, LayerContent, LayerId, Selection, Transform};
use canvas_render::ImageMap;

/// Pestaña activa en el panel lateral izquierdo.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) enum LeftTab {
    Page,
    Layers,
}

use super::interaction::Gesture;
use super::Viewport;

mod background;
mod constructors;
mod history;
mod layer_factory;
mod shortcuts;
mod sidecar;

pub(crate) use history::{DeleteRecord, GlobalStep};

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
    /// Ajuste de opacidad en curso (slider): capa y opacidad original.
    pub(super) opacity_edit: Option<(LayerId, f32)>,
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
    /// Atajo de teclado (Ctrl+\) para plegar/desplegar el panel de capas.
    pub layers_panel_toggle: bool,
    /// Pestaña activa en el panel izquierdo.
    pub active_left_tab: LeftTab,
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
    /// Muestra únicamente el lienzo activo en el editor.
    pub isolate: bool,
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

/// Dirección de salto entre lienzos de la baraja, pedida por teclado.
#[derive(Clone, Copy)]
pub enum DeckNav {
    Next,
    Prev,
    First,
    Last,
}

impl EditorState {
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
            && self.opacity_edit.is_none()
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
