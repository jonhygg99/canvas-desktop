//! Estado del panel «Web» que vive en `EditorState`: consulta, presupuesto
//! de tokens, resultados y el arrastre en curso hacia el lienzo.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::Sender;

use eframe::egui;

use crate::loader;

use super::cache::{lru_insert, CachedPage, MEM_CAP};
use super::filter::FilterCounts;
use super::types::{SearchPage, SerperPhoto, TokenBudget};

/// Tarjetas que revela cada pulsación de «Show more» local (sin coste: ya
/// están descargadas, solo se muestran).
pub const SHOW_STEP: usize = 24;
/// Sin respuesta de búsqueda en este tiempo, el vuelo se da por perdido
/// (ver `stalled`): el Done/resultado se perdió al cambiar de vista o el
/// hilo murió sin avisar.
pub const SEARCH_STALL_SECS: u64 = 120;
/// Sin progreso del bulk en este tiempo, el vuelo se da por perdido.
pub const BULK_STALL_SECS: u64 = 300;

/// ¿Lleva `since` esperando más de `timeout_secs`? Puro y testeable: el
/// watchdog de la UI lo usa para ofrecer Reset en vez de un spinner eterno.
pub fn stalled(
    since: Option<std::time::Instant>,
    now: std::time::Instant,
    timeout_secs: u64,
) -> bool {
    since.is_some_and(|t| now.duration_since(t).as_secs() >= timeout_secs)
}

/// Estado del panel «Web» del sidebar del editor: consulta, presupuesto de
/// tokens, resultados (con sus miniaturas ya subidas a GPU) y errores.
/// Vive en `EditorState`; el gasto (`tokens_spent`) es de sesión y la lista
/// de bloqueados y el presupuesto viven en `AppSettings`.
#[derive(Default)]
pub struct Panel {
    pub query: String,
    /// Presupuesto elegido (1–3 tokens); espejo de `AppSettings`.
    pub budget: TokenBudget,
    /// Llamadas gastadas en la keyword actual (1 token = 1 llamada).
    pub tokens_spent: u8,
    /// Última página cargada (1-based).
    pub page: u32,
    /// Hay una búsqueda o descarga de lote en vuelo (desactiva la UI).
    pub searching: bool,
    /// Cuándo arrancó la búsqueda en vuelo (watchdog anti-bloqueo).
    pub search_started: Option<std::time::Instant>,
    pub photos: Vec<PhotoItem>,
    pub error: Option<String>,
    /// El servidor no tiene más páginas tras esta…
    pub reached_end: bool,
    /// …o se agotó el presupuesto para esta keyword.
    pub budget_exhausted: bool,
    /// Id de la foto cuya imagen completa se está descargando para insertar.
    pub inserting: Option<String>,
    /// Foto arrastrada y soltada sobre el lienzo: su id y la posición de
    /// página donde debe caer. Se consume en `on_serper_image_ready`; si es
    /// `None`, el clic inserta centrada.
    pub pending_drop: Option<(String, (f64, f64))>,
    /// Contador de búsquedas lanzadas: descarta respuestas caducas.
    pub search_seq: u64,
    /// Transparencia de la última búsqueda (query efectiva y filtro).
    pub last_query: String,
    pub exclusions_applied: usize,
    pub exclusions_dropped: usize,
    /// La última búsqueda viajó simplificada (cuenta gratuita).
    pub query_simplified: bool,
    /// Coste REAL de la última llamada (campo `credits` de Serper).
    pub credits_last: u32,
    /// Créditos reales gastados en esta sesión (todas las keywords).
    pub credits_session: u64,
    /// La última página vino de la caché (memoria o disco): insignia «cached».
    pub cached_badge: bool,
    /// Respuestas guardadas en memoria (sin gastar al repetir).
    pub search_cache: HashMap<String, CachedPage>,
    /// Orden de inserción de la caché (tope LRU `MEM_CAP`).
    pub cache_order: Vec<String>,
    pub filtered: FilterCounts,
    /// Banners cazados tras decodificar el thumb (la API no traía dims).
    pub filtered_post: usize,
    /// Tarjetas visibles (paginación local sin coste).
    pub visible_count: usize,
    /// Borrador multilínea (un dominio por línea) del Advanced.
    pub blocked_text: String,
    /// Ya se sincronizó con `AppSettings` al menos una vez.
    pub synced_settings: bool,
    /// Ventana de selección masiva abierta.
    pub bulk_open: bool,
    /// Ids seleccionados en la ventana masiva.
    pub bulk_selected: HashSet<String>,
    /// Guardado masivo en vuelo (hilo worker).
    pub bulk_busy: bool,
    /// Cuándo llegó el último progreso del bulk (watchdog anti-bloqueo).
    pub bulk_progress_at: Option<std::time::Instant>,
    /// Progreso del guardado masivo (hechas, total).
    pub bulk_progress: Option<(usize, usize)>,
    /// Fallos del último guardado masivo (una línea por imagen).
    pub bulk_errors: Vec<String>,
    /// Mensaje de éxito del último guardado masivo.
    pub bulk_done_msg: Option<String>,
}

/// Un resultado con su miniatura (si ya llegó del worker). El thumb se pide
/// de forma perezosa la primera vez que la tarjeta se pinta
/// (`thumb_requested`): lo no visible no descarga nada. Si falla,
/// `thumb_error` guarda el motivo para mostrarlo y reintentarlo.
pub struct PhotoItem {
    pub photo: SerperPhoto,
    pub thumb: Option<egui::TextureHandle>,
    pub thumb_error: Option<String>,
    pub thumb_requested: bool,
}

impl PhotoItem {
    /// Reclama la descarga del thumb si hace falta (una sola vez): `true`
    /// si el llamador debe lanzar `spawn_serper_thumb`.
    pub fn claim_thumb(&mut self) -> bool {
        if self.thumb.is_none() && self.thumb_error.is_none() && !self.thumb_requested {
            self.thumb_requested = true;
            true
        } else {
            false
        }
    }

    /// Prepara un reintento tras un fallo (lo usa el botón Retry).
    pub fn retry_thumb(&mut self) {
        self.thumb_error = None;
        self.thumb_requested = false;
    }
}

/// Payload del arrastre de una foto web hacia el lienzo: lo que el canvas
/// necesita para lanzar la descarga si la sueltan sobre él.
#[derive(Clone)]
pub struct DragSerper {
    pub id: String,
    pub label: String,
    pub url: String,
}

impl Panel {
    /// ¿Se puede gastar un token más en la keyword actual?
    pub fn can_spend_more(&self) -> bool {
        !self.searching && self.tokens_spent < self.budget.calls()
    }

    /// Limpia todo vuelo pendiente (búsqueda y bulk): lo usa el botón Reset
    /// del watchdog y cualquier arranque nuevo. Sube `search_seq` para que
    /// las respuestas tardías lleguen como caducas y se ignoren solas.
    pub fn reset_flight(&mut self) {
        self.searching = false;
        self.search_started = None;
        self.bulk_busy = false;
        self.bulk_progress = None;
        self.bulk_progress_at = None;
        self.search_seq += 1;
    }

    /// Pinta el aviso de vuelo atascado con botón Reset si `stuck` es
    /// verdad; devuelve `true` si lo pintó (el llamador omite el spinner).
    /// El Reset limpia los flags y las respuestas tardías se ignoran solas.
    pub fn stalled_ui(&mut self, stuck: bool, ui: &mut egui::Ui) -> bool {
        if !stuck {
            return false;
        }
        ui.colored_label(
            ui.visuals().error_fg_color,
            "No response for a while — the reply was likely lost.",
        );
        if ui.button("Reset").clicked() {
            self.reset_flight();
        }
        true
    }

    /// Abre la ventana masiva con TODO seleccionado por defecto.
    pub fn open_bulk(&mut self) {
        self.bulk_open = true;
        self.bulk_done_msg = None;
        self.bulk_errors.clear();
        self.bulk_selected = self.photos.iter().map(|p| p.photo.id.clone()).collect();
    }

    /// Marca todo / desmarca todo en la ventana masiva.
    pub fn select_all_bulk(&mut self) {
        self.bulk_selected = self.photos.iter().map(|p| p.photo.id.clone()).collect();
    }

    pub fn deselect_all_bulk(&mut self) {
        self.bulk_selected.clear();
    }

    /// Reclama hasta `budget` thumbs pendientes (tope por frame del bulk):
    /// marca y devuelve (id, url) para lanzar.
    pub fn claim_thumbs(&mut self, budget: usize) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for item in self.photos.iter_mut() {
            if out.len() >= budget {
                break;
            }
            if item.claim_thumb() {
                out.push((item.photo.id.clone(), item.photo.image_url.clone()));
            }
        }
        out
    }

    /// Busca una página en la caché de memoria (sin gastar nada).
    pub fn cache_lookup(&self, key: &str) -> Option<SearchPage> {
        self.search_cache
            .get(key)
            .cloned()
            .map(CachedPage::into_page)
    }

    /// Guarda una página de red en la caché de memoria (tope LRU).
    pub fn cache_insert(&mut self, key: String, page: &SearchPage) {
        lru_insert(
            &mut self.search_cache,
            &mut self.cache_order,
            key,
            CachedPage::from(page),
        );
        debug_assert!(self.search_cache.len() <= MEM_CAP);
    }

    /// Aplica una página (de red o de caché) al panel: reinicia en la 1,
    /// añade sin duplicados y actualiza transparencia y filtro. El gasto
    /// en créditos lo lleva el llamador (la caché no gasta).
    pub fn apply_page(&mut self, page_result: &SearchPage, page: u32) {
        // Solo la primera página reinicia; «Bring more» añade.
        if page == 1 {
            self.photos.clear();
            self.bulk_selected.clear();
            self.bulk_done_msg = None;
            self.bulk_errors.clear();
        }
        let existing: HashSet<String> = self.photos.iter().map(|p| p.photo.id.clone()).collect();
        for photo in &page_result.photos {
            if existing.contains(&photo.id) {
                continue;
            }
            self.photos.push(PhotoItem {
                photo: photo.clone(),
                thumb: None,
                thumb_error: None,
                thumb_requested: false,
            });
        }
        self.error = None;
        self.reached_end = page_result.reached_end;
        self.budget_exhausted =
            !page_result.reached_end && self.tokens_spent >= self.budget.calls();
        self.last_query = page_result.effective_query.clone();
        self.exclusions_applied = page_result.exclusions_applied;
        self.exclusions_dropped = page_result.exclusions_dropped;
        if page == 1 {
            self.query_simplified = page_result.simplified;
        }
        self.cached_badge = page_result.from_cache;
        self.filtered.add_page(page_result.filtered);
        if self.photos.is_empty() {
            self.error = Some("No results after filtering".to_owned());
        }
    }

    /// Fotos elegidas en el orden de la lista (para nombres estables en el
    /// bulk). Lleva las dims de la API para dimensionar sin descargar.
    pub fn selected_bulk_items(&self) -> Vec<loader::BulkItem> {
        self.photos
            .iter()
            .filter(|p| self.bulk_selected.contains(&p.photo.id))
            .map(|p| loader::BulkItem {
                url: p.photo.image_url.clone(),
                label: format!("Web · {}", p.photo.source_host()),
                width: p.photo.width,
                height: p.photo.height,
            })
            .collect()
    }

    /// Una foto web se ha soltado sobre el lienzo en `page_pos`: recuerda
    /// el destino y lanza la descarga, igual que el clic. No hace nada si
    /// otra descarga ya está en vuelo.
    pub fn drop_on_canvas(
        &mut self,
        payload: DragSerper,
        page_pos: (f64, f64),
        tx: &Sender<loader::AppMsg>,
        ctx: &egui::Context,
    ) {
        if self.inserting.is_some() {
            return;
        }
        self.inserting = Some(payload.id.clone());
        self.pending_drop = Some((payload.id.clone(), page_pos));
        loader::spawn_serper_image(
            payload.id,
            payload.label,
            payload.url,
            tx.clone(),
            ctx.clone(),
        );
    }
}
