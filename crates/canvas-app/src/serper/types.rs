//! Tipos de la búsqueda de imágenes web (Serper) compartidos por el cliente
//! (`api`), el filtro (`filter`), el estado del panel (`state`) y la UI
//! (`panel`/`card`): presupuesto de tokens, resultados y páginas.

/// Presupuesto de tokens por keyword, ajustado a lo que Serper cobra y
/// devuelve de verdad: 1 token = 1 llamada de ~10 imágenes (~1 crédito),
/// 2 tokens = 1 llamada de ~100 (~2 créditos), 3 tokens = hasta 2 llamadas
/// de ~100 (~4 créditos). Una keyword nueva resetea el gasto.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum TokenBudget {
    #[default]
    One,
    Two,
    Three,
}

impl TokenBudget {
    pub const ALL: [Self; 3] = [Self::One, Self::Two, Self::Three];

    /// Llamadas a la API (páginas) que autoriza este presupuesto.
    pub fn calls(self) -> u8 {
        match self {
            Self::One => 1,
            Self::Two => 1,
            Self::Three => 2,
        }
    }

    /// Resultados pedidos por llamada (`num` de Serper).
    pub fn num(self) -> u32 {
        match self {
            Self::One => super::api::SMALL_RESULTS,
            Self::Two | Self::Three => super::api::PER_CALL_RESULTS,
        }
    }

    /// Imágenes máximas que puede traer el presupuesto.
    pub fn max_images(self) -> u32 {
        match self {
            Self::One => super::api::SMALL_RESULTS,
            Self::Two => super::api::PER_CALL_RESULTS,
            Self::Three => 2 * super::api::PER_CALL_RESULTS,
        }
    }

    /// Créditos Serper estimados: ~10 cuesta 1, ~100 cuestan 2.
    pub fn credits(self) -> u8 {
        match self {
            Self::One => super::api::CREDITS_PER_SMALL_CALL,
            Self::Two => super::api::CREDITS_PER_CALL,
            Self::Three => 2 * super::api::CREDITS_PER_CALL,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::One => "1 token",
            Self::Two => "2 tokens",
            Self::Three => "3 tokens",
        }
    }

    /// Detalle para el selector: imágenes y créditos de un vistazo.
    pub fn detail(self) -> String {
        format!("~{} imgs · {}cr", self.max_images(), self.credits())
    }
}

/// Modo de la pestaña Web: imágenes generales o fotos de una persona en
/// Instagram/Facebook. Solo sesión (no se persiste en ajustes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchMode {
    #[default]
    Web,
    Social,
}

impl SearchMode {
    pub const ALL: [Self; 2] = [Self::Web, Self::Social];

    pub fn label(self) -> &'static str {
        match self {
            Self::Web => "Web Images",
            Self::Social => "Instagram/Facebook",
        }
    }

    /// En modo social se perdona el bloqueo de CDNs sociales (sus URLs
    /// caducan y a veces dan 403, pero son todo lo que hay).
    pub fn allow_social(self) -> bool {
        matches!(self, Self::Social)
    }
}

/// Red social de una búsqueda de persona.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SocialNetwork {
    Instagram,
    Facebook,
}

impl SocialNetwork {
    pub fn site(self) -> &'static str {
        match self {
            Self::Instagram => "instagram.com",
            Self::Facebook => "facebook.com",
        }
    }
}

/// Persona detectada en el modo social: link o nombre plano.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersonQuery {
    pub network: Option<SocialNetwork>,
    pub handle: String,
}

/// Una búsqueda web ya empaquetada para el worker: agrupa los siete
/// parámetros que viajarían sueltos (convención del repo, nada de
/// `#[allow(too_many_arguments)]`).
#[derive(Debug, Clone)]
pub struct SearchRequest {
    pub keyword: String,
    pub page: u32,
    pub num: u32,
    pub blocked: Vec<String>,
    pub mode: SearchMode,
    pub seq: u64,
}

/// Un resultado ya filtrado: lo mínimo que la UI necesita para mostrar la
/// miniatura, atribuir la fuente e insertar la imagen. El `id` ES la URL
/// directa (sirve de clave de dedupe entre páginas). Serde para la caché
/// de disco (las respuestas se guardan y re-sirven sin gastar créditos).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SerperPhoto {
    pub id: String,
    pub title: String,
    pub image_url: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub source_url: String,
    /// Miniatura proxy de Google, si Serper la trajo (`#[serde(default)]`
    /// para no romper cachés de disco antiguas).
    #[serde(default)]
    pub thumb_url: Option<String>,
}

impl SerperPhoto {
    /// Dominio de la página donde se encontró la imagen (para la
    /// atribución de la tarjeta y la etiqueta de la capa insertada).
    pub fn source_host(&self) -> String {
        super::filter::host_of(&self.source_url)
            .or_else(|| super::filter::host_of(&self.image_url))
            .unwrap_or_else(|| "web".to_owned())
    }

    /// URL a descargar para la miniatura: el proxy de Google si existe
    /// (rápido, fiable, tamaño de tarjeta), si no la original. La
    /// inserción y el bulk usan siempre `image_url` (+ `og:image`).
    pub fn thumb_source(&self) -> String {
        self.thumb_url
            .clone()
            .filter(|u| !u.trim().is_empty())
            .unwrap_or_else(|| self.image_url.clone())
    }
}

/// Una página de resultados ya resuelta y filtrada: las fotos, si era la
/// última página del servidor, el coste REAL cobrado por Serper (campo
/// `credits` de la respuesta) y la transparencia de la query.
#[derive(Debug, Clone)]
pub struct SearchPage {
    pub photos: Vec<SerperPhoto>,
    /// `true` si el servidor no tiene más páginas tras esta.
    pub reached_end: bool,
    /// Créditos que cobró esta llamada (fuente de verdad para el saldo).
    /// En caché es 0 (re-servida sin red).
    pub credits_charged: u32,
    /// Vino de la caché (memoria o disco): no gastó token ni créditos.
    pub from_cache: bool,
    pub effective_query: String,
    pub exclusions_applied: usize,
    pub exclusions_dropped: usize,
    /// La query viajó simplificada (cuenta gratuita: sin `-site:`/`tbs`).
    /// El filtro local sigue aplicándose igual.
    pub simplified: bool,
    pub filtered: super::filter::FilterCounts,
}
