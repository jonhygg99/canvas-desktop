//! Fuente de búsqueda de la pestaña «Images» del sidebar: imágenes web de
//! Google (Serper), fotos de una persona en Instagram/Facebook o fotos de
//! Unsplash. Antes eran dos pestañas (Web e Images) con dos barras de
//! búsqueda; ahora son las tres opciones de un mismo selector.
//!
//! Vive aquí, junto a `LeftTab`, porque es estado de sesión de la vista y no
//! de ningún cliente HTTP. `serper::SearchMode` NO gana una variante
//! `Unsplash` a propósito: describe el corpus de una búsqueda de Serper, y sus
//! `match` en `serper::api` y `serper::cache` construyen la petición y la
//! clave de caché, así que una variante que ese cliente nunca puede recibir
//! solo añadiría ramas inalcanzables.

use crate::serper::SearchMode;

/// Qué buscador pinta la pestaña «Images».
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ImageSource {
    /// Imágenes generales de la web (Google Images vía Serper): lo que antes
    /// era la pestaña «Web», y la fuente por defecto.
    #[default]
    Web,
    /// Fotos de una persona en Instagram/Facebook (Serper en modo social).
    Social,
    /// Fotos de Unsplash.
    Unsplash,
}

impl ImageSource {
    /// Las tres fuentes, en el orden del selector.
    pub const ALL: [Self; 3] = [Self::Web, Self::Social, Self::Unsplash];

    /// Modo de Serper equivalente, si esta fuente la sirve Serper. `None`
    /// (Unsplash) significa «no toques el estado de Serper»: alternar con
    /// Unsplash conserva sus resultados, su paginación y su gasto, y volver
    /// los recupera tal cual.
    pub fn serper_mode(self) -> Option<SearchMode> {
        match self {
            Self::Web => Some(SearchMode::Web),
            Self::Social => Some(SearchMode::Social),
            Self::Unsplash => None,
        }
    }
}
