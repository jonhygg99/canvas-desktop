//! Estado del panel de descargas yt-dlp (pestaña «Download»).

use std::path::PathBuf;

#[derive(Default)]
pub struct Panel {
    /// URLs pegadas (una por línea o separadas por espacios).
    pub urls: String,
    /// Trim por tiempos: inicio/fin en `HH:MM:SS`, `MM:SS` o segundos.
    pub start: String,
    pub end: String,
    /// Descargar sin audio (solo video).
    pub mute: bool,
    /// Descarga en curso en un hilo de trabajo.
    pub downloading: bool,
    /// Progreso textual (`[2/5] 42% …`) para la UI.
    pub progress: String,
    /// Último error visible.
    pub error: Option<String>,
    /// Rutas descargadas en la última tanda (para mostrarlas).
    pub done: Vec<PathBuf>,
    /// Clip pendiente de insertar al canvas (lo pide el botón Insertar; lo
    /// lanza `layers_panel` con el canal a mano).
    pub pending_insert: Option<PathBuf>,
    /// Clip pendiente de editar: abre la ventana de edición.
    pub pending_edit: Option<PathBuf>,
    /// Sesión de edición abierta (`None` = ventana cerrada).
    pub edit: Option<super::edit::VideoEdit>,
}
