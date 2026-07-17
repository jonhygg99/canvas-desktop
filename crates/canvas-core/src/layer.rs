use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Identificador estable de una capa dentro de un documento.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct LayerId(u64);

impl LayerId {
    pub(crate) fn new(raw: u64) -> Self {
        Self(raw)
    }

    pub fn raw(self) -> u64 {
        self.0
    }
}

/// Posición y tamaño de una capa en coordenadas de página (píxeles).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Transform {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    /// Rotación en grados, sentido horario, alrededor del centro.
    pub rotation: f64,
}

impl Transform {
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
            rotation: 0.0,
        }
    }

    pub fn aspect_ratio(&self) -> f64 {
        if self.height > 0.0 {
            self.width / self.height
        } else {
            1.0
        }
    }
}

/// Sombra proyectada de una capa (rectangular, difusa).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Shadow {
    /// Desplazamiento en píxeles de página.
    pub offset_x: f64,
    pub offset_y: f64,
    /// Desviación estándar del desenfoque, en píxeles.
    pub blur: f32,
    /// Opacidad 0..=1.
    pub opacity: f32,
}

impl Default for Shadow {
    fn default() -> Self {
        Self {
            offset_x: 12.0,
            offset_y: 12.0,
            blur: 24.0,
            opacity: 0.5,
        }
    }
}

/// Efectos no destructivos de la capa: parámetros que se ajustan o quitan en
/// cualquier momento y solo se aplican de verdad al exportar/guardar.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Effects {
    /// Radio del desenfoque gaussiano en píxeles; 0 = sin desenfoque.
    pub blur_radius: f32,
    /// Sombra proyectada, si está activa.
    #[serde(default)]
    pub shadow: Option<Shadow>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImageContent {
    /// Ruta de origen de la imagen, si vino de disco.
    pub source_path: Option<PathBuf>,
    /// Dimensiones reales del mapa de bits (tras orientación EXIF).
    pub natural_width: u32,
    pub natural_height: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum LayerContent {
    Image(ImageContent),
    // Text, Shape, Svg y Group llegan en entregas posteriores.
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    pub id: LayerId,
    pub name: String,
    pub visible: bool,
    pub locked: bool,
    /// 0.0..=1.0
    pub opacity: f32,
    pub transform: Transform,
    pub effects: Effects,
    pub content: LayerContent,
}

impl Layer {
    pub fn new(
        id: LayerId,
        name: impl Into<String>,
        transform: Transform,
        content: LayerContent,
    ) -> Self {
        Self {
            id,
            name: name.into(),
            visible: true,
            locked: false,
            opacity: 1.0,
            transform,
            effects: Effects::default(),
            content,
        }
    }
}
