//! Cliente HTTP de Unsplash, solo para hilos worker: búsqueda en la API,
//! descarga de miniaturas/imagenes y decodificado a `LoadedImage`. La clave
//! viaja en el header `Authorization` (nunca en la query string, que acaba
//! en logs y proxies).

use serde::Deserialize;
use thiserror::Error;

use super::types::{Photo, SearchFilters, SearchPage};
use super::ACCESS_KEY_ENV;

const SEARCH_URL: &str = "https://api.unsplash.com/search/photos";
pub(super) const PER_PAGE: u32 = 30;

/// Error tipado del panel de Unsplash. Viaja por `AppMsg` hasta la UI, que
/// solo necesita `Display`; la variante `NotConfigured` es la única que el
/// flujo normal espera (falta de clave), el resto son fallos de red/datos.
#[derive(Debug, Error)]
pub enum UnsplashError {
    #[error("{0} is not set")]
    NotConfigured(&'static str),
    #[error("Unsplash request failed: {0}")]
    Request(String),
    #[error("Unsplash API error: {0}")]
    Api(String),
    #[error("Unsplash returned an invalid response: {0}")]
    BadResponse(String),
    #[error("Unsplash download failed: {0}")]
    Download(String),
    #[error("Unsplash image exceeds the {0}-byte download limit")]
    TooLarge(usize),
    #[error("Unsplash returned an empty image")]
    Empty,
    #[error("Image decode failed: {0}")]
    Decode(String),
}

/// Lee la Access Key del entorno; `None` si no está definida (o está vacía).
pub fn access_key() -> Option<String> {
    std::env::var(ACCESS_KEY_ENV)
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
}

/// Busca fotos en Unsplash con los filtros dados. `page` es 1-based;
/// devuelve hasta `PER_PAGE` resultados y si esta era la última página.
/// Solo se llama desde hilos worker.
pub fn search(query: &str, page: u32, filters: SearchFilters) -> Result<SearchPage, UnsplashError> {
    let Some(key) = access_key() else {
        return Err(UnsplashError::NotConfigured(ACCESS_KEY_ENV));
    };
    let query = query.trim();
    if query.is_empty() {
        return Ok(SearchPage {
            photos: Vec::new(),
            reached_end: true,
        });
    }
    let req = crate::http::agent()
        .get(SEARCH_URL)
        .query("query", query)
        .query("page", &page.to_string())
        .query("per_page", &PER_PAGE.to_string());
    // La clave viaja en el header Authorization, NUNCA en la query string:
    // una URL se filtra por proxies, logs de servidor y errores impresos
    // (el buffer circular que alimenta los informes de crash incluye URLs);
    // el header no forma parte de la URL. Es el mecanismo que la propia
    // API de Unsplash documenta como preferente.
    let mut req = req.set("Authorization", &format!("Client-ID {key}"));
    // Solo se mandan los filtros activos: la API ignora (o falla) valores
    // vacíos, así que un filtro «Any» simplemente no se envía.
    if let Some(o) = filters.orientation.as_str() {
        req = req.query("orientation", o);
    }
    if let Some(c) = filters.color.as_str() {
        req = req.query("color", c);
    }
    req = req.query("order_by", filters.order_by.as_str());
    let resp = req
        .call()
        .map_err(|e| UnsplashError::Request(e.to_string()))?;
    let body = resp
        .into_string()
        .map_err(|e| UnsplashError::Request(e.to_string()))?;
    let parsed: SearchResponse =
        serde_json::from_str(&body).map_err(|e| UnsplashError::BadResponse(e.to_string()))?;
    if let Some(err) = parsed.errors.first() {
        return Err(UnsplashError::Api(err.clone()));
    }
    Ok(SearchPage {
        reached_end: reached_end(parsed.total_pages, page, parsed.results.len()),
        photos: parsed.results,
    })
}

/// Envuelve el JSON de `/search/photos`: resultados, errores y el total de
/// páginas (para saber cuándo «Load more» ya no tiene más resultados).
#[derive(Debug, Deserialize)]
pub(super) struct SearchResponse {
    #[serde(default)]
    pub(super) results: Vec<Photo>,
    #[serde(default)]
    pub(super) errors: Vec<String>,
    /// Páginas disponibles en total; `None` si la respuesta no lo trae.
    #[serde(default)]
    pub(super) total_pages: Option<u32>,
}

/// ¿Esta página era la última? O la API dice que `page` alcanzó
/// `total_pages`, o devolvió menos fotos de las que caben en una página
/// (defensa si el campo `total_pages` falta en la respuesta).
pub(super) fn reached_end(total_pages: Option<u32>, page: u32, returned: usize) -> bool {
    total_pages.is_some_and(|t| page >= t) || returned < PER_PAGE as usize
}

/// Descarga el contenido de una URL (miniatura o imagen completa). Solo se
/// llama desde hilos worker. Corta con error al superar `MAX_DOWNLOAD_BYTES`
/// en vez de acumular sin límite. Reutiliza el helper compartido y mapea su
/// error al tipo de este dominio.
pub fn download(url: &str) -> Result<Vec<u8>, UnsplashError> {
    crate::http::get_bytes_bounded(url).map_err(|e| match e {
        crate::http::HttpError::Download(err) => UnsplashError::Download(err),
        crate::http::HttpError::TooLarge(n) => UnsplashError::TooLarge(n),
        crate::http::HttpError::Empty => UnsplashError::Empty,
    })
}

/// Decodifica bytes (PNG/JPEG/WebP…) a `LoadedImage` RGBA8, listo para
/// `add_image_layer` o para una textura de egui.
pub fn decode(bytes: &[u8]) -> Result<canvas_io::LoadedImage, UnsplashError> {
    let img = image::load_from_memory(bytes).map_err(|e| UnsplashError::Decode(e.to_string()))?;
    let rgba = img.to_rgba8();
    let (w, h) = (rgba.width(), rgba.height());
    Ok(canvas_io::LoadedImage {
        rgba: rgba.into_raw(),
        width: w,
        height: h,
    })
}
