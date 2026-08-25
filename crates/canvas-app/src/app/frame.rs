//! Contextos de préstamo de un frame: `EditorFrame` agrupa los préstamos
//! disjuntos de `AppInner` y del `Workspace` que la vista del editor necesita
//! `&mut` a la vez (se construye en `ws_frame`, donde ambos están en scope).
//!
//! Es el MISMO patrón que antes del refactor: la vista se llama mientras
//! `ws.view` sigue prestado, así que recibe cada campo por separado en vez
//! de `&mut Workspace` entero.

use std::sync::mpsc::Sender;
use std::time::Instant;

use canvas_render::CanvasRenderer;

use crate::loader::AppMsg;
use crate::surface::CanvasSurface;
use crate::{deck, settings, watcher};

use super::{DeckOps, ExportFlow, SaveFlow};

pub(super) struct EditorFrame<'a> {
    pub(super) deck: &'a mut deck::Deck,
    pub(super) renderer: &'a mut CanvasRenderer,
    pub(super) surface: &'a mut Option<CanvasSurface>,
    pub(super) tx: &'a Sender<AppMsg>,
    pub(super) settings: &'a mut settings::AppSettings,
    pub(super) show_settings: &'a mut bool,
    pub(super) watcher: &'a mut Option<watcher::DocWatcher>,
    pub(super) ignore_fs_events_until: &'a mut Option<Instant>,
    /// Estado del camino de guardado.
    pub(super) save: &'a mut SaveFlow,
    /// Estado del camino de exportacion.
    pub(super) export: &'a mut ExportFlow,
    /// Contabilidad de la baraja que cruza con el disco.
    pub(super) deck_ops: &'a mut DeckOps,
}
