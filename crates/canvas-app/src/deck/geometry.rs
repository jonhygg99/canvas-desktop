//! Geometria de la baraja: en que eje se apilan los lienzos, de que lado va
//! la tira, y el rectangulo en espacio de baraja que ocupa cada ranura.

use serde::{Deserialize, Serialize};

/// Eje de apilado de la baraja. Solo cambia el bucle de `Deck::relayout` (qué
/// coordenada acumula, cuál centra) y qué componente de la rueda del ratón
/// mueve `canvas_ui` a lo largo del eje primario — el resto (carga perezosa,
/// descarte, `visible_indices`, `bounds`) trabaja sobre `DeckRect` sin saber
/// ni le importa en qué eje está apilada la baraja.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, Debug)]
pub enum DeckAxis {
    #[default]
    Vertical,
    Horizontal,
}

impl DeckAxis {
    pub fn toggled(self) -> Self {
        match self {
            DeckAxis::Vertical => DeckAxis::Horizontal,
            DeckAxis::Horizontal => DeckAxis::Vertical,
        }
    }
}

/// Lado de la ventana donde vive la tira de miniaturas. DELIBERADAMENTE
/// independiente de `DeckAxis`: el eje decide cómo se APILAN los lienzos en
/// el espacio de baraja (`Deck::relayout`) y qué componente de la rueda
/// desplaza a lo largo de la pila (`canvas_ui`); esto decide solo dónde se
/// dibuja el panel de la tira. Estaban acoplados por convención, no por
/// necesidad — nadie que use `axis` mira la tira, y `deck_strip_ui` no mira
/// `relayout`.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, Debug)]
pub enum StripSide {
    #[default]
    Left,
    Right,
    Top,
    Bottom,
}

impl StripSide {
    /// Siguiente lado en sentido antihorario: Left → Bottom → Right → Top →
    /// Left. Cuatro estados, así que un `toggled()` estilo `DeckAxis` no
    /// vale: un solo botón que cicla es lo que cabe en una cabecera de
    /// 96 px.
    pub fn cycled(self) -> Self {
        match self {
            StripSide::Left => StripSide::Bottom,
            StripSide::Bottom => StripSide::Right,
            StripSide::Right => StripSide::Top,
            StripSide::Top => StripSide::Left,
        }
    }

    /// ¿Las celdas fluyen en columna? Left/Right sí (el ancho manda), Top/
    /// Bottom no (manda el alto). El único bit que necesita `deck_strip_ui`:
    /// el mapeo 4→2 que evita duplicar el cuerpo del panel.
    pub fn is_vertical_flow(self) -> bool {
        matches!(self, StripSide::Left | StripSide::Right)
    }

    pub fn label(self) -> &'static str {
        match self {
            StripSide::Left => "Left",
            StripSide::Top => "Top",
            StripSide::Right => "Right",
            StripSide::Bottom => "Bottom",
        }
    }
}

/// Dirección para `Deck::move_slot` — las flechas ◀/▶ de la cabecera de
/// cada lienzo en el área central.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MoveDir {
    Prev,
    Next,
}

/// Rect en espacio de baraja (px de página). `f64`: una carpeta de 200 fotos
/// llega al millón de píxeles acumulados y `f32` empieza a perder precisión
/// al hacer zoom sobre una ranura lejana.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DeckRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl DeckRect {
    pub const ZERO: Self = Self {
        x: 0.0,
        y: 0.0,
        w: 0.0,
        h: 0.0,
    };

    pub fn origin(&self) -> (f64, f64) {
        (self.x, self.y)
    }

    pub(super) fn intersects(&self, other: DeckRect) -> bool {
        self.x < other.x + other.w
            && self.x + self.w > other.x
            && self.y < other.y + other.h
            && self.y + self.h > other.y
    }
}
