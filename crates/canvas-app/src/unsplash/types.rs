//! Tipos de la API de Unsplash compartidos por el cliente (`api`), el estado
//! del panel (`state`) y la UI (`panel`/`card`): filtros de búsqueda,
//! resultados y páginas.

use eframe::egui;

/// Orientación de las fotos del resultado (parámetro `orientation` de la
/// API). `Any` no envía el parámetro.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Orientation {
    #[default]
    Any,
    Landscape,
    Portrait,
    Squarish,
}

impl Orientation {
    pub const ALL: [Self; 4] = [Self::Any, Self::Landscape, Self::Portrait, Self::Squarish];

    /// Valor para la API; `None` = sin filtro.
    pub fn as_str(self) -> Option<&'static str> {
        match self {
            Self::Any => None,
            Self::Landscape => Some("landscape"),
            Self::Portrait => Some("portrait"),
            Self::Squarish => Some("squarish"),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Any => "Any",
            Self::Landscape => "Landscape",
            Self::Portrait => "Portrait",
            Self::Squarish => "Square",
        }
    }
}

/// Color dominante de la foto (parámetro `color` de la API). `Any` no envía
/// el parámetro.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorFilter {
    #[default]
    Any,
    BlackAndWhite,
    Black,
    White,
    Yellow,
    Orange,
    Red,
    Purple,
    Magenta,
    Green,
    Teal,
    Blue,
}

impl ColorFilter {
    pub const ALL: [Self; 12] = [
        Self::Any,
        Self::BlackAndWhite,
        Self::Black,
        Self::White,
        Self::Yellow,
        Self::Orange,
        Self::Red,
        Self::Purple,
        Self::Magenta,
        Self::Green,
        Self::Teal,
        Self::Blue,
    ];

    /// Valor para la API; `None` = sin filtro.
    pub fn as_str(self) -> Option<&'static str> {
        match self {
            Self::Any => None,
            Self::BlackAndWhite => Some("black_and_white"),
            Self::Black => Some("black"),
            Self::White => Some("white"),
            Self::Yellow => Some("yellow"),
            Self::Orange => Some("orange"),
            Self::Red => Some("red"),
            Self::Purple => Some("purple"),
            Self::Magenta => Some("magenta"),
            Self::Green => Some("green"),
            Self::Teal => Some("teal"),
            Self::Blue => Some("blue"),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Any => "Any color",
            Self::BlackAndWhite => "B&W",
            Self::Black => "Black",
            Self::White => "White",
            Self::Yellow => "Yellow",
            Self::Orange => "Orange",
            Self::Red => "Red",
            Self::Purple => "Purple",
            Self::Magenta => "Magenta",
            Self::Green => "Green",
            Self::Teal => "Teal",
            Self::Blue => "Blue",
        }
    }

    /// Color aproximado para el punto de la UI; `None` para «sin filtro».
    pub fn swatch(self) -> Option<egui::Color32> {
        match self {
            Self::Any => None,
            Self::BlackAndWhite => Some(egui::Color32::from_gray(160)),
            Self::Black => Some(egui::Color32::from_gray(20)),
            Self::White => Some(egui::Color32::from_gray(235)),
            Self::Yellow => Some(egui::Color32::from_rgb(245, 194, 17)),
            Self::Orange => Some(egui::Color32::from_rgb(245, 137, 15)),
            Self::Red => Some(egui::Color32::from_rgb(217, 30, 24)),
            Self::Purple => Some(egui::Color32::from_rgb(142, 68, 173)),
            Self::Magenta => Some(egui::Color32::from_rgb(214, 25, 99)),
            Self::Green => Some(egui::Color32::from_rgb(0, 150, 64)),
            Self::Teal => Some(egui::Color32::from_rgb(0, 121, 107)),
            Self::Blue => Some(egui::Color32::from_rgb(0, 81, 186)),
        }
    }
}

/// Orden de los resultados (parámetro `order_by` de la API).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OrderBy {
    #[default]
    Relevant,
    Latest,
}

impl OrderBy {
    pub const ALL: [Self; 2] = [Self::Relevant, Self::Latest];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Relevant => "relevant",
            Self::Latest => "latest",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Relevant => "Relevant",
            Self::Latest => "Latest",
        }
    }
}

/// Filtros activos de la búsqueda. Se copian al worker para que la petición
/// use los valores del momento en que se lanzó.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SearchFilters {
    pub orientation: Orientation,
    pub color: ColorFilter,
    pub order_by: OrderBy,
}

/// Un resultado de la búsqueda: lo mínimo que la UI necesita para mostrar la
/// miniatura, atribuir al autor y descargar la imagen. Serde ignora el resto
/// de campos que Unsplash devuelva.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Photo {
    pub id: String,
    pub urls: Urls,
    pub user: User,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Urls {
    /// 400px: la que se muestra grande en la lista del panel.
    pub small: String,
    /// Imagen de tamaño medio, lo que se inserta al hacer clic.
    pub regular: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct User {
    pub name: String,
}

/// Una página de resultados ya resuelta: las fotos y si esta era la última
/// página (para ocultar «Load more» y avisar del final).
#[derive(Debug, Clone)]
pub struct SearchPage {
    pub photos: Vec<Photo>,
    /// `true` si no hay más páginas tras esta (fin de los resultados).
    pub reached_end: bool,
}
