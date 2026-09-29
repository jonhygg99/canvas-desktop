//! Cliente HTTP de Serper (búsqueda de imágenes), solo para hilos worker:
//! la llamada a la API, la descarga de miniaturas/imágenes y el
//! decodificado a `LoadedImage`. La clave viaja en el header `X-API-KEY`
//! (nunca en la query string ni en el cuerpo logueable).

use serde::Deserialize;
use thiserror::Error;

use super::filter::{apply_filter, build_query};
use super::types::SerperPhoto;

use super::API_KEY_ENV;

/// Endpoint de búsqueda de imágenes de Serper.
const IMAGES_URL: &str = "https://google.serper.dev/images";
/// Resultados por llamada grande: el máximo (100) para filtrar en local.
/// Una llamada de 100 cuesta 2 créditos.
pub const PER_CALL_RESULTS: u32 = 100;
/// Resultados de la llamada barata (1 token): ~10 imágenes por 1 crédito.
pub const SMALL_RESULTS: u32 = 10;
/// Créditos Serper que cuesta cada llamada de 100 resultados.
pub const CREDITS_PER_CALL: u8 = 2;
/// Créditos que cuesta la llamada pequeña de ~10 resultados.
pub const CREDITS_PER_SMALL_CALL: u8 = 1;
/// Geolocalización e idioma fijos de la búsqueda.
const GEO: &str = "mx";
const LANG: &str = "es";
/// Filtro de Google Images: fotos grandes apaisadas o mayores.
const TBS: &str = "isz:lt,islt:xga,itp:photo";

/// Error tipado del panel Web. Viaja por `AppMsg` hasta la UI, que solo
/// necesita `Display`; `NotConfigured` es la falta de clave y
/// `FilteredBanner` es interno (el thumb resultó ser un banner al
/// decodificarlo: la tarjeta se retira en silencio, no es un error).
#[derive(Debug, Error)]
pub enum SerperError {
    #[error("{0} is not set")]
    NotConfigured(&'static str),
    #[error("Serper request failed: {0}")]
    Request(String),
    #[error("Serper API error: {0}")]
    Api(String),
    #[error("Serper returned an invalid response: {0}")]
    BadResponse(String),
    #[error("Web download failed: {0}")]
    Download(String),
    #[error("Web image exceeds the {0}-byte download limit")]
    TooLarge(usize),
    #[error("Web returned an empty image")]
    Empty,
    #[error("Image decode failed: {0}")]
    Decode(String),
    #[error("Filtered out: banner aspect ratio")]
    FilteredBanner,
}

/// Lee la API key del entorno; `None` si no está definida (o está vacía).
pub fn access_key() -> Option<String> {
    std::env::var(API_KEY_ENV)
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
}

/// Busca imágenes en Serper (`page` 1-based, hasta `num` resultados). El
/// filtro se aplica aquí, en el worker: la UI solo recibe supervivientes
/// más las cuentas de descarte para el pie del panel. Solo se llama desde
/// hilos worker.
///
/// Degradado automático: las cuentas gratuitas rechazan patrones como
/// `-site:` (HTTP 400 «Query pattern not allowed»). Ante ese error se
/// reintenta sin exclusiones y, si persiste, sin `tbs`. Las llamadas
/// fallidas no las cobra Serper, así que el reintento no gasta tokens.
pub fn search(
    keyword: &str,
    page: u32,
    num: u32,
    blocked: &[String],
) -> Result<super::types::SearchPage, SerperError> {
    let keyword = keyword.trim();
    if keyword.is_empty() {
        return Ok(super::types::SearchPage {
            photos: Vec::new(),
            reached_end: true,
            credits_charged: 0,
            from_cache: false,
            effective_query: String::new(),
            exclusions_applied: 0,
            exclusions_dropped: 0,
            simplified: false,
            filtered: super::filter::FilterCounts::default(),
        });
    }
    let Some(key) = access_key() else {
        return Err(SerperError::NotConfigured(API_KEY_ENV));
    };
    let counts = build_query(keyword, blocked);
    let mut last_err = None;
    for stage in [
        QueryStage::Full,
        QueryStage::PlainKeyword,
        QueryStage::Minimal,
    ] {
        let req = build_request(keyword, &counts, stage, page, num);
        match post_images(&key, &req.body) {
            Ok(text) => return parse_page(&text, &req, blocked, num),
            Err(SerperError::Api(msg))
                if is_pattern_error(&msg) && stage != QueryStage::Minimal =>
            {
                tracing::info!("Serper rechazó el patrón ({stage:?}); degradando la query");
                last_err = Some(SerperError::Api(msg));
            }
            Err(e) => return Err(e),
        }
    }
    Err(last_err.unwrap_or_else(|| SerperError::Request("no response".to_owned())))
}

/// Nivel de simplificación de la petición.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum QueryStage {
    /// Keyword + `-site:` + `tbs` (cuentas de pago).
    Full,
    /// Keyword sola + `tbs` (sin operadores).
    PlainKeyword,
    /// Keyword sola, sin nada más.
    Minimal,
}

/// Petición ya construida para un nivel: cuerpo JSON y transparencia.
pub(super) struct BuiltRequest {
    pub(super) body: String,
    pub(super) effective_query: String,
    pub(super) applied: usize,
    pub(super) dropped: usize,
    pub(super) simplified: bool,
}

/// Construye el cuerpo de la petición para `stage` (puro, testeable).
pub(super) fn build_request(
    keyword: &str,
    counts: &super::filter::BuiltQuery,
    stage: QueryStage,
    page: u32,
    num: u32,
) -> BuiltRequest {
    let simplified = stage != QueryStage::Full;
    let effective = match stage {
        QueryStage::Full => counts.query.clone(),
        _ => keyword.to_owned(),
    };
    let mut json = serde_json::json!({
        "q": effective,
        "gl": GEO,
        "hl": LANG,
        "num": num,
        "page": page,
    });
    if stage != QueryStage::Minimal {
        json["tbs"] = serde_json::Value::String(TBS.to_owned());
    }
    // `send_string` en vez de `send_json`: ureq viene sin la feature
    // `json` (solo `tls`) y el cuerpo ya sale serializado de serde_json.
    BuiltRequest {
        body: serde_json::to_string(&json).unwrap_or_default(),
        effective_query: effective,
        applied: if simplified { 0 } else { counts.applied },
        dropped: if simplified {
            counts.applied + counts.dropped
        } else {
            counts.dropped
        },
        simplified,
    }
}

/// ¿Es el 400 de patrón no permitido en cuentas gratuitas?
pub(super) fn is_pattern_error(msg: &str) -> bool {
    msg.contains("Query pattern not allowed")
}

/// Envía la petición y devuelve el cuerpo en texto. Los 4xx/5xx llegan
/// como error de API con el motivo del cuerpo, no como fallo de red.
fn post_images(key: &str, body: &str) -> Result<String, SerperError> {
    let resp = crate::http::agent()
        .post(IMAGES_URL)
        .set("X-API-KEY", key)
        .set("Content-Type", "application/json")
        .send_string(body)
        .map_err(|e| match e {
            // La API responde 4xx/5xx con el motivo en el cuerpo: se
            // propaga como error de API, no como fallo de red.
            ureq::Error::Status(code, resp) => {
                let detail = resp.into_string().unwrap_or_default();
                SerperError::Api(format!("HTTP {code}: {}", detail.trim()))
            }
            other => SerperError::Request(other.to_string()),
        })?;
    resp.into_string()
        .map_err(|e| SerperError::Request(e.to_string()))
}

/// Parsea la respuesta y aplica el filtro local (con la lista real de
/// bloqueados, aunque la query viajase simplificada).
fn parse_page(
    text: &str,
    req: &BuiltRequest,
    blocked: &[String],
    num: u32,
) -> Result<super::types::SearchPage, SerperError> {
    let parsed: ImagesResponse =
        serde_json::from_str(text).map_err(|e| SerperError::BadResponse(e.to_string()))?;
    let raw = parsed.images.len();
    // Coste real de la respuesta; si falta (API antigua), se estima por
    // tamaño pedido: más de 10 resultados siempre cuesta 2.
    let credits = parsed.credits.unwrap_or_else(|| {
        if num > SMALL_RESULTS {
            u32::from(CREDITS_PER_CALL)
        } else {
            u32::from(CREDITS_PER_SMALL_CALL)
        }
    });
    let photos: Vec<SerperPhoto> = parsed
        .images
        .into_iter()
        .filter_map(|img| img.into_photo())
        .collect();
    let (photos, filtered) = apply_filter(photos, blocked);
    Ok(super::types::SearchPage {
        photos,
        reached_end: raw < num as usize,
        credits_charged: credits,
        from_cache: false,
        effective_query: req.effective_query.clone(),
        exclusions_applied: req.applied,
        exclusions_dropped: req.dropped,
        simplified: req.simplified,
        filtered,
    })
}

/// Respuesta de `/images`: la lista de imágenes más el coste REAL de la
/// llamada (cada respuesta trae su `credits`; es la fuente de verdad para
/// el saldo, no la estimación por `num`). El resto de campos se ignoran.
#[derive(Debug, Deserialize)]
pub(super) struct ImagesResponse {
    #[serde(default)]
    pub(super) images: Vec<RawImage>,
    #[serde(default)]
    pub(super) credits: Option<u32>,
}

/// Una imagen tal y como la devuelve Serper. Todo con default: una entrada
/// a medias se descarta en `into_photo`, nunca rompe la tanda.
#[derive(Debug, Deserialize)]
pub(super) struct RawImage {
    #[serde(default)]
    pub(super) title: String,
    #[serde(rename = "imageUrl", default)]
    pub(super) image_url: String,
    #[serde(rename = "imageWidth", default)]
    pub(super) width: Option<u32>,
    #[serde(rename = "imageHeight", default)]
    pub(super) height: Option<u32>,
    #[serde(default)]
    pub(super) link: String,
}

impl RawImage {
    /// Convierte a `SerperPhoto` o `None` si no hay URL directa utilizable.
    /// El `id` es la propia URL (clave de dedupe entre páginas).
    pub(super) fn into_photo(self) -> Option<SerperPhoto> {
        let url = self.image_url.trim().to_owned();
        if url.is_empty() || super::filter::host_of(&url).is_none() {
            return None;
        }
        Some(SerperPhoto {
            id: url.clone(),
            title: self.title,
            image_url: url,
            width: self.width,
            height: self.height,
            source_url: self.link,
        })
    }
}

/// Motivo corto para el placeholder de una tarjeta sin preview (el texto
/// completo del error viaja al tooltip y al log).
pub fn short_reason(msg: &str) -> &'static str {
    if msg.contains("403") || msg.contains("Forbidden") {
        "blocked (403)"
    } else if msg.contains("exceeds") || msg.contains("TooLarge") {
        "too large"
    } else if msg.contains("empty") || msg.contains("Empty") {
        "empty"
    } else if msg.contains("decode") || msg.contains("Decode") {
        "unreadable"
    } else if msg.contains("timed out") || msg.contains("timeout") {
        "timeout"
    } else {
        "failed"
    }
}

/// Descarga el contenido de una URL (miniatura o imagen completa). Solo se
/// llama desde hilos worker. Reutiliza el helper compartido y mapea su
/// error al tipo de este dominio.
pub fn download(url: &str) -> Result<Vec<u8>, SerperError> {
    crate::http::get_bytes_bounded(url).map_err(|e| match e {
        crate::http::HttpError::Download(err) => SerperError::Download(err),
        crate::http::HttpError::TooLarge(n) => SerperError::TooLarge(n),
        crate::http::HttpError::Empty => SerperError::Empty,
    })
}

/// Decodifica bytes (PNG/JPEG/WebP…) a `LoadedImage` RGBA8, listo para
/// `add_image_layer` o para una textura de egui.
pub fn decode(bytes: &[u8]) -> Result<canvas_io::LoadedImage, SerperError> {
    let img = image::load_from_memory(bytes).map_err(|e| SerperError::Decode(e.to_string()))?;
    let rgba = img.to_rgba8();
    let (w, h) = (rgba.width(), rgba.height());
    Ok(canvas_io::LoadedImage {
        rgba: rgba.into_raw(),
        width: w,
        height: h,
    })
}
