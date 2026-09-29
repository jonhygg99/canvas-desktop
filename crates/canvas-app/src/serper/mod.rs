//! Búsqueda e inserción de imágenes web (Google Images vía Serper.dev)
//! desde el editor.
//!
//! La API de Serper exige una API key; aquí se lee de la variable de
//! entorno `SERPER_API_KEY` (se crea gratis en serper.dev, 2.500 consultas
//! de prueba). `main` carga el `.env` del proyecto al arrancar (dotenvy),
//! así que basta con poner la clave en `.env` (ver `.env.example`);
//! también vale una variable de entorno normal. La red y el decodificado
//! nunca tocan la UI: el panel pide trabajo al `loader` (hilos worker) y
//! los resultados llegan por su canal (`AppMsg`), igual que Unsplash.
//!
//! Coste: 1 token = 1 llamada con `num=100` (~2 créditos Serper). El
//! usuario elige 1–3 tokens por keyword (`TokenBudget`) y con 3 tokens el
//! «Bring more» trae la 2ª página. 1 token ≈ 10 imgs/1cr, 2 tokens ≈ 100
//! imgs/2cr.
//!
//! Filtrado: los dominios con marca de agua, clipart, IA, sociales y el
//! resto de basura visual se descartan en el worker (ver `filter`), con
//! las dimensiones de la API, y se rematan tras decodificar el thumb (con
//! las dimensiones reales). Los bloqueados del usuario (Advanced,
//! persistidos en ajustes) alimentan el filtro Y las exclusiones `-site:`
//! de la query, capadas al tope de palabras. En modo Instagram/Facebook
//! se perdona el bloqueo de CDNs sociales (sus URLs caducan, pero son todo
//! lo que hay) y no se escribe a disco.
//!
//! Reparto: `types` (presupuesto, modos y resultados), `filter` (filtro
//! puro, query y parseo de persona), `api` (cliente HTTP), `state` (estado
//! del panel en `EditorState`), `panel` (UI de la pestaña Web), `bulk`
//! (overlay de selección masiva e inserción de cada elegida como capa),
//! `bulk_layout` (matemáticas puras del masonry), `cache` (respuestas
//! guardadas en memoria y disco para no gastar créditos en repeticiones)
//! y `card` (tarjeta con clic suave y arrastre al lienzo).

pub(crate) mod api;
pub(crate) mod bake;
pub(crate) mod bulk;
pub(crate) mod bulk_layout;
pub(crate) mod cache;
pub(crate) mod card;
pub(crate) mod filter;
pub(crate) mod panel;
pub(crate) mod state;
pub(crate) mod types;

/// Variable de entorno con la API key de Serper.
pub const API_KEY_ENV: &str = "SERPER_API_KEY";

#[allow(unused_imports)]
pub use api::{decode, fetch_image, search, SerperError};
pub use filter::default_blocked_domains;
pub use panel::panel_ui;
pub use state::{DragSerper, Panel};
pub use types::{SearchMode, SearchPage, SearchRequest, TokenBudget};

// Nombres que solo usan los tests (glob `use super::*` en `tests.rs`).
#[cfg(test)]
use api::{build_request, is_pattern_error, ImagesResponse, QueryStage, PER_CALL_RESULTS};

#[cfg(test)]
mod tests;
