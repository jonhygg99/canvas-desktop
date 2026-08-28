//! Ajustes persistidos del usuario: un JSON pequeño en el directorio de
//! configuración de la plataforma. Se cargan una vez al arrancar y se
//! escriben en un hilo aparte cada vez que cambian.

use std::path::PathBuf;

use eframe::egui;
use serde::{Deserialize, Serialize};

use crate::deck::{DeckAxis, StripSide};

mod choices;
mod sort;

pub use choices::{GallerySort, NewCanvasFormat, ThemeChoice};
pub use sort::natural_cmp;

#[cfg(test)]
mod tests;
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
            last_page_size: (1920.0, 1080.0),
            deck_axis: DeckAxis::default(),
            deck_strip_visible: true,
            deck_strip_side: StripSide::default(),
            new_canvas_format: NewCanvasFormat::default(),
            layers_collapsed: false,
            layers_tab_order: LayersTabOrder::default(),
            workspaces: Vec::new(),
        }
    }
}

impl AppSettings {
    fn file_path() -> Option<PathBuf> {
        let dirs = directories::ProjectDirs::from("com", "canvas-desktop", "Canvas Desktop")?;
        Some(dirs.config_dir().join("settings.json"))
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

    /// Escribe los ajustes en un hilo aparte: la UI nunca espera al disco.
    pub fn save_in_background(&self) {
        let snapshot = self.clone();
        std::thread::spawn(move || {
            let Some(path) = Self::file_path() else {
                return;
            };
            if let Some(dir) = path.parent() {
                if let Err(e) = std::fs::create_dir_all(dir) {
                    tracing::warn!("no se pudo crear el directorio de ajustes: {e}");
                    return;
                }
            }
            match serde_json::to_vec_pretty(&snapshot) {
                Ok(bytes) => {
                    if let Err(e) = canvas_io::write_atomic(&path, &bytes) {
                        tracing::warn!("no se pudieron guardar los ajustes: {e}");
                    }
                }
                Err(e) => tracing::warn!("no se pudieron serializar los ajustes: {e}"),
            }
        });
    }
}

/// Acción pedida desde la ventana de ajustes que la app debe ejecutar (en un
/// hilo aparte: toca el registro del sistema).
pub enum SettingsAction {
    RegisterShell,
    UnregisterShell,
}

/// Ventana flotante de ajustes. El llamador detecta cambios comparando el
/// estado antes/después y persiste si procede. `shell_status` es el resultado
/// del último registro/desregistro, para mostrarlo.
pub fn settings_window(
    ctx: &egui::Context,
    settings: &mut AppSettings,
    open: &mut bool,
    shell_status: &str,
) -> Option<SettingsAction> {
    let mut action = None;
    egui::Window::new("Settings")
        .open(open)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            ui.label("Theme");
            ui.horizontal(|ui| {
                for choice in [ThemeChoice::System, ThemeChoice::Light, ThemeChoice::Dark] {
                    ui.selectable_value(&mut settings.theme, choice, choice.label());
                }
            });
            ui.add_space(10.0);

            ui.label("New canvas format");
            egui::ComboBox::from_id_salt("new_canvas_format")
                .selected_text(settings.new_canvas_format.label())
                .show_ui(ui, |ui| {
                    for choice in [
                        NewCanvasFormat::Png,
                        NewCanvasFormat::Jpeg,
                        NewCanvasFormat::WebP,
                        NewCanvasFormat::Canvas,
                    ] {
                        ui.selectable_value(
                            &mut settings.new_canvas_format,
                            choice,
                            choice.label(),
                        );
                    }
                });
            ui.weak(
                "What \"New design\" and the \"+\" canvas create: a real image file \
                 (with its layers kept editable in a sidecar) or a standalone .canvas design.",
            );
            ui.add_space(10.0);

            ui.label("JPEG quality when saving");
            ui.add(egui::Slider::new(&mut settings.jpeg_quality, 1..=100).show_value(true));
            ui.weak("Overwriting a JPEG re-encodes it; higher quality = larger file.");
            ui.add_space(10.0);

            let mut ask = !settings.skip_overwrite_warning;
            if ui
                .checkbox(&mut ask, "Ask before overwriting the original file")
                .on_hover_text(
                    "Shows a warning the first time you save over the original \
                     image in each session.",
                )
                .changed()
            {
                settings.skip_overwrite_warning = !ask;
            }

            explorer_section(ui, shell_status, &mut action);
        });
    action
}

/// Sección «File Explorer integration» de la ventana de ajustes: registro y
/// limpieza de las asociaciones «Open with» (en plataformas sin shell
/// integration el botón reporta el error por `shell_status`).
fn explorer_section(ui: &mut egui::Ui, shell_status: &str, action: &mut Option<SettingsAction>) {
    ui.add_space(12.0);
    ui.separator();
    ui.label("File Explorer integration");
    ui.weak(
        "Adds Canvas Desktop to \"Open with\" for images and to the \
         right-click menu of folders.",
    );
    ui.horizontal(|ui| {
        if ui.button("Register").clicked() {
            *action = Some(SettingsAction::RegisterShell);
        }
        if ui.button("Unregister").clicked() {
            *action = Some(SettingsAction::UnregisterShell);
        }
    });
    if !shell_status.is_empty() {
        ui.weak(shell_status);
    }
}
