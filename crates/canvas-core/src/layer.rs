use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Identificador estable de una capa dentro de un documento.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct LayerId(u64);

impl LayerId {
    pub(crate) fn new(raw: u64) -> Self {
        Self(raw)
    }

    /// Reconstruye un id desde su valor crudo. Solo para deserialización
    /// (sidecar): un id inventado puede colisionar con los del documento.
    pub fn from_raw(raw: u64) -> Self {
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
    /// Volteo horizontal/vertical del contenido (`serde(default)` para que
    /// los documentos guardados antes de existir estos campos sigan abriendo).
    #[serde(default)]
    pub flip_h: bool,
    #[serde(default)]
    pub flip_v: bool,
}

impl Transform {
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
            rotation: 0.0,
            flip_h: false,
            flip_v: false,
        }
    }

    pub fn aspect_ratio(&self) -> f64 {
        if self.height > 0.0 {
            self.width / self.height
        } else {
            1.0
        }
    }

    /// Centro del rect, en coordenadas de página.
    pub fn center(&self) -> (f64, f64) {
        (self.x + self.width / 2.0, self.y + self.height / 2.0)
    }

    /// ¿El punto (coordenadas de página) cae dentro del rect, teniendo en
    /// cuenta la rotación alrededor del centro?
    pub fn contains_point(&self, x: f64, y: f64) -> bool {
        let (cx, cy) = self.center();
        let (sin, cos) = (-self.rotation.to_radians()).sin_cos();
        let (dx, dy) = (x - cx, y - cy);
        let lx = dx * cos - dy * sin;
        let ly = dx * sin + dy * cos;
        lx.abs() <= self.width / 2.0 && ly.abs() <= self.height / 2.0
    }

    /// Las cuatro esquinas del rect rotado, en coordenadas de página, en el
    /// orden: superior-izquierda, superior-derecha, inferior-izquierda,
    /// inferior-derecha (del rect SIN rotar, ya proyectadas).
    pub fn corners(&self) -> [(f64, f64); 4] {
        let (cx, cy) = self.center();
        let (sin, cos) = self.rotation.to_radians().sin_cos();
        let (hw, hh) = (self.width / 2.0, self.height / 2.0);
        [(-hw, -hh), (hw, -hh), (-hw, hh), (hw, hh)]
            .map(|(ox, oy)| (cx + ox * cos - oy * sin, cy + ox * sin + oy * cos))
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
/// Todos los campos nuevos llevan `serde(default)`: los documentos guardados
/// antes de existir siguen abriendo.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Effects {
    /// Radio del desenfoque gaussiano en píxeles; 0 = sin desenfoque.
    pub blur_radius: f32,
    /// Sombra proyectada, si está activa.
    #[serde(default)]
    pub shadow: Option<Shadow>,
    /// Ajustes de color (0 = neutro). Rango −1..=1 salvo indicación.
    #[serde(default)]
    pub brightness: f32,
    #[serde(default)]
    pub contrast: f32,
    #[serde(default)]
    pub saturation: f32,
    /// Temperatura: negativo = frío (azul), positivo = cálido (rojo).
    #[serde(default)]
    pub temperature: f32,
    /// Mezcla a escala de grises, 0..=1.
    #[serde(default)]
    pub grayscale: f32,
    /// Mezcla sepia, 0..=1.
    #[serde(default)]
    pub sepia: f32,
}

impl Effects {
    /// ¿Hay algún ajuste de color distinto del neutro?
    pub fn has_color_adjustments(&self) -> bool {
        self.brightness != 0.0
            || self.contrast != 0.0
            || self.saturation != 0.0
            || self.temperature != 0.0
            || self.grayscale != 0.0
            || self.sepia != 0.0
    }
}

/// Recorte no destructivo de una imagen: la fracción visible del mapa de
/// bits, normalizada 0..=1 (x, y = esquina superior izquierda del recorte).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CropRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl CropRect {
    /// Sin recorte: la imagen completa.
    pub fn full() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        }
    }

    /// Limita el rect al cuadrado unidad con un tamaño mínimo.
    pub fn clamped(self) -> Self {
        const MIN: f64 = 0.01;
        let width = self.width.clamp(MIN, 1.0);
        let height = self.height.clamp(MIN, 1.0);
        Self {
            x: self.x.clamp(0.0, 1.0 - width),
            y: self.y.clamp(0.0, 1.0 - height),
            width,
            height,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImageContent {
    /// Ruta de origen de la imagen, si vino de disco.
    pub source_path: Option<PathBuf>,
    /// Dimensiones reales del mapa de bits (tras orientación EXIF).
    pub natural_width: u32,
    pub natural_height: u32,
    /// Recorte no destructivo; `None` = imagen completa. Los píxeles siguen
    /// intactos: solo cambia qué fracción se muestra.
    #[serde(default)]
    pub crop: Option<CropRect>,
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
