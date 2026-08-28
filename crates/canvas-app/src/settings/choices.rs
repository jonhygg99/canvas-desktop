//! Enums de preferencia del usuario: tema, formato de lienzo nuevo y
//! orden de la galería. Todos serializables (viven en `settings.json`).

use eframe::egui;
use serde::{Deserialize, Serialize};

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
