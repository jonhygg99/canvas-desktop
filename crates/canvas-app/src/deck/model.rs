//! Los datos de una ranura: su contenido (por cargar, cargando, listo,
//! fallido), el documento que guarda cuando no es la ranura activa, y la
//! semilla con la que la galeria siembra una baraja nueva.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use canvas_core::{Document, History, LayerId, Selection};
use canvas_render::ImageMap;
use eframe::egui;

use crate::gallery::{GalleryState, ItemKind};
use crate::settings::GallerySort;

use super::geometry::DeckRect;
use super::loading::next_scope;

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
    /// Su creación todavía no se ha registrado en el deshacer global — ver
    /// `EditorState::pending_creation`.
    pub pending_creation: bool,
    /// Bytes aproximados en RAM (Σ ancho·alto·4 de `images`), para el
    /// presupuesto de descarte.
    pub bytes: usize,
}

/// Una ranura = un archivo de la carpeta = un lienzo.
pub struct Slot {
    /// Identidad estable durante toda la sesión, aunque la lista se reordene
    /// o el archivo se renombre.
    pub id: u64,
    /// `FxScope` con el que esta ranura habla con `CanvasRenderer`: único a
    /// nivel de PROCESO (ver `loading::next_scope`), porque el renderer y su
    /// caché de efectos son compartidos entre ventanas — dos ventanas con el
    /// mismo `Slot::id` (cada `Deck` empieza en 1) se pisarían las texturas
    /// procesadas si el scope derivara del id.
    pub scope: u64,
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
    pub(super) last_seen: u64,
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
    /// (`canvas_io::peek_numbered_path`) que aún nadie ha reservado. Se
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
    pub(super) fn size(&self) -> (f64, f64) {
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

/// Semilla que la galería entrega al editor al abrir un archivo desde ella:
/// rutas, nombres y las miniaturas YA subidas a GPU. `TextureHandle` es un
/// handle contado, clonarlo es gratis — evita re-decodificar y volver a
/// subir las miniaturas que la galería ya tenía cargadas.
#[derive(Clone)]
pub struct DeckSeed {
    pub folder: PathBuf,
    pub sort: GallerySort,
    pub items: Vec<SeedItem>,
}

#[derive(Clone)]
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

    /// Añade un page recién creado a la semilla y deja todos los demás
    /// archivos de la carpeta disponibles en la baraja del editor.
    pub fn push_path(&mut self, path: PathBuf, kind: ItemKind) {
        if !self.items.iter().any(|item| item.path == path) {
            self.items.push(SeedItem {
                name: path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                path,
                kind,
                mtime: None,
                thumb: None,
                thumb_failed: false,
            });
        }
    }
}

pub(super) fn idle_slot(
    id: u64,
    path: PathBuf,
    mtime: Option<SystemTime>,
    order_hint: u64,
) -> Slot {
    let kind = slot_kind(&path);
    let name = file_name(&path);
    Slot {
        id,
        scope: next_scope(),
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

pub(super) fn slot_kind(path: &Path) -> ItemKind {
    if canvas_io::is_canvas_file(path) {
        ItemKind::Design
    } else {
        ItemKind::Image
    }
}

pub(super) fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}
