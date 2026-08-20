//! La baraja: todos los archivos de una carpeta como lienzos apilados en una
//! sola superficie con scroll continuo, para poder saltar de uno a otro
//! (tira lateral, `PageUp`/`PageDown`/`Home`/`End`, clic en el propio lienzo)
//! sin volver a la galería. Solo el lienzo ACTIVO vive en `EditorState`; los
//! demás viven aquí, cargados perezosamente y descartados cuando se alejan
//! demasiado de la vista.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use canvas_core::{Document, History, LayerId, Selection};
use canvas_render::{FxScope, ImageMap};
use eframe::egui;
use serde::{Deserialize, Serialize};

use crate::gallery::{GalleryState, ItemKind};
use crate::settings::GallerySort;

/// Eje de apilado de la baraja. Solo cambia el bucle de `Deck::relayout` (qué
/// coordenada acumula, cuál centra) y qué componente de la rueda del ratón
/// mueve `canvas_ui` a lo largo del eje primario — el resto (carga perezosa,
/// descarte, `visible_indices`, `bounds`) trabaja sobre `DeckRect` sin saber
/// ni le importa en qué eje está apilada la baraja.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, Debug)]
pub enum DeckAxis {
    #[default]
    Vertical,
    Horizontal,
}

impl DeckAxis {
    pub fn toggled(self) -> Self {
        match self {
            DeckAxis::Vertical => DeckAxis::Horizontal,
            DeckAxis::Horizontal => DeckAxis::Vertical,
        }
    }
}

/// Lado de la ventana donde vive la tira de miniaturas. DELIBERADAMENTE
/// independiente de `DeckAxis`: el eje decide cómo se APILAN los lienzos en
/// el espacio de baraja (`Deck::relayout`) y qué componente de la rueda
/// desplaza a lo largo de la pila (`canvas_ui`); esto decide solo dónde se
/// dibuja el panel de la tira. Estaban acoplados por convención, no por
/// necesidad — nadie que use `axis` mira la tira, y `deck_strip_ui` no mira
/// `relayout`.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, Debug)]
pub enum StripSide {
    #[default]
    Left,
    Right,
    Top,
    Bottom,
}

impl StripSide {
    /// Siguiente lado en sentido horario: Left → Top → Right → Bottom →
    /// Left. Cuatro estados, así que un `toggled()` estilo `DeckAxis` no
    /// vale: un solo botón que cicla es lo que cabe en una cabecera de
    /// 96 px.
    pub fn cycled(self) -> Self {
        match self {
            StripSide::Left => StripSide::Top,
            StripSide::Top => StripSide::Right,
            StripSide::Right => StripSide::Bottom,
            StripSide::Bottom => StripSide::Left,
        }
    }

    /// ¿Las celdas fluyen en columna? Left/Right sí (el ancho manda), Top/
    /// Bottom no (manda el alto). El único bit que necesita `deck_strip_ui`:
    /// el mapeo 4→2 que evita duplicar el cuerpo del panel.
    pub fn is_vertical_flow(self) -> bool {
        matches!(self, StripSide::Left | StripSide::Right)
    }

    /// Glifo del botón de la cabecera: apunta al lado ACTUAL.
    pub fn glyph(self) -> &'static str {
        match self {
            StripSide::Left => "◀",
            StripSide::Top => "▲",
            StripSide::Right => "▶",
            StripSide::Bottom => "▼",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            StripSide::Left => "Left",
            StripSide::Top => "Top",
            StripSide::Right => "Right",
            StripSide::Bottom => "Bottom",
        }
    }
}

/// Dirección para `Deck::move_slot` — las flechas ◀/▶ de la cabecera de
/// cada lienzo en el área central.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MoveDir {
    Prev,
    Next,
}

/// Ranuras vecinas a la activa que se cargan siempre, se vean o no: al
/// saltar con `PageUp`/`PageDown` el destino inmediato ya está listo.
const PRELOAD_RADIUS: usize = 2;
/// Cargas simultáneas en vuelo. Más de dos solo hace competir a los hilos
/// por el disco sin acelerar nada.
const MAX_INFLIGHT_LOADS: usize = 2;
/// Techo duro de ranuras cargadas a la vez, además del presupuesto de bytes.
const MAX_LOADED_SLOTS: usize = 12;
/// Presupuesto de píxeles decodificados en RAM, sin contar la activa. Una
/// foto de 20 MP son ~80 MB en RGBA: esto son unas 6 fotos así.
const EVICT_BUDGET_BYTES: usize = 512 * 1024 * 1024;
/// El rect visible se dilata esta fracción a cada lado antes de decidir qué
/// cargar/mostrar: evita que una ranura entre y salga de "visible" con un
/// scroll de un solo píxel.
const VISIBLE_MARGIN: f64 = 0.5;

/// Rect en espacio de baraja (px de página). `f64`: una carpeta de 200 fotos
/// llega al millón de píxeles acumulados y `f32` empieza a perder precisión
/// al hacer zoom sobre una ranura lejana.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DeckRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl DeckRect {
    pub const ZERO: Self = Self {
        x: 0.0,
        y: 0.0,
        w: 0.0,
        h: 0.0,
    };

    pub fn origin(&self) -> (f64, f64) {
        (self.x, self.y)
    }

    fn intersects(&self, other: DeckRect) -> bool {
        self.x < other.x + other.w
            && self.x + self.w > other.x
            && self.y < other.y + other.h
            && self.y + self.h > other.y
    }
}

/// Estado de carga de una ranura.
pub enum SlotContent {
    /// Nunca se pidió, o se descartó por presupuesto: solo se conoce su
    /// tamaño sondeado (si ha llegado) y su miniatura.
    Idle,
    /// Carga en vuelo en un hilo de trabajo.
    Loading,
    /// Cargado, listo para renderizar y para activarse.
    Ready(Box<SlotDoc>),
    /// La carga falló; no se reintenta sola, solo si el usuario la activa.
    Failed(String),
    /// ES el lienzo activo: su contenido está prestado a `EditorState`
    /// (`EditorState::take_slot`/`put_slot`).
    Active,
}

/// Lo que hoy vive suelto en `EditorState` para el único documento cargado.
/// Deliberadamente NO lleva viewport, gestos en curso ni ediciones de panel
/// a medias: `EditorState::is_idle()` garantiza que están vacíos en el
/// instante del intercambio, así que no hace falta transportarlos.
pub struct SlotDoc {
    pub doc: Document,
    pub history: History,
    pub images: ImageMap,
    pub selection: Selection,
    pub background_layer: Option<LayerId>,
    pub sidecar_enabled: bool,
    pub is_design: bool,
    pub source_metadata: Option<canvas_io::ImageMetadata>,
    pub saving: bool,
    pub save_error: Option<String>,
    /// El archivo cambió en disco desde el último guardado con capas. Para
    /// una carga de fondo esto NUNCA abre un diálogo (sería un modal
    /// disparado por hacer scroll); se enciende en silencio y se muestra
    /// como el banner normal de «cambió por fuera» en cuanto la ranura se
    /// active.
    pub external_change: bool,
    /// Nació en blanco y no se ha guardado ni una vez — ver
    /// `EditorState::born_blank`.
    pub born_blank: bool,
    /// Bytes aproximados en RAM (Σ ancho·alto·4 de `images`), para el
    /// presupuesto de descarte.
    pub bytes: usize,
}

/// Una ranura = un archivo de la carpeta = un lienzo.
pub struct Slot {
    /// Identidad estable durante toda la sesión, aunque la lista se reordene
    /// o el archivo se renombre. También el `FxScope` con el que se habla
    /// con `CanvasRenderer`.
    pub id: u64,
    pub path: PathBuf,
    pub name: String,
    pub kind: ItemKind,
    pub mtime: Option<SystemTime>,
    pub thumb: Option<egui::TextureHandle>,
    pub thumb_failed: bool,
    /// Tamaño de página sondeado (`canvas_io::probe_page_size`). `None`
    /// hasta que llega `DeckProbed`: mientras tanto `Slot::size()` estima
    /// con la miniatura.
    pub page: Option<(f64, f64)>,
    /// Geometría en espacio de baraja, recalculada por `Deck::relayout`.
    pub rect: DeckRect,
    pub content: SlotContent,
    /// Último frame en que la ranura estuvo visible (LRU de descarte).
    last_seen: u64,
    /// Orden manual: solo se usa cuando `Deck::sort == GallerySort::Manual`
    /// (`Deck::apply_sort` ordena por este campo). Se conserva POR RANURA,
    /// no por posición en el `Vec`, porque `merge_scan` reconstruye la lista
    /// en el orden del escaneo de disco y luego reordena — un orden manual
    /// que solo viviera en la posición se perdería en el siguiente
    /// reescaneo. `merge_scan` ya conserva la ranura existente completa al
    /// reencontrarla por ruta, así que este campo sobrevive gratis; solo una
    /// ranura NUEVA recibe un valor fresco (al final).
    pub order_hint: u64,
    /// Diseño bloqueado: ni gestos sobre el lienzo (`layer_interaction`) ni
    /// los paneles de capas/propiedades pueden editarlo. A nivel de DISEÑO
    /// completo — deliberadamente independiente de `Layer::locked` (por
    /// capa, en `canvas-core`).
    pub locked: bool,
    /// Ranura PROVISIONAL: un lienzo en blanco que el usuario pidió pero que
    /// todavía no existe en disco. Su `path` es un nombre «asomado»
    /// (`canvas_io::peek_unique_path`) que aún nadie ha reservado. Se
    /// materializa — nombre reservado de verdad y archivo escrito — en
    /// cuanto el usuario la edita. Mientras tanto queda FUERA de la
    /// reconciliación con el disco (`merge_scan`), del descarte por
    /// presupuesto (`evict`), de la carga perezosa (`request_loads`) y de la
    /// siembra de la galería (`seed_gallery_from_deck`): ninguno de esos
    /// tiene sentido sobre un archivo que no existe.
    pub is_placeholder: bool,
}

impl Slot {
    /// Tamaño con el que se dispone esta ranura, en orden de fiabilidad: el
    /// tamaño de página sondeado; si no ha llegado aún, el aspecto de su
    /// miniatura escalado a lado mayor 1600 px; si tampoco hay miniatura,
    /// 1600×1600. Nunca `(0,0)`: la disposición no puede dividir por cero.
    fn size(&self) -> (f64, f64) {
        if let Some(page) = self.page {
            return page;
        }
        if let Some(tex) = &self.thumb {
            let size = tex.size_vec2();
            if size.x > 0.0 && size.y > 0.0 {
                let scale = 1600.0 / f64::from(size.x.max(size.y));
                return (f64::from(size.x) * scale, f64::from(size.y) * scale);
            }
        }
        (1600.0, 1600.0)
    }
}

/// La baraja del editor: todos los archivos de `folder`, con el activo
/// marcado por índice. `folder` es `None` para un archivo abierto suelto
/// (arrastrar y soltar, CLI, recientes): baraja degenerada de una ranura.
pub struct Deck {
    pub folder: Option<PathBuf>,
    pub slots: Vec<Slot>,
    pub active: usize,
    pub sort: GallerySort,
    /// La tira lateral está visible (se oculta además si `slots.len() <= 1`).
    pub strip_visible: bool,
    /// Eje de apilado (vertical u horizontal); el llamador (`App`) lo siembra
    /// desde `settings.deck_axis` al construir la baraja, y lo persiste de
    /// vuelta cuando el usuario lo cambia — `Deck` en sí no conoce ajustes.
    pub axis: DeckAxis,
    /// Lado donde se ancla la tira. Igual que `axis`, lo siembra `App` desde
    /// `settings.deck_strip_side` y lo persiste de vuelta — `Deck` no conoce
    /// `AppSettings`. Independiente de `axis` (ver doc de `StripSide`).
    pub strip_side: StripSide,
    /// Petición de saltar a otro lienzo (clic en un lienzo del workspace o
    /// en la tira, o `PageUp`/`PageDown`/`Home`/`End`); `apply_jump` la
    /// consume. A diferencia de la Fase 14a/14b, el salto es SIN PÉRDIDA: el
    /// lienzo saliente queda guardado en su propia ranura con su historial
    /// de deshacer intacto, así que no hace falta preguntar por cambios sin
    /// guardar para saltar dentro de la misma baraja.
    pub jump_to: Option<usize>,
    /// Si el salto pendiente, al aplicarse, debe recentrar la vista. Un clic
    /// directo sobre un lienzo (siempre visible, es lo que se acaba de
    /// pulsar) no lo necesita; un salto por la tira o el teclado sí, porque
    /// el destino puede no estar a la vista. Quien fija `jump_to` fija
    /// también esto explícitamente, no queda implícito.
    pub jump_center: bool,
    /// La geometría necesita recalcularse (llegaron sondas nuevas, cambió
    /// el orden…).
    pub layout_dirty: bool,
    /// Rect (en espacio de baraja, como `Slot::rect`) donde iría el PRÓXIMO
    /// lienzo si se añadiera uno — justo después de la última ranura, con
    /// el mismo tamaño y hueco que separa a las demás. Lo recalcula
    /// `relayout()`; `canvas_ui` lo pinta como zona "+" y resuelve el clic
    /// sobre ella igual que sobre cualquier `Slot::rect`.
    pub add_zone: DeckRect,
    /// Renombrado en curso desde la cabecera del lienzo en el área central:
    /// (id de la ranura, texto del cuadro). Mientras sea `Some`, `canvas_ui`
    /// dibuja un `egui::Area` en primer plano anclado a esa cabecera en vez
    /// del nombre estático — mismo patrón que `GalleryState::rename_edit`.
    pub rename_edit: Option<(u64, String)>,
    inflight: usize,
    next_id: u64,
    /// Siguiente `Slot::order_hint` a repartir — mismo patrón que `next_id`,
    /// pero un contador independiente: el orden manual no tiene por qué
    /// coincidir con el orden de creación de las ranuras.
    next_order: u64,
}

impl Default for Deck {
    fn default() -> Self {
        Self {
            folder: None,
            slots: Vec::new(),
            active: 0,
            sort: GallerySort::default(),
            strip_visible: false,
            axis: DeckAxis::default(),
            strip_side: StripSide::default(),
            jump_to: None,
            jump_center: false,
            // Arranca sucio a propósito: el primer `canvas_ui` siempre
            // dispone la baraja al menos una vez, aunque solo haya una
            // ranura (da igual el resultado — `relayout` con una ranura es
            // `rect = (0,0,w,h)` — pero así no hay un caso especial "recién
            // creada" que recordar en el llamante).
            layout_dirty: true,
            add_zone: DeckRect::ZERO,
            rename_edit: None,
            inflight: 0,
            next_id: 1,
            next_order: 1,
        }
    }
}

/// Semilla que la galería entrega al editor al abrir un archivo desde ella:
/// rutas, nombres y las miniaturas YA subidas a GPU. `TextureHandle` es un
/// handle contado, clonarlo es gratis — evita re-decodificar y volver a
/// subir las miniaturas que la galería ya tenía cargadas.
pub struct DeckSeed {
    pub folder: PathBuf,
    pub sort: GallerySort,
    pub items: Vec<SeedItem>,
}

pub struct SeedItem {
    pub path: PathBuf,
    pub name: String,
    pub kind: ItemKind,
    pub mtime: Option<SystemTime>,
    pub thumb: Option<egui::TextureHandle>,
    pub thumb_failed: bool,
}

impl DeckSeed {
    /// Extrae los ítems de una galería (`std::mem::take`, conservando sus
    /// miniaturas ya cargadas) justo antes de navegar al editor.
    pub fn from_gallery(g: &mut GalleryState) -> Self {
        let items = std::mem::take(&mut g.items)
            .into_iter()
            .map(|i| SeedItem {
                path: i.path,
                name: i.name,
                kind: i.kind,
                mtime: i.mtime,
                thumb: i.tex,
                thumb_failed: i.failed,
            })
            .collect();
        Self {
            folder: g.folder.clone(),
            sort: g.sort,
            items,
        }
    }
}

fn idle_slot(id: u64, path: PathBuf, mtime: Option<SystemTime>, order_hint: u64) -> Slot {
    let kind = slot_kind(&path);
    let name = file_name(&path);
    Slot {
        id,
        path,
        name,
        kind,
        mtime,
        thumb: None,
        thumb_failed: false,
        page: None,
        rect: DeckRect::ZERO,
        content: SlotContent::Idle,
        last_seen: 0,
        is_placeholder: false,
        order_hint,
        locked: false,
    }
}

impl Deck {
    /// Baraja degenerada de una ranura: archivo abierto suelto (CLI,
    /// arrastrar y soltar, recientes, segunda instancia). La única ranura
    /// es la activa desde el principio.
    pub fn single(path: PathBuf) -> Self {
        let mut deck = Self::default();
        let mut slot = idle_slot(deck.next_id, path, None, deck.next_order);
        slot.content = SlotContent::Active;
        deck.slots.push(slot);
        deck.next_id += 1;
        deck.next_order += 1;
        deck
    }

    /// Construye la baraja a partir de lo que dejó la galería, activando el
    /// archivo recién abierto (por ruta; si no aparece en la semilla — no
    /// debería pasar —, activa el primero).
    pub fn from_seed(seed: DeckSeed, active_path: &Path) -> Self {
        let mut deck = Self {
            folder: Some(seed.folder),
            sort: seed.sort,
            strip_visible: true,
            ..Self::default()
        };
        for item in seed.items {
            let id = deck.next_id;
            deck.next_id += 1;
            let order_hint = deck.next_order;
            deck.next_order += 1;
            deck.slots.push(Slot {
                id,
                path: item.path,
                name: item.name,
                kind: item.kind,
                mtime: item.mtime,
                thumb: item.thumb,
                thumb_failed: item.thumb_failed,
                page: None,
                rect: DeckRect::ZERO,
                content: SlotContent::Idle,
                last_seen: 0,
                is_placeholder: false,
                order_hint,
                locked: false,
            });
        }
        deck.active = deck
            .slots
            .iter()
            .position(|s| s.path == active_path)
            .unwrap_or(0);
        if let Some(slot) = deck.slots.get_mut(deck.active) {
            slot.content = SlotContent::Active;
        }
        deck
    }

    /// Añade AL FINAL una ranura provisional: un lienzo en blanco del tamaño
    /// de página heredado, sin archivo detrás todavía. `ext` es la extensión
    /// elegida en Ajustes (`settings.new_canvas_format`) — `"canvas"` sigue
    /// siendo un diseño autónomo, cualquier otra cosa es un raster real con
    /// su sidecar. Devuelve su índice para que el llamador salte a ella.
    /// `None` si la baraja no tiene carpeta (`Deck::single`: un archivo
    /// suelto no tiene dónde escribir un hermano).
    ///
    /// IDEMPOTENTE: nunca hay más de una provisional a la vez. Si ya existe
    /// una, no crea otra — devuelve la que hay, que es exactamente a donde
    /// el usuario quería ir al pulsar «+» por segunda vez.
    pub fn push_placeholder(&mut self, page: (f64, f64), ext: &str) -> Option<usize> {
        let folder = self.folder.clone()?;
        if let Some(i) = self.slots.iter().position(|s| s.is_placeholder) {
            return Some(i);
        }
        let is_design = ext == canvas_io::CANVAS_EXTENSION;
        let path = canvas_io::peek_unique_path(&folder, "Untitled", ext);
        let mut state = if is_design {
            crate::editor::EditorState::new_blank(page.0, page.1)
        } else {
            crate::editor::EditorState::new_blank_image(page.0, page.1)
        };
        let mut doc = state.take_slot();
        // Un `History::default()` arranca con `saved_depth: None`, o sea
        // `is_dirty() == true`: un lienzo recién creado se leería como «el
        // usuario ya lo ha editado» y se materializaría solo, sin que nadie
        // dibujara nada. Sellarlo aquí es lo que hace que la transición a
        // sucio signifique de verdad «lo ha tocado».
        doc.history.mark_saved();
        let id = self.next_id;
        self.next_id += 1;
        let order_hint = self.next_order;
        self.next_order += 1;
        self.slots.push(Slot {
            id,
            name: file_name(&path),
            path,
            kind: if is_design {
                ItemKind::Design
            } else {
                ItemKind::Image
            },
            mtime: None,
            thumb: None,
            thumb_failed: false,
            // Ya se conoce su tamaño (hereda el de la página anterior), así
            // que `spawn_deck_probe` (que solo mira `page.is_none()`) no
            // intenta sondear un archivo que no existe.
            page: Some(page),
            // Nace YA colocada donde `add_zone` predijo que iría el próximo
            // lienzo — NO en `DeckRect::ZERO`. `layout_dirty=true` (abajo)
            // dispara un `relayout()` de verdad en el próximo `canvas_ui`,
            // pero quien llama a `push_placeholder` normalmente encadena
            // `jump_to`/`jump_center` y centra la cámara EN ESTE MISMO
            // frame (`deck::apply_jump` + `request_center` en `main.rs`,
            // después de `canvas_ui`) — con `DeckRect::ZERO` esa lectura
            // encontraba un rect en el origen de la baraja (el del PRIMER
            // lienzo apilado) en vez del de esta ranura, y la cámara
            // centraba ahí: "añadir salta al primer lienzo". El tamaño
            // puede no coincidir exacto si el llamador pidió uno distinto
            // al de `add_zone` (el botón de la tira usa
            // `settings.last_page_size`) — se autocorrige con el próximo
            // `relayout()`, y para entonces el centrado ya apuntó al sitio
            // correcto.
            rect: self.add_zone,
            // NUNCA `Idle`: `request_loads` mandaría a cargar de disco un
            // archivo que todavía no está ahí y acabaría en `Failed`.
            content: SlotContent::Ready(Box::new(doc)),
            last_seen: 0,
            is_placeholder: true,
            order_hint,
            locked: false,
        });
        self.layout_dirty = true;
        Some(self.slots.len() - 1)
    }

    /// La tira solo tiene sentido con más de un lienzo.
    pub fn is_visible(&self) -> bool {
        self.strip_visible && self.slots.len() > 1
    }

    pub fn active_path(&self) -> Option<PathBuf> {
        self.slots.get(self.active).map(|s| s.path.clone())
    }

    pub fn active_rect(&self) -> DeckRect {
        self.slots
            .get(self.active)
            .map(|s| s.rect)
            .unwrap_or(DeckRect::ZERO)
    }

    /// Origen del lienzo activo en el espacio de baraja (px de página): el
    /// gancho que usa `canvas_ui` para desplazar `layer_interaction` y los
    /// ayudantes de coordenadas sin tocarlos.
    pub fn active_origin(&self) -> (f64, f64) {
        self.active_rect().origin()
    }

    pub fn find_by_path(&self, path: &Path) -> Option<usize> {
        self.slots.iter().position(|s| s.path == path)
    }

    /// Igual que `find_by_path`, pero por el id estable de la ranura — lo
    /// usa `App::save_all_queue`, que necesita reencontrar una ranura
    /// aunque el orden haya cambiado entre frames (reordenado, renombrado).
    pub fn find_by_id(&self, id: u64) -> Option<usize> {
        self.slots.iter().position(|s| s.id == id)
    }

    /// Intercambia la posición VISUAL de la ranura `id` con su vecina
    /// adyacente (en el orden actual, sea cual sea). Un uso de las flechas
    /// ◀/▶ es una declaración explícita de "quiero ESTE orden": cambia
    /// `self.sort` a `Manual` para que el próximo reescaneo no lo deshaga.
    /// `false` si `id` no existe o ya está en el extremo correspondiente.
    pub fn move_slot(&mut self, id: u64, dir: MoveDir) -> bool {
        let Some(idx) = self.find_by_id(id) else {
            return false;
        };
        let neighbor_idx = match dir {
            MoveDir::Prev => idx.checked_sub(1),
            MoveDir::Next if idx + 1 < self.slots.len() => Some(idx + 1),
            MoveDir::Next => None,
        };
        let Some(neighbor_idx) = neighbor_idx else {
            return false;
        };
        // Al entrar en modo manual por primera vez, normaliza TODOS los
        // hints a la posición visual actual (la que dejó Name/DateModified)
        // — si no, heredarían valores de creación sin relación con lo que
        // se ve, y el resto de la baraja "saltaría" de sitio en el próximo
        // `apply_sort`, no solo la pareja que se acaba de mover.
        if self.sort != GallerySort::Manual {
            for (i, slot) in self.slots.iter_mut().enumerate() {
                slot.order_hint = i as u64;
            }
            self.next_order = self.slots.len() as u64;
        }
        self.sort = GallerySort::Manual;
        let hint_a = self.slots[idx].order_hint;
        let hint_b = self.slots[neighbor_idx].order_hint;
        self.slots[idx].order_hint = hint_b;
        self.slots[neighbor_idx].order_hint = hint_a;
        // `apply_sort` reordena `self.slots` por posición, no por id —
        // `self.active` es un ÍNDICE, así que hay que reencontrar la activa
        // por su id estable después de reordenar (mismo motivo por el que
        // `merge_scan` restaura `self.active` por ruta tras su propio
        // `apply_sort`).
        let active_id = self.slots.get(self.active).map(|s| s.id);
        self.apply_sort();
        if let Some(active_id) = active_id {
            if let Some(i) = self.find_by_id(active_id) {
                self.active = i;
            }
        }
        true
    }

    fn apply_sort(&mut self) {
        match self.sort {
            // Natural/numérico, no byte a byte: ver la doc de `natural_cmp`
            // (settings.rs) — sin esto, "6.png".."9.png" acaban después de
            // "51.png" en una carpeta sin ceros de relleno.
            GallerySort::Name => self
                .slots
                .sort_by(|a, b| crate::settings::natural_cmp(&a.name, &b.name)),
            // Más recientes primero; sin fecha, al final. Mismo criterio que
            // `GalleryState::apply_sort`.
            GallerySort::DateModified => self.slots.sort_by(|a, b| {
                b.mtime
                    .cmp(&a.mtime)
                    .then_with(|| crate::settings::natural_cmp(&a.name, &b.name))
            }),
            // El orden que dejaron las flechas de mover (`Deck::move_slot`).
            GallerySort::Manual => self.slots.sort_by_key(|s| s.order_hint),
        }
        self.layout_dirty = true;
    }

    /// Sustituye la lista de archivos conservando por ruta todo lo que ya
    /// tenía la ranura (id, miniatura, tamaño sondeado y, sobre todo, el
    /// CONTENIDO cargado) — mismo espíritu que `GalleryState::merge_files`.
    /// Una ranura cuyo archivo desapareció del disco se descarta, SALVO que
    /// sea la activa o tenga cambios sin guardar: perder ese trabajo en
    /// silencio por un simple reescaneo sería peor que dejar un lienzo
    /// «huérfano» a la vista.
    pub fn merge_scan(&mut self, files: Vec<(PathBuf, Option<SystemTime>)>) {
        let active_path = self.active_path();
        // Las ranuras PROVISIONALES no están en disco, así que el listado
        // nunca las trae y la regla de conservación de abajo (activa o
        // sucia) las tiraría: una provisional recién creada no es ninguna de
        // las dos. Se apartan ANTES de reconciliar (también evita que
        // colisionen en el `HashMap` de abajo si su nombre asomado coincide
        // con un archivo real) y se vuelven a pegar al final — que es su
        // sitio — DESPUÉS de ordenar pero ANTES de restaurar la activa: si
        // no, una provisional activa perdería su índice en cuanto el
        // reordenado la desplazase.
        let (placeholders, rest): (Vec<Slot>, Vec<Slot>) =
            self.slots.drain(..).partition(|s| s.is_placeholder);
        let mut old: HashMap<PathBuf, Slot> =
            rest.into_iter().map(|s| (s.path.clone(), s)).collect();
        let mut next_id = self.next_id;
        let mut slots = Vec::with_capacity(files.len());
        for (path, mtime) in files {
            match old.remove(&path) {
                Some(mut existing) => {
                    existing.mtime = mtime;
                    slots.push(existing);
                }
                None => {
                    let id = next_id;
                    next_id += 1;
                    let order_hint = self.next_order;
                    self.next_order += 1;
                    slots.push(idle_slot(id, path, mtime, order_hint));
                }
            }
        }
        for (_, slot) in old {
            let keep = matches!(&slot.content, SlotContent::Active)
                || matches!(&slot.content, SlotContent::Ready(doc) if doc.history.is_dirty());
            if keep {
                slots.push(slot);
            }
        }
        self.slots = slots;
        self.next_id = next_id;
        self.apply_sort();
        self.slots.extend(placeholders);
        if let Some(p) = active_path {
            if let Some(idx) = self.find_by_path(&p) {
                self.active = idx;
            }
        }
    }

    /// Entrega una miniatura llegada de un hilo de trabajo (por ruta: el
    /// orden puede haber cambiado desde que se lanzó el escaneo).
    pub fn set_thumb(&mut self, path: &Path, tex: Option<egui::TextureHandle>) {
        if let Some(slot) = self.slots.iter_mut().find(|s| s.path == path) {
            match tex {
                Some(tex) => slot.thumb = Some(tex),
                None => slot.thumb_failed = true,
            }
        }
    }

    /// Rellena el tamaño de página sondeado de toda la carpeta, llegado en
    /// un solo mensaje (`DeckProbed`) para que haga falta un único
    /// `relayout`, no uno por archivo.
    pub fn set_probes(&mut self, sizes: Vec<(PathBuf, Option<(f64, f64)>)>) {
        let mut by_path: HashMap<PathBuf, (f64, f64)> = sizes
            .into_iter()
            .filter_map(|(p, s)| s.map(|s| (p, s)))
            .collect();
        for slot in &mut self.slots {
            if let Some(size) = by_path.remove(&slot.path) {
                slot.page = Some(size);
            }
        }
        self.layout_dirty = true;
    }

    /// Recoloca todas las ranuras en una fila o columna (según `self.axis`),
    /// centradas en el eje transversal, con un hueco proporcional al ancho de
    /// la pila (en espacio de baraja, no en pantalla: constante entre fotos
    /// grandes, no desproporcionado entre miniaturas pequeñas). Con una sola
    /// ranura da `rect = (0,0,w,h)` en cualquiera de los dos ejes. También
    /// calcula `add_zone`: el rect donde iría el PRÓXIMO lienzo, justo
    /// después del último con el mismo hueco (o en el origen, con un tamaño
    /// por defecto, si la baraja está vacía).
    pub fn relayout(&mut self) {
        let sizes: Vec<(f64, f64)> = self.slots.iter().map(Slot::size).collect();
        // Tamaño por defecto de una baraja vacía: el de la última ranura, o
        // (si no hay ninguna) el mismo por defecto que usa una página nueva.
        let add_size = sizes.last().copied().unwrap_or((1920.0, 1080.0));
        match self.axis {
            DeckAxis::Vertical => {
                let deck_w = sizes
                    .iter()
                    .fold(0.0_f64, |m, &(w, _)| m.max(w))
                    .max(add_size.0);
                let gap = (deck_w * 0.03).clamp(24.0, 400.0);
                let mut y = 0.0;
                for (slot, (w, h)) in self.slots.iter_mut().zip(sizes) {
                    slot.rect = DeckRect {
                        x: (deck_w - w) / 2.0,
                        y,
                        w,
                        h,
                    };
                    y += h + gap;
                }
                self.add_zone = DeckRect {
                    x: (deck_w - add_size.0) / 2.0,
                    y,
                    w: add_size.0,
                    h: add_size.1,
                };
            }
            DeckAxis::Horizontal => {
                let deck_h = sizes
                    .iter()
                    .fold(0.0_f64, |m, &(_, h)| m.max(h))
                    .max(add_size.1);
                let gap = (deck_h * 0.03).clamp(24.0, 400.0);
                let mut x = 0.0;
                for (slot, (w, h)) in self.slots.iter_mut().zip(sizes) {
                    slot.rect = DeckRect {
                        x,
                        y: (deck_h - h) / 2.0,
                        w,
                        h,
                    };
                    x += w + gap;
                }
                self.add_zone = DeckRect {
                    x,
                    y: (deck_h - add_size.1) / 2.0,
                    w: add_size.0,
                    h: add_size.1,
                };
            }
        }
        self.layout_dirty = false;
    }

    /// Rect envolvente de toda la pila, para «Fit all» (`Ctrl+Alt+0`).
    /// Min/max genéricos sobre ambos ejes en vez de asumir apilado vertical
    /// desde `(0,0)`: funciona igual con `DeckAxis::Horizontal`, donde es el
    /// eje X el que recorre toda la pila y el Y el que queda acotado.
    pub fn bounds(&self) -> DeckRect {
        if self.slots.is_empty() {
            return DeckRect::ZERO;
        }
        // Incluye `add_zone` (si hay carpeta: sin ella no se pinta ni se
        // puede pulsar) — si no, "ver toda la baraja" (`Ctrl+Alt+0`) deja la
        // zona "+" fuera o al borde del encuadre.
        let rects = self
            .slots
            .iter()
            .map(|s| s.rect)
            .chain(self.folder.is_some().then_some(self.add_zone));
        let mut x0 = f64::INFINITY;
        let mut x1 = f64::NEG_INFINITY;
        let mut y0 = f64::INFINITY;
        let mut y1 = f64::NEG_INFINITY;
        for r in rects {
            x0 = x0.min(r.x);
            x1 = x1.max(r.x + r.w);
            y0 = y0.min(r.y);
            y1 = y1.max(r.y + r.h);
        }
        DeckRect {
            x: x0,
            y: y0,
            w: (x1 - x0).max(1.0),
            h: (y1 - y0).max(1.0),
        }
    }

    /// Índices cuyo rect corta `view` (ya dilatado por el llamante — usar
    /// `dilate`).
    pub fn visible_indices(&self, view: DeckRect) -> Vec<usize> {
        self.slots
            .iter()
            .enumerate()
            .filter(|(_, s)| s.rect.intersects(view))
            .map(|(i, _)| i)
            .collect()
    }

    /// Dilata un rect de viewport `VISIBLE_MARGIN` a cada lado — el margen
    /// de precarga: una ranura entra en juego un poco antes de asomar de
    /// verdad, para que no haya un parpadeo al llegar a ella.
    pub fn dilate(view: DeckRect) -> DeckRect {
        let mx = view.w * VISIBLE_MARGIN;
        let my = view.h * VISIBLE_MARGIN;
        DeckRect {
            x: view.x - mx,
            y: view.y - my,
            w: view.w + 2.0 * mx,
            h: view.h + 2.0 * my,
        }
    }

    /// Sella `last_seen` de las ranuras visibles este frame (política LRU
    /// de descarte).
    pub fn mark_visible(&mut self, visible: &[usize]) {
        let frame = self
            .slots
            .iter()
            .map(|s| s.last_seen)
            .max()
            .unwrap_or(0)
            .wrapping_add(1);
        for &i in visible {
            if let Some(s) = self.slots.get_mut(i) {
                s.last_seen = frame;
            }
        }
    }

    /// Pide la carga de las ranuras `Idle` entre las visibles, las del radio
    /// de precarga alrededor de la activa, y el destino de un salto pendiente
    /// (`jump_to`) si lo hay — `apply_jump` exige que ese destino esté
    /// `Ready`, así que sin esto un salto a una ranura fuera del radio de
    /// precarga se quedaría pedido para siempre, porque nada dispararía
    /// jamás su carga. El destino de `jump_to` va primero (es lo que el
    /// usuario está esperando activamente), el resto por cercanía a la
    /// activa como antes. Respeta `MAX_INFLIGHT_LOADS`. Devuelve las rutas
    /// para las que `App` debe lanzar `loader::spawn_load_slot`; las marca
    /// `Loading`.
    pub fn request_loads(&mut self, visible: &[usize]) -> Vec<PathBuf> {
        if self.slots.is_empty() {
            return Vec::new();
        }
        let active = self.active;
        let jump = self.jump_to.filter(|&i| i < self.slots.len());
        let lo = active.saturating_sub(PRELOAD_RADIUS);
        let hi = (active + PRELOAD_RADIUS).min(self.slots.len() - 1);
        let mut candidates: Vec<usize> = visible.iter().copied().chain(lo..=hi).collect();
        if let Some(j) = jump {
            candidates.push(j);
        }
        candidates.sort_unstable();
        candidates.dedup();
        // `!s.is_placeholder` es defensa en profundidad: con `evict`
        // guardada (ver más abajo) una provisional nunca debería llegar
        // aquí en `Idle`, pero la condición es barata y evita cargar de
        // disco un archivo que aún no existe si algo falla.
        candidates.retain(|&i| {
            matches!(self.slots.get(i), Some(s) if matches!(s.content, SlotContent::Idle) && !s.is_placeholder)
        });
        candidates.sort_by_key(|&i| (Some(i) != jump, i.abs_diff(active)));

        let mut spawned = Vec::new();
        for i in candidates {
            if self.inflight >= MAX_INFLIGHT_LOADS {
                break;
            }
            if let Some(slot) = self.slots.get_mut(i) {
                slot.content = SlotContent::Loading;
                self.inflight += 1;
                spawned.push(slot.path.clone());
            }
        }
        spawned
    }

    /// Una carga (con éxito o sin él) terminó: libera un hueco de
    /// `MAX_INFLIGHT_LOADS`.
    pub fn loading_finished(&mut self) {
        self.inflight = self.inflight.saturating_sub(1);
    }

    fn loaded_bytes(&self) -> usize {
        self.slots
            .iter()
            .filter_map(|s| match &s.content {
                SlotContent::Ready(d) => Some(d.bytes),
                _ => None,
            })
            .sum()
    }

    /// Descarta ranuras `Ready` lejanas, limpias y sin guardado en curso
    /// hasta volver al presupuesto (la más lejos vista primero). Nunca
    /// descarta la activa ni una sucia. Devuelve los `FxScope` a liberar en
    /// el renderer — el llamador, que tiene el `CanvasRenderer`, hace el
    /// `forget_scope` (aquí no se acopla `Deck` a `canvas-render` más que
    /// por el tipo del scope).
    pub fn evict(&mut self) -> Vec<FxScope> {
        let active = self.active;
        let mut freed = Vec::new();
        loop {
            let loaded_count = self
                .slots
                .iter()
                .filter(|s| matches!(s.content, SlotContent::Ready(_)))
                .count();
            if self.loaded_bytes() <= EVICT_BUDGET_BYTES && loaded_count <= MAX_LOADED_SLOTS {
                break;
            }
            let candidate = self
                .slots
                .iter()
                .enumerate()
                .filter(|(i, s)| {
                    *i != active
                        && i.abs_diff(active) > PRELOAD_RADIUS
                        // Una provisional está LIMPIA por construcción
                        // (`mark_saved` al nacer en `push_placeholder`), así
                        // que pasaría el resto de este filtro y volvería a
                        // `Idle` — y desde `Idle`, `request_loads`
                        // intentaría cargar de disco un archivo que aún no
                        // existe, dejándola en `Failed`. No se puede confiar
                        // en que otro estado la proteja por accidente.
                        && !s.is_placeholder
                        && matches!(&s.content, SlotContent::Ready(d) if !d.history.is_dirty() && !d.saving)
                })
                .min_by_key(|(_, s)| s.last_seen)
                .map(|(i, _)| i);
            let Some(idx) = candidate else {
                tracing::warn!(
                    "baraja: presupuesto de memoria excedido ({} ranuras cargadas) y ninguna \
                     se puede descartar (todas sucias, guardando, o cerca de la activa)",
                    loaded_count
                );
                break;
            };
            self.slots[idx].content = SlotContent::Idle;
            freed.push(FxScope(self.slots[idx].id));
        }
        freed
    }

    fn path_at(&self, idx: usize) -> Option<PathBuf> {
        self.slots.get(idx).map(|s| s.path.clone())
    }

    /// Ruta del lienzo siguiente, envolviendo. `None` con ≤1 lienzo.
    pub fn next_path(&self) -> Option<PathBuf> {
        if self.slots.len() <= 1 {
            return None;
        }
        self.path_at((self.active + 1) % self.slots.len())
    }

    /// Ruta del lienzo anterior, envolviendo. `None` con ≤1 lienzo.
    pub fn prev_path(&self) -> Option<PathBuf> {
        if self.slots.len() <= 1 {
            return None;
        }
        self.path_at((self.active + self.slots.len() - 1) % self.slots.len())
    }

    pub fn first_path(&self) -> Option<PathBuf> {
        if self.slots.len() <= 1 {
            return None;
        }
        self.path_at(0)
    }

    pub fn last_path(&self) -> Option<PathBuf> {
        if self.slots.len() <= 1 {
            return None;
        }
        self.path_at(self.slots.len() - 1)
    }
}

/// Cambia el lienzo activo: devuelve el actual a su ranura y saca el nuevo.
/// Función libre (no un método de `Deck`) a propósito: ninguno de los dos es
/// dueño del otro, y así queda claro en la firma. Solo se aplica si el
/// destino ya está `Ready` y el editor está ocioso (`EditorState::is_idle`,
/// sin gestos ni ediciones de panel a medias) — si no, la petición se deja
/// pendiente y se reintenta el siguiente frame; un arrastre termina al
/// soltar, así que la espera real es de uno o dos frames, invisible.
/// Devuelve `true` si el salto se aplicó.
pub fn apply_jump(deck: &mut Deck, state: &mut crate::editor::EditorState) -> bool {
    let Some(target) = deck.jump_to else {
        return false;
    };
    if target >= deck.slots.len() || target == deck.active {
        deck.jump_to = None;
        deck.jump_center = false;
        return false;
    }
    if !state.is_idle() {
        return false;
    }
    if let SlotContent::Failed(_) = &deck.slots[target].content {
        // No se reintenta sola: si se dejara `jump_to` puesto, `request_loads`
        // la seguiría priorizando cada frame sin ningún efecto — mejor
        // avisar una vez y soltar la petición.
        tracing::warn!(
            "baraja: se pidió saltar a «{}», que falló al cargar; se descarta el salto",
            deck.slots[target].name
        );
        deck.jump_to = None;
        deck.jump_center = false;
        return false;
    }
    if !matches!(deck.slots[target].content, SlotContent::Ready(_)) {
        return false;
    }
    let SlotContent::Ready(incoming) =
        std::mem::replace(&mut deck.slots[target].content, SlotContent::Active)
    else {
        unreachable!("comprobado justo arriba");
    };
    let outgoing = state.take_slot();
    deck.slots[deck.active].content = SlotContent::Ready(Box::new(outgoing));
    state.put_slot(*incoming);
    deck.active = target;
    deck.jump_to = None;
    true
}

fn slot_kind(path: &Path) -> ItemKind {
    if canvas_io::is_canvas_file(path) {
        ItemKind::Design
    } else {
        ItemKind::Image
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed(paths: &[&str]) -> DeckSeed {
        DeckSeed {
            folder: PathBuf::from("C:\\folder"),
            sort: GallerySort::Name,
            items: paths
                .iter()
                .map(|p| SeedItem {
                    path: PathBuf::from(p),
                    name: p.to_string(),
                    kind: ItemKind::Image,
                    mtime: None,
                    thumb: None,
                    thumb_failed: false,
                })
                .collect(),
        }
    }

    fn blank_slot_doc(w: f64, h: f64) -> SlotDoc {
        SlotDoc {
            doc: Document::new(w, h),
            history: History::default(),
            images: ImageMap::new(),
            selection: Selection::default(),
            background_layer: None,
            sidecar_enabled: true,
            is_design: false,
            source_metadata: None,
            saving: false,
            save_error: None,
            external_change: false,
            born_blank: false,
            bytes: 0,
        }
    }

    /// Como `blank_slot_doc`, pero con `history.is_dirty() == true`. El
    /// comando nunca se aplica de verdad (`push_applied` no llama a
    /// `apply`), así que da igual que `layer` no exista en `doc`.
    fn dirty_slot_doc(w: f64, h: f64) -> SlotDoc {
        let mut doc = blank_slot_doc(w, h);
        doc.history.push_applied(Box::new(canvas_core::Rename {
            layer: LayerId::from_raw(1),
            before: "a".to_string(),
            after: "b".to_string(),
        }));
        doc
    }

    #[test]
    fn single_is_degenerate_and_active() {
        let deck = Deck::single(PathBuf::from("a.png"));
        assert_eq!(deck.slots.len(), 1);
        assert!(!deck.is_visible());
        assert!(matches!(deck.slots[0].content, SlotContent::Active));
        assert_eq!(deck.next_path(), None);
        assert_eq!(deck.prev_path(), None);
    }

    #[test]
    fn from_seed_activates_matching_path() {
        let deck = Deck::from_seed(seed(&["a.png", "b.png", "c.png"]), Path::new("b.png"));
        assert_eq!(deck.active, 1);
        assert!(deck.is_visible());
        assert!(matches!(deck.slots[1].content, SlotContent::Active));
    }

    #[test]
    fn next_and_prev_wrap() {
        let mut deck = Deck::from_seed(seed(&["a.png", "b.png", "c.png"]), Path::new("a.png"));
        assert_eq!(deck.next_path(), Some(PathBuf::from("b.png")));
        deck.active = 2;
        assert_eq!(deck.next_path(), Some(PathBuf::from("a.png")));
        deck.active = 0;
        assert_eq!(deck.prev_path(), Some(PathBuf::from("c.png")));
    }

    #[test]
    fn first_and_last() {
        let deck = Deck::from_seed(seed(&["a.png", "b.png", "c.png"]), Path::new("b.png"));
        assert_eq!(deck.first_path(), Some(PathBuf::from("a.png")));
        assert_eq!(deck.last_path(), Some(PathBuf::from("c.png")));
    }

    #[test]
    fn merge_scan_preserves_id_and_thumb_by_path() {
        let mut deck = Deck::from_seed(seed(&["a.png", "b.png"]), Path::new("a.png"));
        let id_a = deck.slots[0].id;
        deck.merge_scan(vec![
            (PathBuf::from("b.png"), None),
            (PathBuf::from("a.png"), None),
            (PathBuf::from("c.png"), None),
        ]);
        assert_eq!(deck.slots.len(), 3);
        let a = deck
            .slots
            .iter()
            .find(|s| s.path.ends_with("a.png"))
            .unwrap();
        assert_eq!(a.id, id_a);
        assert_eq!(deck.active_path(), Some(PathBuf::from("a.png")));
    }

    #[test]
    fn merge_scan_drops_vanished_files() {
        let mut deck = Deck::from_seed(seed(&["a.png", "b.png"]), Path::new("a.png"));
        deck.merge_scan(vec![(PathBuf::from("a.png"), None)]);
        assert_eq!(deck.slots.len(), 1);
    }

    #[test]
    fn merge_scan_keeps_a_dirty_slot_even_if_its_file_vanished() {
        let mut deck = Deck::from_seed(seed(&["a.png", "b.png"]), Path::new("a.png"));
        deck.slots[1].content = SlotContent::Ready(Box::new(dirty_slot_doc(10.0, 10.0)));
        // b.png desaparece del disco en el reescaneo.
        deck.merge_scan(vec![(PathBuf::from("a.png"), None)]);
        assert_eq!(deck.slots.len(), 2, "la ranura sucia no debe perderse");
    }

    #[test]
    fn move_slot_swaps_two_neighbors_and_switches_to_manual_sort() {
        let mut deck = Deck::from_seed(seed(&["a.png", "b.png", "c.png"]), Path::new("a.png"));
        let id_b = deck.slots[1].id;
        assert!(deck.move_slot(id_b, MoveDir::Prev));
        assert_eq!(deck.sort, GallerySort::Manual);
        let names: Vec<&str> = deck.slots.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["b.png", "a.png", "c.png"]);
    }

    #[test]
    fn move_slot_keeps_the_active_slot_active_across_the_swap() {
        let mut deck = Deck::from_seed(seed(&["a.png", "b.png", "c.png"]), Path::new("a.png"));
        let id_a = deck.slots[0].id;
        assert!(deck.move_slot(id_a, MoveDir::Next));
        // "a" ahora está en la posición 1, pero sigue siendo la activa.
        assert_eq!(deck.slots[deck.active].id, id_a);
    }

    #[test]
    fn move_slot_fails_at_either_end() {
        let mut deck = Deck::from_seed(seed(&["a.png", "b.png"]), Path::new("a.png"));
        let id_a = deck.slots[0].id;
        let id_b = deck.slots[1].id;
        assert!(!deck.move_slot(id_a, MoveDir::Prev), "ya es la primera");
        assert!(!deck.move_slot(id_b, MoveDir::Next), "ya es la última");
    }

    #[test]
    fn order_hint_survives_a_rescan_that_also_adds_a_new_file() {
        let mut deck = Deck::from_seed(seed(&["a.png", "b.png", "c.png"]), Path::new("a.png"));
        let id_c = deck.slots[2].id;
        // Manda "c.png" al principio.
        assert!(deck.move_slot(id_c, MoveDir::Prev));
        assert!(deck.move_slot(id_c, MoveDir::Prev));
        assert_eq!(deck.slots[0].id, id_c);
        // Reescaneo que además trae un archivo nuevo.
        deck.merge_scan(vec![
            (PathBuf::from("a.png"), None),
            (PathBuf::from("b.png"), None),
            (PathBuf::from("c.png"), None),
            (PathBuf::from("d.png"), None),
        ]);
        assert_eq!(
            deck.sort,
            GallerySort::Manual,
            "el reescaneo no reinicia el orden"
        );
        let names: Vec<&str> = deck.slots.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["c.png", "a.png", "b.png", "d.png"],
            "el orden manual sobrevive; la ranura nueva aterriza al final"
        );
    }

    #[test]
    fn relayout_single_slot_starts_at_origin() {
        let mut deck = Deck::single(PathBuf::from("a.png"));
        deck.slots[0].page = Some((800.0, 600.0));
        deck.relayout();
        assert_eq!(
            deck.slots[0].rect,
            DeckRect {
                x: 0.0,
                y: 0.0,
                w: 800.0,
                h: 600.0
            }
        );
    }

    #[test]
    fn relayout_stacks_vertically_and_centers_narrower_slots() {
        let mut deck = Deck::from_seed(seed(&["a.png", "b.png"]), Path::new("a.png"));
        deck.slots[0].page = Some((800.0, 600.0));
        deck.slots[1].page = Some((400.0, 300.0));
        deck.relayout();
        let a = deck.slots[0].rect;
        let b = deck.slots[1].rect;
        assert_eq!(a.x, 0.0);
        assert_eq!(a.y, 0.0);
        // b es más estrecho: centrado dentro del ancho de la pila (800).
        assert_eq!(b.x, (800.0 - 400.0) / 2.0);
        // b empieza justo debajo de a más el hueco.
        assert!(b.y > a.y + a.h);
    }

    #[test]
    fn relayout_stacks_horizontally_and_centers_shorter_slots() {
        let mut deck = Deck::from_seed(seed(&["a.png", "b.png"]), Path::new("a.png"));
        deck.axis = DeckAxis::Horizontal;
        deck.slots[0].page = Some((800.0, 600.0));
        deck.slots[1].page = Some((400.0, 300.0));
        deck.relayout();
        let a = deck.slots[0].rect;
        let b = deck.slots[1].rect;
        assert_eq!(a.x, 0.0);
        assert_eq!(a.y, 0.0);
        // b es más bajo: centrado dentro del alto de la pila (600).
        assert_eq!(b.y, (600.0 - 300.0) / 2.0);
        // b empieza justo a la derecha de a más el hueco.
        assert!(b.x > a.x + a.w);
    }

    #[test]
    fn bounds_spans_all_slots_on_either_axis() {
        let mut deck = Deck::from_seed(seed(&["a.png", "b.png"]), Path::new("a.png"));
        deck.axis = DeckAxis::Horizontal;
        deck.slots[0].page = Some((800.0, 600.0));
        deck.slots[1].page = Some((400.0, 300.0));
        deck.relayout();
        let bounds = deck.bounds();
        // Horizontal: el ancho recorre las dos ranuras + hueco; el alto es
        // el más alto de los dos (el más bajo queda centrado dentro).
        assert!(bounds.w > 800.0 + 400.0);
        assert_eq!(bounds.h, 600.0);
    }

    #[test]
    fn visible_indices_finds_only_intersecting_slots() {
        let mut deck = Deck::from_seed(seed(&["a.png", "b.png", "c.png"]), Path::new("a.png"));
        deck.slots[0].page = Some((100.0, 100.0));
        deck.slots[1].page = Some((100.0, 100.0));
        deck.slots[2].page = Some((100.0, 100.0));
        deck.relayout();
        // Solo cabe el primero en una ventana pequeña arriba del todo.
        let view = DeckRect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 50.0,
        };
        assert_eq!(deck.visible_indices(view), vec![0]);
    }

    #[test]
    fn apply_jump_swaps_content_and_preserves_dirty_flag() {
        let mut deck = Deck::from_seed(seed(&["a.png", "b.png"]), Path::new("a.png"));
        deck.slots[1].content = SlotContent::Ready(Box::new(blank_slot_doc(50.0, 50.0)));
        let mut state = crate::editor::EditorState::new_blank(20.0, 20.0);
        deck.jump_to = Some(1);
        assert!(apply_jump(&mut deck, &mut state));
        assert_eq!(deck.active, 1);
        assert_eq!(state.doc.page().unwrap().width, 50.0);
        assert!(matches!(deck.slots[1].content, SlotContent::Active));
        assert!(matches!(deck.slots[0].content, SlotContent::Ready(_)));
    }

    #[test]
    fn evict_skips_dirty_and_nearby_slots_but_takes_clean_far_ones() {
        let mut deck = Deck::from_seed(
            seed(&["a.png", "b.png", "c.png", "d.png", "e.png", "f.png"]),
            Path::new("a.png"),
        );
        // Activo = 0, radio de precarga = 2 ⇒ 0..=2 están protegidos; 3..=5
        // son candidatas. Todas pesan de sobra para forzar el descarte.
        for i in 3..6 {
            let mut doc = blank_slot_doc(10.0, 10.0);
            doc.bytes = EVICT_BUDGET_BYTES;
            deck.slots[i].content = SlotContent::Ready(Box::new(doc));
        }
        deck.slots[3].content = SlotContent::Ready(Box::new(dirty_slot_doc(10.0, 10.0)));
        if let SlotContent::Ready(d) = &mut deck.slots[3].content {
            d.bytes = EVICT_BUDGET_BYTES;
        }

        let freed = deck.evict();

        assert_eq!(freed.len(), 2, "solo las dos limpias, la sucia sobrevive");
        assert!(
            matches!(deck.slots[3].content, SlotContent::Ready(_)),
            "una ranura sucia nunca se descarta"
        );
        assert!(matches!(deck.slots[4].content, SlotContent::Idle));
        assert!(matches!(deck.slots[5].content, SlotContent::Idle));
    }

    #[test]
    fn request_loads_prioritises_a_pending_jump_outside_the_preload_radius() {
        // 8 ranuras, activa = 0 (radio de precarga 2 ⇒ 0..=2 cubiertas
        // aparte). Sin `jump_to`, la ranura 7 nunca se pediría por sí sola.
        let mut deck = Deck::from_seed(
            seed(&[
                "a.png", "b.png", "c.png", "d.png", "e.png", "f.png", "g.png", "h.png",
            ]),
            Path::new("a.png"),
        );
        deck.jump_to = Some(7);
        let spawned = deck.request_loads(&[]);
        assert_eq!(
            spawned.first(),
            Some(&PathBuf::from("h.png")),
            "el destino del salto pendiente debe cargarse primero, aunque esté lejos"
        );
        assert!(matches!(deck.slots[7].content, SlotContent::Loading));
    }

    #[test]
    fn request_loads_ignores_a_jump_that_is_not_idle() {
        let mut deck = Deck::from_seed(seed(&["a.png", "b.png", "c.png"]), Path::new("a.png"));
        deck.slots[2].content = SlotContent::Ready(Box::new(blank_slot_doc(10.0, 10.0)));
        deck.jump_to = Some(2);
        let spawned = deck.request_loads(&[]);
        assert!(
            !spawned.contains(&PathBuf::from("c.png")),
            "una ranura ya Ready no debe volver a pedirse"
        );
    }

    #[test]
    fn request_loads_ignores_an_out_of_range_jump() {
        let mut deck = Deck::from_seed(seed(&["a.png", "b.png"]), Path::new("a.png"));
        deck.jump_to = Some(99);
        // No debe entrar en pánico; el resto del comportamiento normal sigue.
        let spawned = deck.request_loads(&[]);
        assert!(spawned.contains(&PathBuf::from("b.png")));
    }

    #[test]
    fn push_placeholder_appends_a_clean_ready_slot_at_the_end() {
        let mut deck = Deck::from_seed(seed(&["a.png", "b.png"]), Path::new("a.png"));
        let idx = deck
            .push_placeholder((800.0, 600.0), "canvas")
            .expect("con carpeta");
        assert_eq!(idx, 2);
        let slot = &deck.slots[idx];
        assert!(slot.is_placeholder);
        assert_eq!(slot.page, Some((800.0, 600.0)));
        assert!(slot.kind == ItemKind::Design);
        match &slot.content {
            SlotContent::Ready(d) => assert!(
                !d.history.is_dirty(),
                "una provisional recién creada debe leerse como limpia, \
                 o se materializaría sola sin que nadie la editase"
            ),
            _ => panic!("se esperaba SlotContent::Ready"),
        }
    }

    #[test]
    fn push_placeholder_png_makes_a_real_image_slot() {
        let mut deck = Deck::from_seed(seed(&["a.png", "b.png"]), Path::new("a.png"));
        let idx = deck
            .push_placeholder((800.0, 600.0), "png")
            .expect("con carpeta");
        let slot = &deck.slots[idx];
        assert_eq!(slot.kind, ItemKind::Image);
        assert!(slot.path.extension().is_some_and(|e| e == "png"));
        match &slot.content {
            SlotContent::Ready(d) => {
                assert!(!d.is_design, "un raster nuevo no es un diseño autónomo");
                assert!(
                    d.sidecar_enabled,
                    "sin sidecar, un raster en blanco perdería sus capas al guardar"
                );
            }
            _ => panic!("se esperaba SlotContent::Ready"),
        }
    }

    #[test]
    fn push_placeholder_never_makes_a_second_one() {
        let mut deck = Deck::from_seed(seed(&["a.png"]), Path::new("a.png"));
        let first = deck.push_placeholder((800.0, 600.0), "canvas");
        let second = deck.push_placeholder((800.0, 600.0), "canvas");
        assert_eq!(first, second);
        assert_eq!(deck.slots.len(), 2);
    }

    #[test]
    fn push_placeholder_needs_a_folder() {
        let mut deck = Deck::single(PathBuf::from("a.png"));
        assert_eq!(deck.push_placeholder((800.0, 600.0), "canvas"), None);
        assert_eq!(deck.slots.len(), 1);
    }

    #[test]
    fn merge_scan_keeps_the_placeholder_and_leaves_it_last() {
        let mut deck = Deck::from_seed(seed(&["a.png", "b.png"]), Path::new("a.png"));
        let placeholder_idx = deck.push_placeholder((800.0, 600.0), "canvas").unwrap();
        let placeholder_id = deck.slots[placeholder_idx].id;
        // Reescaneo que reordena y añade un archivo nuevo: la provisional no
        // aparece en ningún listado real.
        deck.merge_scan(vec![
            (PathBuf::from("c.png"), None),
            (PathBuf::from("b.png"), None),
            (PathBuf::from("a.png"), None),
        ]);
        assert_eq!(deck.slots.len(), 4, "la provisional sobrevive al reescaneo");
        let last = deck.slots.last().unwrap();
        assert!(last.is_placeholder);
        assert_eq!(last.id, placeholder_id);
    }

    #[test]
    fn merge_scan_follows_an_active_placeholder() {
        let mut deck = Deck::from_seed(seed(&["a.png", "b.png"]), Path::new("a.png"));
        let placeholder_idx = deck.push_placeholder((800.0, 600.0), "canvas").unwrap();
        // Simula que la provisional pasó a ser la activa (como haría
        // `apply_jump` al saltar a ella).
        deck.slots[deck.active].content = SlotContent::Ready(Box::new(blank_slot_doc(1.0, 1.0)));
        deck.active = placeholder_idx;
        deck.slots[placeholder_idx].content = SlotContent::Active;
        deck.merge_scan(vec![
            (PathBuf::from("c.png"), None),
            (PathBuf::from("b.png"), None),
            (PathBuf::from("a.png"), None),
        ]);
        assert!(
            deck.slots[deck.active].is_placeholder,
            "el índice activo debe seguir apuntando a la provisional tras el reordenado"
        );
    }

    #[test]
    fn evict_never_discards_a_placeholder() {
        let mut deck = Deck::from_seed(
            seed(&["a.png", "b.png", "c.png", "d.png", "e.png", "f.png"]),
            Path::new("a.png"),
        );
        let placeholder_idx = deck.push_placeholder((10.0, 10.0), "canvas").unwrap();
        // Otra ranura limpia y lejana, también sobre el presupuesto: debe
        // descartarse ella y no la provisional.
        let mut doc = blank_slot_doc(10.0, 10.0);
        // Estrictamente por encima del presupuesto (no `==`): el
        // presupuesto total incluye los 0 bytes de la provisional, así que
        // igualarlo exactamente nunca dispara la condición de descarte.
        doc.bytes = EVICT_BUDGET_BYTES + 1;
        deck.slots[5].content = SlotContent::Ready(Box::new(doc));

        let freed = deck.evict();

        assert_eq!(
            freed.len(),
            1,
            "solo la ranura 5 se descarta, no la provisional"
        );
        assert!(matches!(deck.slots[5].content, SlotContent::Idle));
        assert!(
            matches!(deck.slots[placeholder_idx].content, SlotContent::Ready(_)),
            "una provisional nunca se descarta, aunque esté lejos y limpia"
        );
    }

    #[test]
    fn request_loads_never_asks_for_a_placeholder() {
        let mut deck = Deck::from_seed(seed(&["a.png"]), Path::new("a.png"));
        let placeholder_idx = deck.push_placeholder((10.0, 10.0), "canvas").unwrap();
        // Forzar `Idle` a mano: no debería llegar a pasar en la práctica
        // (evict la protege), pero `request_loads` tiene que ser robusta
        // igualmente.
        let placeholder_path = deck.slots[placeholder_idx].path.clone();
        deck.slots[placeholder_idx].content = SlotContent::Idle;
        let spawned = deck.request_loads(&[]);
        assert!(!spawned.contains(&placeholder_path));
    }
}
