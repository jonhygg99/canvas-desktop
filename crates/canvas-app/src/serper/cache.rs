//! Caché de respuestas de Serper para no gastar créditos en búsquedas
//! repetidas: la clave es (keyword normalizada + bloqueados + página). Un
//! hit se sirve con `credits_charged = 0` y `from_cache = true`, sin tocar
//! el presupuesto de tokens.
//!
//! Dos niveles: memoria (LRU de `MEM_CAP` páginas, vive en `Panel`) y disco
//! (JSON por clave con TTL de `DISK_TTL_SECS`, sobrevive a reinicios). Solo
//! se guardan URLs/títulos/dims (KBs); los thumbs se re-descargan perezosos
//! (no cuestan créditos Serper). Todo fallo de disco se ignora y se sigue
//! a red: la caché nunca rompe una búsqueda.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use super::filter::FilterCounts;
use super::types::{SearchPage, SerperPhoto};

/// Páginas en memoria (LRU simple por orden de inserción).
pub const MEM_CAP: usize = 20;
/// Validez de la caché de disco: las SERP cambian, 24h es el equilibrio
/// entre ahorro y frescura.
pub const DISK_TTL_SECS: u64 = 24 * 3600;
/// Variable de entorno para redirigir la caché (solo la usan los tests).
const CACHE_DIR_ENV: &str = "SERPER_CACHE_DIR";

/// Página guardada: lo mismo que `SearchPage` menos el coste (al re-servir
/// siempre es 0) más el instante de guardado para el TTL.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CachedPage {
    pub photos: Vec<SerperPhoto>,
    pub reached_end: bool,
    pub effective_query: String,
    pub exclusions_applied: usize,
    pub exclusions_dropped: usize,
    pub simplified: bool,
    pub filtered: FilterCounts,
    pub saved_at: u64,
}

impl CachedPage {
    /// De vuelta a página viva: coste 0 y bandera de caché para la UI.
    pub fn into_page(self) -> SearchPage {
        SearchPage {
            photos: self.photos,
            reached_end: self.reached_end,
            credits_charged: 0,
            from_cache: true,
            effective_query: self.effective_query,
            exclusions_applied: self.exclusions_applied,
            exclusions_dropped: self.exclusions_dropped,
            simplified: self.simplified,
            filtered: self.filtered,
        }
    }

    /// ¿Sigue fresca para servirse desde disco?
    pub fn fresh(&self, now: u64) -> bool {
        now.saturating_sub(self.saved_at) <= DISK_TTL_SECS
    }
}

impl From<&SearchPage> for CachedPage {
    fn from(page: &SearchPage) -> Self {
        Self {
            photos: page.photos.clone(),
            reached_end: page.reached_end,
            effective_query: page.effective_query.clone(),
            exclusions_applied: page.exclusions_applied,
            exclusions_dropped: page.exclusions_dropped,
            simplified: page.simplified,
            filtered: page.filtered,
            saved_at: now_secs(),
        }
    }
}

/// Clave estable: keyword en minúsculas + bloqueados ordenados + página +
/// tamaño pedido + modo (una misma keyword en web y en social trae tandas
/// distintas). Ni el orden de los bloqueados ni las mayúsculas cambian la
/// clave.
pub fn cache_key(
    keyword: &str,
    blocked: &[String],
    page: u32,
    num: u32,
    mode: super::types::SearchMode,
) -> String {
    let mut blocked: Vec<String> = blocked
        .iter()
        .map(|b| b.trim().to_lowercase())
        .filter(|b| !b.is_empty())
        .collect();
    blocked.sort();
    blocked.dedup();
    let mode_tag = match mode {
        super::types::SearchMode::Web => "web",
        super::types::SearchMode::Social => "social",
    };
    let raw = format!(
        "{}|{}|{page}|{num}|{mode_tag}",
        keyword.trim().to_lowercase(),
        blocked.join(",")
    );
    format!("{:016x}", fnv1a(&raw))
}

/// FNV-1a de 64 bits (determinista entre ejecuciones).
fn fnv1a(s: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Directorio de la caché de disco (`None` si no se puede resolver).
pub fn cache_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var(CACHE_DIR_ENV)
        .ok()
        .filter(|s| !s.trim().is_empty())
    {
        return Some(PathBuf::from(dir));
    }
    directories::ProjectDirs::from("com", "canvas-desktop", "Canvas Desktop")
        .map(|d| d.cache_dir().join("serper_search"))
}

/// Lee una página de disco si existe y está fresca.
pub fn load_disk(key: &str) -> Option<CachedPage> {
    let path = cache_dir()?.join(format!("{key}.json"));
    let bytes = std::fs::read(&path).ok()?;
    let page: CachedPage = serde_json::from_slice(&bytes).ok()?;
    page.fresh(now_secs()).then_some(page)
}

/// Guarda una página en disco (best-effort: nunca falla la búsqueda).
pub fn save_disk(key: &str, page: &SearchPage) {
    let Some(dir) = cache_dir() else {
        return;
    };
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let cached = CachedPage::from(page);
    if let Ok(bytes) = serde_json::to_vec(&cached) {
        let _ = std::fs::write(dir.join(format!("{key}.json")), bytes);
    }
}

/// Inserta con tope LRU en un mapa + orden de inserción.
pub fn lru_insert(
    map: &mut HashMap<String, CachedPage>,
    order: &mut Vec<String>,
    key: String,
    page: CachedPage,
) {
    if let Some(pos) = order.iter().position(|k| k == &key) {
        order.remove(pos);
    }
    order.push(key.clone());
    map.insert(key, page);
    while order.len() > MEM_CAP {
        let oldest = order.remove(0);
        map.remove(&oldest);
    }
}
