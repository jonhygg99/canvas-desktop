//! Menús de la aplicación.
//!
//! En Windows son menús nativos con `muda` enganchados al HWND de la ventana
//! (`Menu::init_for_hwnd`) y sondeados cada frame con
//! `MenuEvent::receiver().try_recv()`. Matiz importante: los aceleradores
//! NATIVOS de muda necesitarían `TranslateAcceleratorW` en el bucle de
//! mensajes, que eframe no expone — por eso los atajos de teclado los sigue
//! gestionando egui y el menú solo los muestra como texto decorativo.
//!
//! Fuera de Windows (muda exigiría GTK en Linux) el fallback es una barra de
//! menús egui con las mismas acciones.

use std::path::PathBuf;

/// Acción de menú, común a la implementación nativa y al fallback egui.
#[derive(Clone)]
pub enum MenuAction {
    NewDesign,
    OpenFile,
    OpenFolder,
    CloseProject,
    Save,
    SaveAs,
    /// Guarda todas las ranuras sucias de la baraja, no solo la activa.
    SaveAll,
    Export,
    OpenRecent(PathBuf),
    Quit,
    Undo,
    Redo,
    ZoomIn,
    ZoomOut,
    FitToWindow,
    ToggleGrid,
    ToggleRulers,
    /// Salta al siguiente/anterior lienzo de la baraja (equivale a
    /// `PageDown`/`PageUp`); no hace nada con un solo archivo abierto.
    NextCanvas,
    PrevCanvas,
    /// Muestra/oculta la tira lateral de miniaturas de la baraja.
    ToggleCanvasesPanel,
    /// Alterna el eje de apilado de la baraja (vertical/horizontal).
    ToggleCanvasesAxis,
    /// Mueve la tira de la baraja al siguiente lado de la ventana.
    CycleCanvasesSide,
    /// Añade un lienzo en blanco al final de la baraja (celda "+" de la
    /// tira, o aquí cuando la tira está oculta con un solo archivo).
    AddCanvas,
    FullScreen,
    Settings,
    About,
    Cut,
    Copy,
    Paste,
    Duplicate,
    Delete,
    SelectAll,
    Group,
    Ungroup,
}

#[cfg(windows)]
#[cfg(windows)]
mod native;

#[cfg(not(windows))]
mod fallback;

#[cfg(windows)]
pub use native::AppMenus;

#[cfg(not(windows))]
pub use fallback::{menu_bar_ui, AppMenus};
