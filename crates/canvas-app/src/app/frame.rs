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
    /// Ventana a la que pertenece este frame: identifica la foto de estado
    /// del informe de crash cuando hay varias ventanas abiertas.
    pub(super) viewport: eframe::egui::ViewportId,
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
    /// ¿Hay un diálogo de «cambios sin guardar» abierto en esta ventana? El
    /// autoguardado no escribe mientras el usuario decide: si elige
    /// «Discard», guardar por detrás convertiría esa decisión en una mentira.
    pub(super) unsaved_dialog_open: bool,
    /// Estado del camino de exportacion.
    pub(super) export: &'a mut ExportFlow,
    /// Contabilidad de la baraja que cruza con el disco.
    pub(super) deck_ops: &'a mut DeckOps,
    /// Carpeta de caché de miniaturas (la misma que usa el escaneo de la
    /// galería): la necesita el reintento de miniaturas fallidas de la baraja
    /// (ver `DeckOps::claim_failed_thumbs`).
    pub(super) thumb_cache: Option<&'a std::path::Path>,
}
