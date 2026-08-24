//! Ajustes persistidos del usuario: un JSON pequeño en el directorio de
//! configuración de la plataforma. Se cargan una vez al arrancar y se
//! escriben en un hilo aparte cada vez que cambian.

use std::path::PathBuf;

use eframe::egui;
use serde::{Deserialize, Serialize};

use crate::deck::{DeckAxis, StripSide};

/// Tema de la interfaz.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ThemeChoice {
    /// Sigue el tema del sistema (por defecto).
    #[default]
    System,
    Light,
    Dark,
}

impl ThemeChoice {
    pub fn label(self) -> &'static str {
        match self {
            ThemeChoice::System => "System",
            ThemeChoice::Light => "Light",
            ThemeChoice::Dark => "Dark",
        }
    }

    pub fn to_egui(self) -> egui::ThemePreference {
        match self {
            ThemeChoice::System => egui::ThemePreference::System,
            ThemeChoice::Light => egui::ThemePreference::Light,
            ThemeChoice::Dark => egui::ThemePreference::Dark,
        }
    }
}

/// Formato en el que nace un lienzo en blanco («✚ New design» de la galería,
/// zona «+» de la baraja). Un raster real (Png/Jpeg/WebP) queda respaldado
/// por un sidecar `.canvas` con sus capas, igual que cualquier otra imagen
/// editada — visible en el Explorador y en cualquier visor. `Canvas` es el
/// comportamiento de antes de este ajuste: un diseño autónomo, sin ningún
/// archivo de imagen detrás.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, Debug)]
pub enum NewCanvasFormat {
    #[default]
    Png,
    Jpeg,
    WebP,
    Canvas,
}

impl NewCanvasFormat {
    pub fn label(self) -> &'static str {
        match self {
            NewCanvasFormat::Png => "PNG image",
            NewCanvasFormat::Jpeg => "JPEG image",
            NewCanvasFormat::WebP => "WebP image",
            NewCanvasFormat::Canvas => "Canvas design (.canvas)",
        }
    }

    /// Extensión de archivo (sin el punto), lista para
    /// `canvas_io::reserve_numbered_path`.
    pub fn extension(self) -> &'static str {
        match self {
            NewCanvasFormat::Png => "png",
            NewCanvasFormat::Jpeg => "jpg",
            NewCanvasFormat::WebP => "webp",
            NewCanvasFormat::Canvas => canvas_io::CANVAS_EXTENSION,
        }
    }
}

/// Criterio de orden de la galería de carpetas.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, Debug)]
pub enum GallerySort {
    #[default]
    Name,
    DateModified,
    /// Orden explícito del usuario (flechas ◀/▶ del panel del lienzo).
    /// Nunca se ofrece como opción del selector de la galería: se entra en
    /// él implícitamente al mover una ranura (`Deck::move_slot`), y
    /// DELIBERADAMENTE no se persiste en `AppSettings` — es una decisión
    /// sobre esta carpeta, no una preferencia global.
    Manual,
}

impl GallerySort {
    pub fn label(self) -> &'static str {
        match self {
            GallerySort::Name => "Name",
            GallerySort::DateModified => "Date modified",
            GallerySort::Manual => "Manual order",
        }
    }
}

/// Compara dos nombres tratando cada tramo de dígitos consecutivos como un
/// número, no como texto — así "6.png" ordena antes que "51.png" en vez de
/// después. El `Ord` de `String` puro (byte a byte) es lo que usaba antes
/// `GallerySort::Name`: en una carpeta de fotos numeradas sin ceros de
/// relleno (`1.png`..`51.png`) las de un solo dígito (6,7,8,9) acababan
/// DESPUÉS de la 51 — una sorpresa real, no una preferencia de nadie.
/// Explorador de Windows y Finder ya comparan así por defecto, así que
/// "Name" pasa a significar esto en vez de añadir un tercer criterio.
pub fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let mut ai = a.chars().peekable();
    let mut bi = b.chars().peekable();
    loop {
        let (ac, bc) = match (ai.peek().copied(), bi.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(ac), Some(bc)) => (ac, bc),
        };
        if ac.is_ascii_digit() && bc.is_ascii_digit() {
            let mut a_run = String::new();
            while let Some(&c) = ai.peek() {
                if !c.is_ascii_digit() {
                    break;
                }
                a_run.push(c);
                ai.next();
            }
            let mut b_run = String::new();
            while let Some(&c) = bi.peek() {
                if !c.is_ascii_digit() {
                    break;
                }
                b_run.push(c);
                bi.next();
            }
            // Sin ceros a la izquierda para comparar el valor, no los
            // dígitos: longitud primero (más dígitos = número mayor, ya sin
            // ceros de relleno), luego lexicográfico como desempate entre
            // tramos de igual longitud.
            let a_trimmed = a_run.trim_start_matches('0');
            let b_trimmed = b_run.trim_start_matches('0');
            let ord = a_trimmed
                .len()
                .cmp(&b_trimmed.len())
                .then_with(|| a_trimmed.cmp(b_trimmed));
            if ord != Ordering::Equal {
                return ord;
            }
            // Mismo valor numérico (p.ej. "007" y "7"): sigue comparando el
            // resto del nombre en vez de darlo por empatado aquí.
        } else {
            let al = ac.to_ascii_lowercase();
            let bl = bc.to_ascii_lowercase();
            if al != bl {
                return al.cmp(&bl);
            }
            ai.next();
            bi.next();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_cmp_orders_numbered_filenames_numerically() {
        let mut names = vec!["6.png", "51.png", "10.png", "1.png", "9.png"];
        names.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(names, vec!["1.png", "6.png", "9.png", "10.png", "51.png"]);
    }

    #[test]
    fn natural_cmp_is_case_insensitive_on_the_non_numeric_parts() {
        assert_eq!(
            natural_cmp("Photo2.png", "photo10.png"),
            std::cmp::Ordering::Less
        );
    }

    #[test]
    fn natural_cmp_treats_leading_zeros_as_the_same_number() {
        assert_eq!(natural_cmp("007.png", "7.png"), std::cmp::Ordering::Equal);
    }

    #[test]
    fn natural_cmp_falls_back_to_plain_text_without_digits() {
        let mut names = vec!["banana.png", "apple.png", "cherry.png"];
        names.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(names, vec!["apple.png", "banana.png", "cherry.png"]);
    }

    #[test]
    fn layers_tab_order_swapped_round_trips() {
        assert_eq!(
            LayersTabOrder::PageFirst.swapped(),
            LayersTabOrder::LayersFirst
        );
        assert_eq!(
            LayersTabOrder::LayersFirst.swapped(),
            LayersTabOrder::PageFirst
        );
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
