//! Ajustes persistidos del usuario: un JSON pequeño en el directorio de
//! configuración de la plataforma. Se cargan una vez al arrancar y se
//! escriben en un hilo aparte cada vez que cambian.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::deck::{DeckAxis, StripSide};

mod appearance;
mod choices;
mod sort;
mod writer;

pub use appearance::Density;
pub use choices::{BulkCanvasSize, GallerySort, NewCanvasFormat, ThemeChoice};
pub use sort::natural_cmp;
pub(crate) use writer::flush_settings;

#[cfg(test)]
mod tests;
/// Filtro de la galería / baraja: qué tipos de archivo mostrar.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, Debug)]
pub enum MediaFilter {
    #[default]
    All,
    ImagesOnly,
    VideosOnly,
}

impl MediaFilter {
    pub fn label(self) -> &'static str {
        match self {
            MediaFilter::All => "All",
            MediaFilter::ImagesOnly => "Images",
            MediaFilter::VideosOnly => "Videos",
        }
    }
}

/// Orden de las pestañas del panel izquierdo del editor (Page/Layers): el
/// usuario las arrastra para reordenarlas y el orden queda guardado aquí.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, Debug)]
pub enum LayersTabOrder {
    /// «Page» arriba (por defecto).
    #[default]
    PageFirst,
    /// «Layers» arriba.
    LayersFirst,
}

impl LayersTabOrder {
    /// El orden invertido (con dos pestañas, cualquier cruce de un arrastre
    /// produce exactamente esto).
    pub fn swapped(self) -> Self {
        match self {
            LayersTabOrder::PageFirst => LayersTabOrder::LayersFirst,
            LayersTabOrder::LayersFirst => LayersTabOrder::PageFirst,
        }
    }
}

/// Bloqueados iniciales del buscador web: los del filtro de Serper.
fn default_serper_blocked() -> Vec<String> {
    crate::serper::default_blocked_domains()
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    /// Calidad de recompresión al guardar JPEG (1–100).
    pub jpeg_quality: u8,
    /// «Don't ask again» del aviso de sobrescritura destructiva.
    pub skip_overwrite_warning: bool,
    /// Valor por defecto del checkbox «Editable sidecar (.canvas)».
    pub sidecar_default: bool,
    /// Orden de la galería de carpetas.
    pub gallery_sort: GallerySort,
    /// Lado donde se ancla el navegador de carpetas de Gallery.
    pub gallery_folder_panel_side: StripSide,
    /// Archivos y carpetas abiertos recientemente (el más nuevo primero).
    pub recent_files: Vec<PathBuf>,
    /// Carpetas ancladas: siempre aparecen al principio de la lista de
    /// recientes aunque no se hayan abierto hace poco.
    pub pinned_folders: Vec<PathBuf>,
    /// Tema de la interfaz.
    pub theme: ThemeChoice,
    pub density: Density,
    pub ui_scale: f32,
    pub reduced_motion: bool,
    pub language: crate::i18n::Language,
    /// Tamaño de página del último documento abierto o creado: lo hereda el
    /// siguiente diseño nuevo (galería, Ctrl+N o bienvenida).
    pub last_page_size: (f64, f64),
    /// Eje de apilado de la baraja del editor (Fase 14e): con qué eje se
    /// abre la próxima carpeta, hasta que el usuario lo cambie otra vez.
    pub deck_axis: DeckAxis,
    /// La tira de miniaturas de la baraja está visible por defecto.
    pub deck_strip_visible: bool,
    /// Lado de la ventana donde se ancla la tira de la baraja. Independiente
    /// de `deck_axis` — ver la doc de `StripSide`.
    pub deck_strip_side: StripSide,
    /// Formato en el que nace un lienzo en blanco nuevo. PNG por defecto: un
    /// raster real y visible, no el diseño autónomo `.canvas` de antes.
    pub new_canvas_format: NewCanvasFormat,
    /// Panel de capas colapsado en una pestaña fina al borde izquierdo.
    pub layers_collapsed: bool,
    /// Orden de las pestañas del panel izquierdo (arrastrables con el ratón).
    pub layers_tab_order: LayersTabOrder,
    /// Dominios bloqueados del buscador web (pestaña «Web»): filtro local y
    /// exclusiones `-site:` de la query. Editable en el Advanced del panel.
    #[serde(default = "default_serper_blocked")]
    pub serper_blocked: Vec<String>,
    /// Presupuesto de tokens por keyword del buscador web (1–3).
    pub serper_budget: crate::serper::TokenBudget,
    /// Créditos Serper gastados en total (histórico persistido; la sesión
    /// lleva su propio contador en el panel).
    pub serper_credits_total: u64,
    /// Última carpeta destino del bulk web (para no preguntar dos veces).
    pub serper_last_folder: Option<PathBuf>,
    /// Tamaño de página de cada lienzo creado por el bulk web.
    pub serper_bulk_size: BulkCanvasSize,
    /// Última carpeta destino del Download de vídeo.
    pub ytdlp_last_folder: Option<PathBuf>,
    /// Tamaño de lienzo recordado de Editar vídeo (defecto Full HD).
    pub ytdlp_canvas_size: (f64, f64),
    pub media_filter: MediaFilter,
    /// Workspaces abiertos en la última sesión, para restaurarlos al
    /// arrancar. El orden es el de creación (la ventana 0 es la raíz). Se
    /// vuelve a escribir cada vez que un workspace se abre o se cierra, y al
    /// cerrar la app (`App::on_exit`).
    pub workspaces: Vec<StoredWorkspace>,
}

/// Un workspace tal y como queda en `settings.json` para restaurarlo en la
/// siguiente sesión: qué documento (o `None` = bienvenida) estaba activo y
/// la última geometría conocida de su ventana (en puntos lógicos, los
/// mismos que usa egui). Solo se restaura el documento ACTIVO — la baraja
/// de hermanos de la carpeta no viaja aquí.
#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
pub struct StoredWorkspace {
    /// Documento abierto (imagen/diseño/carpeta) o `None` para la
    /// bienvenida.
    pub path: Option<PathBuf>,
    /// Esquina superior izquierda de la ventana, en puntos. `None` = no se
    /// conoce (el SO decide).
    pub pos: Option<[f32; 2]>,
    /// Tamaño interior de la ventana, en puntos. `None` = por defecto.
    pub size: Option<[f32; 2]>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            jpeg_quality: 92,
            skip_overwrite_warning: false,
            sidecar_default: true,
            gallery_sort: GallerySort::default(),
            gallery_folder_panel_side: StripSide::default(),
            recent_files: Vec::new(),
            pinned_folders: Vec::new(),
            theme: ThemeChoice::default(),
            density: Density::default(),
            ui_scale: 1.0,
            reduced_motion: false,
            language: crate::i18n::Language::default(),
            last_page_size: (1920.0, 1080.0),
            deck_axis: DeckAxis::default(),
            deck_strip_visible: true,
            deck_strip_side: StripSide::default(),
            new_canvas_format: NewCanvasFormat::default(),
            layers_collapsed: false,
            layers_tab_order: LayersTabOrder::default(),
            serper_blocked: default_serper_blocked(),
            serper_budget: crate::serper::TokenBudget::default(),
            serper_credits_total: 0,
            serper_last_folder: None,
            serper_bulk_size: BulkCanvasSize::default(),
            ytdlp_last_folder: None,
            ytdlp_canvas_size: (1920.0, 1080.0),
            media_filter: MediaFilter::default(),
            workspaces: Vec::new(),
        }
    }
}

impl AppSettings {
    pub(super) fn settings_path() -> Option<PathBuf> {
        let dirs = directories::ProjectDirs::from("com", "canvas-desktop", "Canvas Desktop")?;
        Some(dirs.config_dir().join("settings.json"))
    }

    fn file_path() -> Option<PathBuf> {
        Self::settings_path()
    }

    /// Carga los ajustes. Cualquier problema (primera ejecución, JSON roto)
    /// devuelve los valores por defecto sin molestar al usuario.
    pub fn load() -> Self {
        let Some(path) = Self::file_path() else {
            return Self::default();
        };
        match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
                tracing::warn!("settings.json ilegible ({e}); valores por defecto");
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    /// Encola los ajustes para escribirlos en segundo plano (la UI nunca
    /// espera al disco). Un único hilo escritor con revisiones monotónicas
    /// garantiza el orden: un snapshot viejo nunca sobrescribe a uno nuevo
    /// (A10, ver `writer`).
    pub fn save_in_background(&self) {
        writer::queue_settings(self.clone());
    }
}

/// Acción pedida desde la ventana de ajustes que la app debe ejecutar (en un
/// hilo aparte: toca el registro del sistema).
pub enum SettingsAction {
    RegisterShell,
    UnregisterShell,
}

mod ui;
pub use ui::settings_window;
