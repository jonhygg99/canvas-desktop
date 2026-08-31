//! Un workspace: TODO el estado de una ventana nativa — su vista (bienvenida,
//! galería, editor), la baraja de lienzos del editor, el watcher de su
//! archivo, sus flujos de guardado/exportación y su textura de render.
//!
//! Los campos compartidos de la app (renderer wgpu, ajustes, menús) viven en
//! `AppInner`; lo que era propio de "la ventana" vive aquí. Cada workspace
//! tiene además su propio canal `tx`/`rx`: los hilos de disco a los que pide
//! trabajo responden por ese canal, así el mensaje llega ya "direccionado" al
//! workspace que lo pidió aunque se haya cerrado el que lo esperaba (se
//! descarta con gracia al no haber nadie drenando).

use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Instant;

use eframe::egui;

use crate::loader::AppMsg;
use crate::surface::CanvasSurface;
use crate::{deck, watcher};

use super::{DeckOps, ExportFlow, SaveFlow, View};

/// Un workspace = una ventana nativa. El índice 0 es la ventana raíz (la que
/// no se puede cerrar sin salir de la app); el resto son ventanas hijas
/// creadas con `show_viewport_deferred`.
pub(crate) struct Workspace {
    /// Vista actual de esta ventana.
    pub(crate) view: View,
    /// Baraja de lienzos del editor de esta ventana.
    pub(crate) deck: deck::Deck,
    /// Watcher `notify` del archivo abierto, si lo hay.
    pub(crate) watcher: Option<watcher::DocWatcher>,
    /// Ventana de gracia tras un guardado propio (eventos del watcher hasta
    /// este instante son nuestros y se descartan).
    pub(crate) ignore_fs_events_until: Option<Instant>,
    /// Estado del camino de guardado de esta ventana.
    pub(crate) save: SaveFlow,
    /// Estado del camino de exportación de esta ventana.
    pub(crate) export: ExportFlow,
    /// Contabilidad de la baraja que cruza con el disco.
    pub(crate) deck_ops: DeckOps,
    /// Textura offscreen donde vello pinta el lienzo de esta ventana.
    pub(crate) surface: Option<CanvasSurface>,
    /// Último título enviado a la ventana (para no reenviarlo cada frame).
    pub(crate) last_title: String,
    /// Ventana de ajustes visible en esta ventana.
    pub(crate) show_settings: bool,
    /// Ventana «About» visible en esta ventana.
    pub(crate) show_about: bool,
    /// Canal propio: los hilos de trabajo que esta ventana lanza responden
    /// aquí. `tx` se clona en cada `loader::spawn_*`.
    pub(crate) tx: Sender<AppMsg>,
    /// Extremo de recepción del canal propio; lo drena el frame raíz (y el
    /// de la propia ventana, si es hija) una vez por frame.
    pub(crate) rx: Receiver<AppMsg>,
    /// El usuario pidió cerrar esta ventana (X de la barra de título): el
    /// frame raíz la retira (deja de mostrarla) en cuanto este flag termina
    /// de aplicarse. La ventana 0 nunca se retira: cerrarla cierra la app.
    pub(crate) close_requested: bool,
    /// Diálogo «¿guardar los cambios?» EN VUELO: el modal corre en un hilo
    /// aparte y responde por `AppMsg::UnsavedDialogAnswer`. Guarda qué hay
    /// que decidir (cerrar la ventana o navegar); `None` si no hay ninguno.
    /// Un modal SINCRÓNICO dentro del pase de un viewport diferido congela
    /// todo el event loop multi-ventana — de ahí el hilo + canal.
    pub(crate) unsaved_dialog: Option<super::UnsavedDialog>,
    /// Identidad de la VENTANA NATIVA de este workspace. La raíz es
    /// `ViewportId::ROOT`; las hijas, un id derivado estable del id de
    /// workspace — `show_viewport_deferred` exige el MISO id cada frame.
    pub(crate) viewport: egui::ViewportId,
    /// Última geometría conocida de la ventana (esquina sup. izq. y tamaño,
    /// en puntos lógicos), capturada cada frame del `ViewportInfo`; se
    /// persiste al cerrar y al salir de la app para restaurarla.
    pub(crate) geometry: Option<(egui::Pos2, egui::Vec2)>,
}

/// Etiqueta del workspace para el conmutador y la persistencia: el nombre
/// del documento/carpeta activo, «Welcome» para una ventana nueva.
impl Workspace {
    pub(crate) fn new(viewport: egui::ViewportId) -> Self {
        let (tx, rx) = channel();
        Self {
            view: View::Welcome { error: None },
            deck: deck::Deck::default(),
            watcher: None,
            ignore_fs_events_until: None,
            save: SaveFlow::default(),
            export: ExportFlow::default(),
            deck_ops: DeckOps::default(),
            surface: None,
            last_title: String::new(),
            show_settings: false,
            show_about: false,
            tx,
            rx,
            close_requested: false,
            unsaved_dialog: None,
            viewport,
            geometry: None,
        }
    }

    /// Nombre corto para el conmutador y la persistencia.
    pub(crate) fn label(&self) -> String {
        match &self.view {
            View::Editor(state) => state.file_name(),
            View::Loading { path } => path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Loading…".to_owned()),
            View::Gallery(g) => g
                .folder
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| g.folder.display().to_string()),
            View::Welcome { .. } => "Welcome".to_owned(),
        }
    }

    /// Ruta persistible de esta ventana para restaurarla al arrancar: el
    /// documento activo del editor, la carpeta abierta en la galería, o
    /// `None` para la bienvenida.
    pub(crate) fn persisted_path(&self) -> Option<PathBuf> {
        match &self.view {
            View::Editor(state) => state.doc.source_path.clone(),
            View::Gallery(g) => Some(g.folder.clone()),
            View::Loading { path } => Some(path.clone()),
            View::Welcome { .. } => None,
        }
    }

    /// ¿Hay cambios sin guardar en esta ventana? El estado sucio del lienzo
    /// activo vive en `EditorState::history`; el resto, en las ranuras de la
    /// baraja.
    pub(crate) fn is_dirty(&self) -> bool {
        match &self.view {
            View::Editor(state) => state.is_dirty()
                || self.deck.slots.iter().any(
                    |s| matches!(&s.content, deck::SlotContent::Ready(d) if d.history.is_dirty()),
                ),
            _ => false,
        }
    }

    /// Nombres de los lienzos con cambios sin guardar de esta ventana — el
    /// activo primero, luego el resto de la baraja — para los diálogos de
    /// «cambios sin guardar».
    pub(crate) fn dirty_canvas_names(&self) -> Vec<String> {
        let View::Editor(state) = &self.view else {
            return Vec::new();
        };
        let mut names = Vec::new();
        if state.is_dirty() {
            names.push(state.file_name());
        }
        for slot in &self.deck.slots {
            if matches!(&slot.content, deck::SlotContent::Ready(d) if d.history.is_dirty()) {
                names.push(slot.name.clone());
            }
        }
        names
    }
}
