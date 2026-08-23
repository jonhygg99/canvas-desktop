//! El contexto que necesita la vista del editor durante un frame.
//!
//! `editor_view_ui` se llama mientras `state` sigue prestado de `self.view`
//! (dentro de `match &mut self.view { View::Editor(state) => … }`), asi que no
//! puede recibir `&mut App`. Antes recibia los 25 campos sueltos, uno por
//! parametro; `EditorFrame` los agrupa sin perder nada: siguen siendo
//! prestamos independientes de campos DISTINTOS de `App`, asi que el
//! comprobador de prestamos los deja usar a la vez, cosa que un `&mut self`
//! no permitiria.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::time::Instant;

use canvas_render::CanvasRenderer;

use crate::loader::AppMsg;
use crate::surface::CanvasSurface;
use crate::{deck, export, settings, watcher};

use super::Nav;

pub(super) struct EditorFrame<'a> {
    pub(super) deck: &'a mut deck::Deck,
    pub(super) renderer: &'a mut CanvasRenderer,
    pub(super) surface: &'a mut Option<CanvasSurface>,
    pub(super) tx: &'a Sender<AppMsg>,
    pub(super) settings: &'a mut settings::AppSettings,
    pub(super) show_settings: &'a mut bool,
    pub(super) save_requested: &'a mut bool,
    pub(super) close_after_save: &'a mut bool,
    pub(super) after_save: &'a mut Option<Nav>,
    pub(super) allow_close: &'a mut bool,
    pub(super) overwrite_confirmed: &'a mut bool,
    pub(super) overwrite_prompt: &'a mut Option<PathBuf>,
    pub(super) overwrite_dont_ask: &'a mut bool,
    pub(super) readonly_prompt: &'a mut Option<PathBuf>,
    pub(super) export_dialog: &'a mut Option<export::ExportDialog>,
    pub(super) pending_export_settings: &'a mut Option<export::ExportSettings>,
    pub(super) pending_export: &'a mut Option<(PathBuf, export::ExportSettings)>,
    pub(super) pending_save_as: &'a mut Option<PathBuf>,
    pub(super) ignore_fs_events_until: &'a mut Option<Instant>,
    pub(super) watcher: &'a mut Option<watcher::DocWatcher>,
    pub(super) undoable_deletes: &'a mut HashMap<PathBuf, Option<PathBuf>>,
    pub(super) materializing: &'a mut Option<u64>,
    pub(super) materialize_blocked: &'a mut Option<u64>,
    pub(super) save_all_queue: &'a mut Vec<u64>,
    pub(super) save_all_attempted: &'a mut bool,
}
