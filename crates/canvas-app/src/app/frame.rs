//! El contexto que necesita la vista del editor durante un frame.
//!
//! `editor_view_ui` se llama mientras `state` sigue prestado de `self.view`
//! (dentro de `match &mut self.view { View::Editor(state) => … }`), asi que no
//! puede recibir `&mut App`. Antes recibia los 25 campos sueltos, uno por
//! parametro; `EditorFrame` los agrupa sin perder nada: siguen siendo
//! prestamos independientes de campos DISTINTOS de `App`, asi que el
//! comprobador de prestamos los deja usar a la vez, cosa que un `&mut self`
//! no permitiria.

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
