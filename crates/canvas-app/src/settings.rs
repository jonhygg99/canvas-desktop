//! Ajustes persistidos del usuario: un JSON pequeño en el directorio de
//! configuración de la plataforma. Se cargan una vez al arrancar y se
//! escriben en un hilo aparte cada vez que cambian.

use std::path::PathBuf;

use eframe::egui;
use serde::{Deserialize, Serialize};

/// Criterio de orden de la galería de carpetas.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum GallerySort {
    #[default]
    Name,
    DateModified,
}

impl GallerySort {
    pub fn label(self) -> &'static str {
        match self {
            GallerySort::Name => "Name",
            GallerySort::DateModified => "Date modified",
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
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            jpeg_quality: 92,
            skip_overwrite_warning: false,
            sidecar_default: true,
            gallery_sort: GallerySort::default(),
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

            ui.add_space(12.0);
            ui.separator();
            ui.label("File Explorer integration");
            ui.weak(
                "Adds Canvas Desktop to \"Open with\" for images and to the \
                 right-click menu of folders.",
            );
            ui.horizontal(|ui| {
                if ui.button("Register").clicked() {
                    action = Some(SettingsAction::RegisterShell);
                }
                if ui.button("Unregister").clicked() {
                    action = Some(SettingsAction::UnregisterShell);
                }
            });
            if !shell_status.is_empty() {
                ui.weak(shell_status);
            }
        });
    action
}
