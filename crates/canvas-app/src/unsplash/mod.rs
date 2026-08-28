//! Búsqueda e inserción de imágenes de Unsplash desde el editor.
//!
//! La API de Unsplash exige una Access Key; aquí se lee de la variable de
//! entorno `UNSPLASH_ACCESS_KEY` (se crea gratis en unsplash.com/developers).
//! `main` carga el `.env` del proyecto al arrancar (dotenvy), así que basta
//! con poner la clave en `.env` (ver `.env.example`); también vale una
//! variable de entorno normal. La red y el decodificado nunca tocan la UI:
//! el panel pide trabajo al `loader` (hilos worker) y los resultados llegan
//! por su canal (`AppMsg`), igual que las miniaturas de la galería.
//!
//! Atribución: los términos de Unsplash requieren mostrar el autor. Cada
//! resultado muestra el nombre del fotógrafo y la capa insertada se llama
//! «Unsplash · <autor>».
//!
//! Reparto: `types` (tipos de filtros y resultados de la API), `api`
//! (cliente HTTP: búsqueda, descarga y decodificado), `state` (estado del
//! panel en `EditorState`), `panel` (UI de la pestaña Images) y `card`
//! (tarjeta de foto con clic suave y arrastre al lienzo).

pub(crate) mod api;
pub(crate) mod card;
pub(crate) mod panel;
pub(crate) mod state;
pub(crate) mod types;

/// Variable de entorno con la Access Key de la API de Unsplash.
pub const ACCESS_KEY_ENV: &str = "UNSPLASH_ACCESS_KEY";

pub use api::{decode, download, search, UnsplashError};
pub use panel::panel_ui;
pub use state::{DragUnsplash, Panel, PhotoItem};
pub use types::{SearchFilters, SearchPage};

// Nombres que solo usan los tests (glob `use super::*` en `tests.rs`).
#[cfg(test)]
use api::{reached_end, SearchResponse, PER_PAGE};
#[cfg(test)]
use types::{ColorFilter, OrderBy, Orientation};

#[cfg(test)]
mod tests;
