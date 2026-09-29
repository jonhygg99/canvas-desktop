//! Filtro de resultados de Serper: funciones puras sin red ni UI que
//! deciden qué imágenes sobreviven. Se aplica en el hilo worker antes de
//! responder (con las dimensiones de la API) y se remata tras decodificar
//! el thumb (con las dimensiones reales, por si la API no las traía).
//!
//! También construye la query con exclusiones `-site:` sin superar nunca
//! el tope de palabras que Serper acepta.

use super::types::SerperPhoto;

/// Palabras máximas de la query completa (keyword + exclusiones): por
/// encima Serper la rechaza, así que se recortan exclusiones por la cola.
pub const MAX_QUERY_WORDS: usize = 30;
/// Proporción a partir de la cual una imagen es un banner y se descarta.
pub const MAX_ASPECT: u64 = 3;

/// Exclusiones `-site:` base de la query (stock con marca de agua, bancos
/// gratuitos que devuelven duplicados y portfolios que suelen dar 403).
fn base_query_exclusions() -> &'static [&'static str] {
    &[
        "shutterstock.com",
        "gettyimages.com",
        "istockphoto.com",
        "alamy.com",
        "stock.adobe.com",
        "dreamstime.com",
        "depositphotos.com",
        "123rf.com",
        "agefotostock.com",
        "pexels.com",
        "unsplash.com",
        "pixabay.com",
        "freepik.com",
        "vecteezy.com",
        "deviantart.com",
        "behance.net",
        "dribbble.com",
    ]
}

/// Dominios bloqueados por defecto en el filtro (el host de `imageUrl`
/// basta con que CONTENGA uno): la lista base de la query más clipart,
/// IA generativa, sociales que dan 403 y resto de basura visual. Es el
/// valor inicial de `AppSettings::serper_blocked`; el usuario lo edita en
/// el Advanced del panel.
pub fn default_blocked_domains() -> Vec<String> {
    let extra = [
        // Stock con marca de agua (resto).
        "gettyimages.es",
        "canstockphoto.com",
        "bigstockphoto.com",
        "pond5.com",
        "envato.com",
        "stockcake.com",
        "fotosearch.com",
        "stocksy.com",
        "masterfile.com",
        // Clipart/vectores.
        "vectorstock.com",
        "pngtree.com",
        "cleanpng.com",
        "pngitem.com",
        "pngegg.com",
        "kindpng.com",
        "clipart-library.com",
        "clipartmax.com",
        "hiclipart.com",
        "iconfinder.com",
        // IA generativa.
        "stablediffusionweb.com",
        "midjourney.com",
        "playgroundai.com",
        "leonardo.ai",
        "civitai.com",
        "openart.ai",
        // Sociales (403 al descargar).
        "tiktok.com",
        "lookaside.instagram.com",
        "fbsbx.com",
        // Basura visual.
        "ytimg.com",
        "media-amazon.com",
        "wbm.im",
        "media.licdn.com",
    ];
    base_query_exclusions()
        .iter()
        .map(ToString::to_string)
        .chain(extra.iter().map(ToString::to_string))
        .collect()
}

/// Patrones en la URL que delatan marca de agua, preview o imagen de IA.
fn blocked_url_patterns() -> &'static [&'static str] {
    &[
        "thumbs.dreamstime.",
        "previews.123rf.",
        "static.vecteezy.",
        "/watermark/",
        "/comp_image/",
        "ai-generated",
        "ai_generated",
    ]
}

/// Términos en el título que delatan infografía, vector, mockup o render.
fn blocked_title_terms() -> &'static [&'static str] {
    &[
        "infographic",
        "infografía",
        "clipart",
        "vector illustration",
        "vector art",
        "stock vector",
        "royalty free",
        "seamless pattern",
        "icon set",
        "flat icon",
        "flat design",
        "coloring page",
        "mockup",
        "hd wallpaper",
        "transparent background",
        "transparent png",
        "cartoon illustration",
        "3d render",
        "powerpoint template",
    ]
}

/// Query efectiva con sus cuentas de transparencia: cuántas exclusiones se
/// aplicaron y cuántas se recortaron por el tope de palabras.
pub struct BuiltQuery {
    pub query: String,
    pub applied: usize,
    pub dropped: usize,
}

/// Construye `{keyword} -site:a -site:b …` uniendo la base con los
/// bloqueados del usuario (sin duplicados). Si supera `MAX_QUERY_WORDS`,
/// recorta exclusiones por la cola hasta entrar; la keyword manda y nunca
/// se recorta.
pub fn build_query(keyword: &str, blocked: &[String]) -> BuiltQuery {
    let keyword = keyword.trim();
    let mut exclusions: Vec<String> = Vec::new();
    for site in base_query_exclusions()
        .iter()
        .map(ToString::to_string)
        .chain(blocked.iter().cloned())
    {
        let site = site.trim().to_lowercase();
        if site.is_empty() || exclusions.contains(&site) {
            continue;
        }
        exclusions.push(site);
    }
    let words = keyword.split_whitespace().count();
    let mut keep = exclusions.len();
    while keep > 0 && words + keep > MAX_QUERY_WORDS {
        keep -= 1;
    }
    let mut query = keyword.to_owned();
    for site in exclusions.iter().take(keep) {
        query.push_str(" -site:");
        query.push_str(site);
    }
    BuiltQuery {
        query,
        applied: keep,
        dropped: exclusions.len() - keep,
    }
}

/// Host en minúsculas de una URL (`None` si no hay nada parecido a un
/// host). Sin el crate `url` a propósito: solo se necesita el host para
/// el `contains` del filtro.
pub fn host_of(url: &str) -> Option<String> {
    let after_scheme = url.rsplit("://").next()?;
    let host = after_scheme
        .split(['/', '?', '#'])
        .next()?
        .rsplit('@')
        .next()?;
    let host = host.split(':').next()?;
    let host = host.trim().to_lowercase();
    (!host.is_empty()).then_some(host)
}

/// ¿La extensión del path (sin query ni fragmento) es foto real?
/// Un `.png` puede ser una foto real: no se descarta por la extensión.
pub fn extension_allowed(url: &str) -> bool {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let ext = path.rsplit('.').next().unwrap_or("").to_lowercase();
    matches!(ext.as_str(), "jpg" | "jpeg" | "png" | "webp")
}

/// ¿El host de `imageUrl` contiene algún dominio bloqueado?
pub fn host_blocked(image_url: &str, blocked: &[String]) -> bool {
    let Some(host) = host_of(image_url) else {
        return true;
    };
    blocked
        .iter()
        .any(|b| !b.trim().is_empty() && host.contains(&b.trim().to_lowercase()))
}

/// ¿La URL contiene algún patrón de marca de agua/preview/IA?
pub fn url_pattern_blocked(url: &str) -> bool {
    let lower = url.to_lowercase();
    blocked_url_patterns().iter().any(|p| lower.contains(p))
}

/// ¿El título contiene algún término de infografía/vector/mockup?
pub fn title_blocked(title: &str) -> bool {
    let lower = title.to_lowercase();
    blocked_title_terms().iter().any(|t| lower.contains(t))
}

/// ¿La proporción es de banner? `false` si faltan dimensiones (sin datos
/// no se juzga: se conserva y decide el segundo filtro tras el decode).
/// Sin flotantes: `grande >= 3 * pequeña`.
pub fn ratio_blocked(width: u32, height: u32) -> bool {
    if width == 0 || height == 0 {
        return false;
    }
    let (big, small) = if width >= height {
        (width, height)
    } else {
        (height, width)
    };
    u64::from(big) >= MAX_ASPECT * u64::from(small)
}

/// Motivo por el que una foto no sobrevive (para el contador del pie).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropReason {
    Extension,
    Host,
    Pattern,
    Title,
    Ratio,
}

/// Clasifica una foto: `None` = pasa el filtro. Orden barato primero
/// (extensión) y caro al final (proporción).
pub fn classify(photo: &SerperPhoto, blocked: &[String]) -> Option<DropReason> {
    if !extension_allowed(&photo.image_url) {
        return Some(DropReason::Extension);
    }
    if host_blocked(&photo.image_url, blocked) {
        return Some(DropReason::Host);
    }
    if url_pattern_blocked(&photo.image_url) {
        return Some(DropReason::Pattern);
    }
    if title_blocked(&photo.title) {
        return Some(DropReason::Title);
    }
    let (w, h) = (photo.width.unwrap_or(0), photo.height.unwrap_or(0));
    if ratio_blocked(w, h) {
        return Some(DropReason::Ratio);
    }
    None
}

/// Cuántas fotos cayeron por cada motivo (pie del panel). Serde para la
/// caché de disco.
#[derive(Debug, Clone, Copy, Default, serde::Serialize, serde::Deserialize)]
pub struct FilterCounts {
    pub extension: usize,
    pub host: usize,
    pub pattern: usize,
    pub title: usize,
    pub ratio: usize,
}

impl FilterCounts {
    pub fn total(self) -> usize {
        self.extension + self.host + self.pattern + self.title + self.ratio
    }

    fn add(&mut self, reason: DropReason) {
        match reason {
            DropReason::Extension => self.extension += 1,
            DropReason::Host => self.host += 1,
            DropReason::Pattern => self.pattern += 1,
            DropReason::Title => self.title += 1,
            DropReason::Ratio => self.ratio += 1,
        }
    }

    /// Suma las cuentas de otra página (acumulado por keyword).
    pub fn add_page(&mut self, other: FilterCounts) {
        self.extension += other.extension;
        self.host += other.host;
        self.pattern += other.pattern;
        self.title += other.title;
        self.ratio += other.ratio;
    }
}

/// Parte la tanda en supervivientes y cuentas de descarte.
pub fn apply_filter(
    photos: Vec<SerperPhoto>,
    blocked: &[String],
) -> (Vec<SerperPhoto>, FilterCounts) {
    let mut kept = Vec::with_capacity(photos.len());
    let mut counts = FilterCounts::default();
    for photo in photos {
        match classify(&photo, blocked) {
            None => kept.push(photo),
            Some(reason) => counts.add(reason),
        }
    }
    (kept, counts)
}
