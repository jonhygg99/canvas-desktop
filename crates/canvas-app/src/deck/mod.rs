//! La baraja: todos los archivos de una carpeta como lienzos apilados en una
//! sola superficie con scroll continuo, para poder saltar de uno a otro
//! (tira lateral, `PageUp`/`PageDown`/`Home`/`End`, clic en el propio lienzo)
//! sin volver a la galeria. Solo el lienzo ACTIVO vive en `EditorState`; los
//! demas viven aqui, cargados perezosamente y descartados cuando se alejan
//! demasiado de la vista.

use std::path::{Path, PathBuf};

use crate::settings::GallerySort;

mod cache;
mod geometry;
mod layout;
mod loading;
mod model;
mod nav;
mod scan;
mod system;

#[cfg(test)]
mod tests;

pub use geometry::{DeckAxis, DeckRect, MoveDir, StripSide};
pub use model::{DeckSeed, SeedItem, Slot, SlotContent, SlotDoc};
pub use nav::apply_jump;

use loading::{next_generation, next_scope};
use model::idle_slot;

/// RAM libre del sistema y umbral de «poca memoria», para que `App` avise
/// antes de un guardado masivo (`Save all`) — la caché ya los usa
/// internamente (ver `budget_under_free_ram`).
pub(crate) use system::{free_ram_bytes, FREE_RAM_REDUCTION_THRESHOLD_BYTES};

/// La baraja del editor: todos los archivos de `folder`, con el activo
/// marcado por índice. `folder` es `None` para un archivo abierto suelto
/// (arrastrar y soltar, CLI, recientes): baraja degenerada de una ranura.
pub struct Deck {
    pub folder: Option<PathBuf>,
    /// Identifica la instancia de baraja a la que pertenecen las cargas.
    /// Los resultados de generaciones anteriores se descartan.
    pub generation: u64,
    pub slots: Vec<Slot>,
    pub active: usize,
    pub sort: GallerySort,
    /// La tira lateral está visible (se oculta además si `slots.len() <= 1`).
    pub strip_visible: bool,
    /// Las barajas nacidas desde Gallery precargan todas sus páginas en
    /// segundo plano, no solo las visibles. Así `New design` comparte la
    /// misma disponibilidad de fondos que abrir una imagen desde Gallery.
    pub preload_all: bool,
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
    /// Si el salto pendiente, al aplicarse, debe REENCUADRAR el lienzo nuevo
    /// (ajustar el zoom a la ventana y centrarlo) en vez de dejarlo donde
    /// caiga. Hoy todos los caminos de salto (tira, clic directo, teclado,
    /// «añadir lienzo», Save all, deshacer/rehacer global) lo piden — un
    /// lienzo recién activado debe verse entero y centrado, mismo encuadre
    /// que `Ctrl+0` — pero el campo se mantiene explícito: quien fija
    /// `jump_to` fija también esto, no queda implícito.
    pub jump_reframe: bool,
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
            generation: 0,
            slots: Vec::new(),
            active: 0,
            sort: GallerySort::default(),
            strip_visible: false,
            preload_all: false,
            axis: DeckAxis::default(),
            strip_side: StripSide::default(),
            jump_to: None,
            jump_reframe: false,
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
            generation: next_generation(),
            sort: seed.sort,
            strip_visible: true,
            preload_all: true,
            ..Self::default()
        };
        for item in seed.items {
            let id = deck.next_id;
            deck.next_id += 1;
            let order_hint = deck.next_order;
            deck.next_order += 1;
            deck.slots.push(Slot {
                id,
                scope: next_scope(),
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
}
