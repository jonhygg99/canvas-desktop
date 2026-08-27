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

use std::io::Read;
use std::sync::mpsc::Sender;
use std::time::Duration;

use eframe::egui;

use crate::editor::EditorState;
use crate::loader;

/// Variable de entorno con la Access Key de la API de Unsplash.
pub const ACCESS_KEY_ENV: &str = "UNSPLASH_ACCESS_KEY";

const SEARCH_URL: &str = "https://api.unsplash.com/search/photos";
const PER_PAGE: u32 = 30;
/// Margen lateral (en puntos) a cada lado de las tarjetas de foto en la
/// lista: las imágenes quedan un poco más estrechas que el panel.
const CARD_INSET: f32 = 12.0;

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

/// Lee la Access Key del entorno; `None` si no está definida (o está vacía).
pub fn access_key() -> Option<String> {
    std::env::var(ACCESS_KEY_ENV)
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
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

/// Envuelve el JSON de `/search/photos`: resultados, errores y el total de
/// páginas (para saber cuándo «Load more» ya no tiene más resultados).
#[derive(Debug, serde::Deserialize)]
struct SearchResponse {
    #[serde(default)]
    results: Vec<Photo>,
    #[serde(default)]
    errors: Vec<String>,
    /// Páginas disponibles en total; `None` si la respuesta no lo trae.
    #[serde(default)]
    total_pages: Option<u32>,
}

/// Una página de resultados ya resuelta: las fotos y si esta era la última
/// página (para ocultar «Load more» y avisar del final).
#[derive(Debug, Clone)]
pub struct SearchPage {
    pub photos: Vec<Photo>,
    /// `true` si no hay más páginas tras esta (fin de los resultados).
    pub reached_end: bool,
}

/// Agente HTTP con tiempos de espera acotados: una red colgada no puede
/// bloquear un hilo worker para siempre.
fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(Duration::from_secs(30))
        .timeout_write(Duration::from_secs(30))
        .build()
}

/// Busca fotos en Unsplash con los filtros dados. `page` es 1-based;
/// devuelve hasta `PER_PAGE` resultados y si esta era la última página.
/// Solo se llama desde hilos worker.
pub fn search(query: &str, page: u32, filters: SearchFilters) -> Result<SearchPage, String> {
    let Some(key) = access_key() else {
        return Err(format!("{ACCESS_KEY_ENV} is not set"));
    };
    let query = query.trim();
    if query.is_empty() {
        return Ok(SearchPage {
            photos: Vec::new(),
            reached_end: true,
        });
    }
    let mut req = agent()
        .get(SEARCH_URL)
        .query("query", query)
        .query("page", &page.to_string())
        .query("per_page", &PER_PAGE.to_string())
        .query("client_id", &key);
    // Solo se mandan los filtros activos: la API ignora (o falla) valores
    // vacíos, así que un filtro «Any» simplemente no se envía.
    if let Some(o) = filters.orientation.as_str() {
        req = req.query("orientation", o);
    }
    if let Some(c) = filters.color.as_str() {
        req = req.query("color", c);
    }
    req = req.query("order_by", filters.order_by.as_str());
    let resp = req.call().map_err(|e| format!("Unsplash search failed: {e}"))?;
    let body = resp
        .into_string()
        .map_err(|e| format!("Unsplash search failed: {e}"))?;
    let parsed: SearchResponse =
        serde_json::from_str(&body).map_err(|e| format!("Unsplash returned an invalid response: {e}"))?;
    if let Some(err) = parsed.errors.first() {
        return Err(err.clone());
    }
    Ok(SearchPage {
        reached_end: reached_end(parsed.total_pages, page, parsed.results.len()),
        photos: parsed.results,
    })
}

/// ¿Esta página era la última? O la API dice que `page` alcanzó
/// `total_pages`, o devolvió menos fotos de las que caben en una página
/// (defensa si el campo `total_pages` falta en la respuesta).
fn reached_end(total_pages: Option<u32>, page: u32, returned: usize) -> bool {
    total_pages.is_some_and(|t| page >= t) || returned < PER_PAGE as usize
}

/// Descarga el contenido de una URL (miniatura o imagen completa). Solo se
/// llama desde hilos worker.
pub fn download(url: &str) -> Result<Vec<u8>, String> {
    let resp = agent()
        .get(url)
        .call()
        .map_err(|e| format!("Unsplash download failed: {e}"))?;
    let mut bytes = Vec::new();
    resp.into_reader()
        .read_to_end(&mut bytes)
        .map_err(|e| format!("Unsplash download failed: {e}"))?;
    if bytes.is_empty() {
        return Err("Unsplash returned an empty image".to_owned());
    }
    Ok(bytes)
}

/// Decodifica bytes (PNG/JPEG/WebP…) a `LoadedImage` RGBA8, listo para
/// `add_image_layer` o para una textura de egui.
pub fn decode(bytes: &[u8]) -> Result<canvas_io::LoadedImage, String> {
    let img = image::load_from_memory(bytes).map_err(|e| format!("Image decode failed: {e}"))?;
    let rgba = img.to_rgba8();
    let (w, h) = (rgba.width(), rgba.height());
    Ok(canvas_io::LoadedImage {
        rgba: rgba.into_raw(),
        width: w,
        height: h,
    })
}

/// Estado del panel «Images» del sidebar del editor: consulta, filtros,
/// resultados (con sus miniaturas ya subidas a GPU) y errores. Vive en
/// `EditorState`.
#[derive(Default)]
pub struct Panel {
    pub query: String,
    /// Filtros activos de la búsqueda (orientación, color, orden).
    pub filters: SearchFilters,
    /// Última página cargada (1-based).
    pub page: u32,
    /// Hay una búsqueda o descarga de lote en vuelo (desactiva la UI).
    pub searching: bool,
    pub photos: Vec<PhotoItem>,
    pub error: Option<String>,
    /// Se llegó a la última página: «Load more» desaparece y se muestra un
    /// aviso de fin de resultados.
    pub reached_end: bool,
    /// Id de la foto cuya imagen completa se está descargando para insertar.
    pub inserting: Option<String>,
    /// Foto de Unsplash arrastrada y soltada sobre el lienzo: su id y la
    /// posición de página donde debe caer. Se consume en
    /// `on_unsplash_image_ready`; si es `None`, el clic inserta centrada.
    pub pending_drop: Option<(String, (f64, f64))>,
    /// Contador de búsquedas lanzadas: descarta respuestas caducas (llegarían
    /// con los filtros/consulta anteriores).
    pub search_seq: u64,
}

/// Payload del arrastre de una foto de Unsplash hacia el lienzo: lo que el
/// canvas necesita para lanzar la descarga si la sueltan sobre él. Viaja por
/// el drag & drop de egui (`dnd_drag_source` → `dnd_release_payload`).
#[derive(Clone)]
pub struct DragUnsplash {
    pub id: String,
    pub label: String,
    pub url: String,
}

impl Panel {
    /// Una foto de Unsplash se ha soltado sobre el lienzo en `page_pos`:
    /// recuerda el destino (para que `on_unsplash_image_ready` la coloque
    /// ahí en vez de centrada) y lanza la descarga, igual que el clic.
    /// No hace nada si otra descarga ya está en vuelo.
    pub fn drop_on_canvas(
        &mut self,
        payload: DragUnsplash,
        page_pos: (f64, f64),
        tx: &Sender<loader::AppMsg>,
        ctx: &egui::Context,
    ) {
        if self.inserting.is_some() {
            return;
        }
        self.inserting = Some(payload.id.clone());
        self.pending_drop = Some((payload.id.clone(), page_pos));
        loader::spawn_unsplash_image(
            payload.id,
            payload.label,
            payload.url,
            tx.clone(),
            ctx.clone(),
        );
    }
}

/// Lanza una búsqueda nueva (página 1) con la consulta y filtros actuales.
/// No hace nada si ya hay una en vuelo o la consulta está vacía.
fn start_search(panel: &mut Panel, tx: &Sender<loader::AppMsg>, ctx: &egui::Context) {
    if panel.searching || panel.query.trim().is_empty() {
        return;
    }
    panel.search_seq += 1;
    panel.searching = true;
    panel.page = 1;
    panel.photos.clear();
    panel.error = None;
    panel.reached_end = false;
    panel.pending_drop = None;
    loader::spawn_unsplash_search(
        panel.query.trim().to_owned(),
        panel.filters,
        panel.search_seq,
        1,
        tx.clone(),
        ctx.clone(),
    );
}

/// Pide la siguiente página de la búsqueda actual («Load more»).
fn load_more(panel: &mut Panel, tx: &Sender<loader::AppMsg>, ctx: &egui::Context) {
    if panel.searching || panel.query.trim().is_empty() {
        return;
    }
    panel.search_seq += 1;
    panel.searching = true;
    panel.page += 1;
    loader::spawn_unsplash_search(
        panel.query.trim().to_owned(),
        panel.filters,
        panel.search_seq,
        panel.page,
        tx.clone(),
        ctx.clone(),
    );
}

/// Fila de filtros (orientación, orden y color). Devuelve `true` si algún
/// filtro cambió para relanzar la búsqueda.
fn filters_ui(panel: &mut Panel, ui: &mut egui::Ui) -> bool {
    let mut changed = false;

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        for o in Orientation::ALL {
            if ui
                .selectable_label(panel.filters.orientation == o, o.label())
                .clicked()
            {
                panel.filters.orientation = o;
                changed = true;
            }
        }
    });

    ui.add_space(2.0);
    // Orden y color en la misma fila (envuelve si no caben).
    ui.horizontal_wrapped(|ui| {
        for ob in OrderBy::ALL {
            if ui
                .selectable_label(panel.filters.order_by == ob, ob.label())
                .clicked()
            {
                panel.filters.order_by = ob;
                changed = true;
            }
        }
        ui.separator();
        for c in ColorFilter::ALL {
            let is_sel = panel.filters.color == c;
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::click());
            let fill = c.swatch().unwrap_or(ui.visuals().faint_bg_color);
            ui.painter().circle_filled(rect.center(), 6.5, fill);
            if is_sel {
                ui.painter().circle_stroke(
                    rect.center(),
                    8.5,
                    egui::Stroke::new(2.0, ui.visuals().strong_text_color()),
                );
            }
            if resp.clicked() {
                panel.filters.color = c;
                changed = true;
            }
            let _ = resp.on_hover_text(c.label());
        }
    });

    changed
}

/// Un resultado con su miniatura (si ya llegó del worker).
pub struct PhotoItem {
    pub photo: Photo,
    pub thumb: Option<egui::TextureHandle>,
    pub thumb_failed: bool,
}

/// Botón «Load more» a todo lo ancho de la lista: píldora con borde sutil,
/// un chevron simple hacia abajo + texto centrados como una unidad (ambos
/// centrados verticalmente), y hover con fondo más claro — el mismo
/// lenguaje visual que el resto de botones de la app.
fn load_more_button_ui(ui: &mut egui::Ui, w: f32) -> egui::Response {
    let visuals = ui.visuals().clone();
    let font = egui::FontId::proportional(13.0);
    let text = "Load more";
    let icon_sz = 10.0;
    let gap = 7.0;
    let h = 30.0;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::click());
    let hovered = resp.hovered();
    let bg = if hovered {
        visuals.widgets.hovered.bg_fill
    } else {
        visuals.widgets.inactive.bg_fill
    };
    ui.painter().rect(
        rect,
        h * 0.5,
        bg,
        visuals.widgets.inactive.bg_stroke,
        egui::StrokeKind::Inside,
    );
    let color = if hovered {
        visuals.widgets.active.text_color()
    } else {
        visuals.strong_text_color()
    };
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font.clone(), color);
    // Icono + texto como una unidad centrada en la píldora; el texto se
    // centra también en vertical (su esquina NO va al centro del botón).
    let total = icon_sz + gap + galley.size().x;
    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(rect.center().x - total * 0.5 + icon_sz * 0.5, rect.center().y),
        egui::vec2(icon_sz, icon_sz),
    );
    crate::app_icons::draw_triangle_icon(
        ui.painter(),
        icon_rect,
        crate::app_icons::IconDir::Down,
        color,
    );
    ui.painter().galley(
        egui::pos2(
            icon_rect.right() + gap,
            rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        color,
    );
    resp.on_hover_text("Load the next page of photos")
}

/// Contenido de la pestaña «Images» del panel lateral izquierdo.
pub fn panel_ui(state: &mut EditorState, ui: &mut egui::Ui, tx: &Sender<loader::AppMsg>) {
    if access_key().is_none() {
        ui.add_space(8.0);
        ui.label(format!("{ACCESS_KEY_ENV} is not set"));
        ui.add_space(4.0);
        ui.weak("Get a free key at unsplash.com/developers and add it\nto the project .env as UNSPLASH_ACCESS_KEY,\nthen restart the app.");
        return;
    }
    let panel = &mut state.unsplash;

    ui.add_space(6.0);
    let mut do_search = false;
    ui.horizontal(|ui| {
        let width = (ui.available_width() - 58.0).max(110.0);
        let resp = ui.add(
            egui::TextEdit::singleline(&mut panel.query)
                .hint_text("Search Unsplash…")
                .desired_width(width),
        );
        let submit = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let clicked = ui.button("Search").clicked();
        do_search = (submit || clicked) && !panel.query.trim().is_empty();
    });
    // Cambiar un filtro relanza la búsqueda (si ya hay una consulta).
    if filters_ui(panel, ui) && !panel.query.trim().is_empty() {
        do_search = true;
    }
    if do_search {
        start_search(panel, tx, ui.ctx());
    }

    // Solo la PRIMERA búsqueda (sin resultados aún) muestra el spinner a
    // pantalla completa; «Load more» mantiene la lista visible y avisa en la
    // parte baja — nada de flashes al cargar la página siguiente.
    if panel.searching && panel.photos.is_empty() {
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.spinner();
            ui.weak("Searching…");
        });
        return;
    }
    if panel.photos.is_empty() {
        if let Some(err) = &panel.error {
            ui.add_space(8.0);
            ui.colored_label(ui.visuals().error_fg_color, err);
        } else {
            ui.add_space(8.0);
            ui.weak("Search for photos and click one\nto add it to the canvas.");
        }
        return;
    }

    // Lista vertical: una tarjeta por foto, imagen grande y centrada, un
    // poco más estrecha que el panel para que no ocupe todo el ancho
    // (margen lateral de `CARD_INSET` a cada lado).
    let row_w = (ui.available_width() - CARD_INSET * 2.0).max(120.0);
    let img_h = (row_w * 0.66).clamp(150.0, 320.0);
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.vertical_centered(|ui| {
                let inserting = &mut panel.inserting;
                for item in panel.photos.iter_mut() {
                    photo_card_ui(item, inserting, row_w, img_h, ui, tx);
                    ui.add_space(12.0);
                }
                ui.add_space(4.0);
                // Pie de la lista: mientras «Load more» está en vuelo solo
                // se muestra la animación centrada bajo la última tarjeta
                // (el botón desaparece); luego aviso de fin de resultados,
                // error con reintento, o el botón de más resultados.
                if panel.searching {
                    ui.add(egui::Spinner::new().size(26.0));
                } else if let Some(err) = &panel.error {
                    ui.colored_label(ui.visuals().error_fg_color, err);
                    if ui.button("Try again").clicked() {
                        load_more(panel, tx, ui.ctx());
                    }
                } else if panel.reached_end {
                    ui.weak("No more results for this search.");
                } else if load_more_button_ui(ui, row_w).clicked() {
                    load_more(panel, tx, ui.ctx());
                }
                // Aire bajo el pie de la lista: el botón/spinner/mensaje no
                // queda pegado al borde inferior del panel al hacer scroll.
                ui.add_space(12.0);
            });
        });
    ui.add_space(4.0);
    ui.weak("Photos from Unsplash — unsplash.com/license");
}

/// Una tarjeta de la lista: la foto cubre TODA la tarjeta de borde a borde
/// (recorte «cover», sin cajas interiores ni franjas) y el nombre del
/// fotógrafo va superpuesto abajo con una barra semitransparente. Clic para
/// insertar la foto centrada en el lienzo, o ARRASTRARLA hasta el lienzo
/// para soltarla en una posición concreta.
///
/// El clic es «suave»: una pulsación simple NO coge la tarjeta (ver
/// `card_drag_source`) — el fantasma y el payload de arrastre solo entran
/// en juego cuando el ratón se mueve de verdad (más allá del umbral de
/// clic), así que un clic nunca se convierte en un arrastre accidental ni
/// simula un arrastre.
fn photo_card_ui(
    item: &mut PhotoItem,
    inserting: &mut Option<String>,
    w: f32,
    h: f32,
    ui: &mut egui::Ui,
    tx: &Sender<loader::AppMsg>,
) {
    let visuals = ui.visuals().clone();
    let photo = item.photo.clone();

    // 1) Origen de arrastre sobre TODA la tarjeta. El closure pinta la
    //    tarjeta (el mismo pintado sirve de fantasma mientras se arrastra).
    let resp = card_drag_source(
        ui,
        egui::Id::new(("unsplash_card", photo.id.as_str())),
        DragUnsplash {
            id: photo.id.clone(),
            label: format!("Unsplash · {}", photo.user.name),
            url: photo.urls.regular.clone(),
        },
        |ui| {
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::click());

            // Fondo de la tarjeta (visible solo mientras la foto no ha llegado).
            ui.painter()
                .rect_filled(rect, 4.0, visuals.extreme_bg_color);

            if let Some(tex) = &item.thumb {
                let img = tex.size_vec2();
                if img.x > 0.0 && img.y > 0.0 {
                    // «Cover»: escala para llenar la tarjeta entera,
                    // recortando el sobrante — nunca hay huecos.
                    let scale = (rect.width() / img.x).max(rect.height() / img.y);
                    let size = img * scale;
                    let pos = rect.center() - size * 0.5;
                    ui.painter().with_clip_rect(rect).image(
                        tex.id(),
                        egui::Rect::from_min_size(pos, size),
                        egui::Rect::from_min_max(
                            egui::pos2(0.0, 0.0),
                            egui::pos2(1.0, 1.0),
                        ),
                        egui::Color32::WHITE,
                    );
                }
            } else {
                let msg = if item.thumb_failed { "no preview" } else { "…" };
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    msg,
                    egui::FontId::proportional(12.0),
                    visuals.weak_text_color(),
                );
            }

            // Barra inferior semitransparente con la atribución
            // (obligatoria) y la pista de clic, sobre la propia foto.
            let bar_h = 26.0;
            let bar = egui::Rect::from_min_max(
                egui::pos2(rect.left(), rect.bottom() - bar_h),
                rect.right_bottom(),
            );
            ui.painter()
                .rect_filled(bar, 0.0, egui::Color32::from_black_alpha(120));
            ui.painter().text(
                egui::pos2(bar.left() + 10.0, bar.center().y),
                egui::Align2::LEFT_CENTER,
                &photo.user.name,
                egui::FontId::proportional(11.0),
                egui::Color32::WHITE,
            );
            if inserting.as_deref() != Some(photo.id.as_str()) {
                ui.painter().text(
                    egui::pos2(bar.right() - 10.0, bar.center().y),
                    egui::Align2::RIGHT_CENTER,
                    "Click to add",
                    egui::FontId::proportional(10.0),
                    egui::Color32::from_white_alpha(210),
                );
            }

            if inserting.as_deref() == Some(photo.id.as_str()) {
                ui.painter()
                    .rect_filled(rect, 4.0, visuals.panel_fill.gamma_multiply(0.6));
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "Downloading…",
                    egui::FontId::proportional(12.0),
                    visuals.strong_text_color(),
                );
            }

            if resp.hovered() {
                ui.painter().rect_stroke(
                    rect,
                    4.0,
                    egui::Stroke::new(1.5, visuals.strong_text_color()),
                    egui::StrokeKind::Inside,
                );
            }
            resp
        },
    )
    .response;

    // 2) Clic: interact registrado DESPUÉS del drag source (queda ENCIMA).
    //    Si un widget de drag tapa a uno de clic, egui descarta el clic
    //    (`hits.click` se queda a `None`); el clic tiene que ser el widget
    //    superior. El arrastre no se ve afectado: `hits.drag` se calcula
    //    aparte en el hit-testing.
    let click = ui.interact(
        resp.rect,
        egui::Id::new(("unsplash_card_click", photo.id.as_str())),
        egui::Sense::click(),
    );
    if click.clicked() && inserting.is_none() {
        *inserting = Some(photo.id.clone());
        loader::spawn_unsplash_image(
            photo.id,
            format!("Unsplash · {}", photo.user.name),
            photo.urls.regular,
            tx.clone(),
            ui.ctx().clone(),
        );
    }
    let _ = resp.on_hover_text("Click to insert · drag to the canvas to place it");
}

/// Origen de arrastre de una tarjeta de foto, igual que
/// `Ui::dnd_drag_source` pero con una diferencia clave: el payload y el
/// fantasma solo entran en juego cuando el arrastre es REAL
/// (`pointer.is_decidedly_dragging`, movimiento más allá del umbral de
/// clic). Aunque egui marque el widget de drag como arrastrado en cuanto se
/// pulsa, una pulsación simple mantiene la tarjeta quieta y deja que el
/// clic haga su trabajo: nada de «coger» la tarjeta ni simular un arrastre
/// en un clic normal.
fn card_drag_source<Payload, R>(
    ui: &mut egui::Ui,
    id: egui::Id,
    payload: Payload,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R>
where
    Payload: std::any::Any + Send + Sync,
{
    let dragging = ui.ctx().is_being_dragged(id)
        && ui.ctx().input(|i| i.pointer.is_decidedly_dragging());
    if dragging {
        // Arrastre real en curso: refresca el payload y pinta el cuerpo en
        // una capa propia que sigue al cursor (el centro de la tarjeta queda
        // bajo el cursor).
        egui::DragAndDrop::set_payload(ui.ctx(), payload);
        let layer_id = egui::LayerId::new(egui::Order::Tooltip, id);
        let egui::InnerResponse { inner, response } =
            ui.scope_builder(egui::UiBuilder::new().layer_id(layer_id), add_contents);
        if let Some(pointer_pos) = ui.ctx().pointer_interact_pos() {
            let delta = pointer_pos - response.rect.center();
            ui.ctx().transform_layer_shapes(
                layer_id,
                egui::emath::TSTransform::from_translation(delta),
            );
        }
        egui::InnerResponse::new(inner, response)
    } else {
        let egui::InnerResponse { inner, response } = ui.scope(add_contents);
        let dnd_response = ui.interact(response.rect, id, egui::Sense::drag());
        egui::InnerResponse::new(inner, dnd_response | response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Un PNG 2x2 en memoria (rojo, verde, azul, blanco) para `decode`.
    fn tiny_png() -> Vec<u8> {
        let img = image::RgbaImage::from_raw(2, 2, vec![255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255])
            .unwrap();
        let mut out = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    #[test]
    fn decode_turns_png_bytes_into_loaded_image() {
        let img = decode(&tiny_png()).unwrap();
        assert_eq!((img.width, img.height), (2, 2));
        assert_eq!(img.rgba.len(), 2 * 2 * 4);
        assert_eq!(&img.rgba[..4], &[255, 0, 0, 255]);
    }

    #[test]
    fn decode_rejects_garbage() {
        assert!(decode(b"not an image").is_err());
    }

    #[test]
    fn search_without_key_is_an_error() {
        let had = std::env::var(ACCESS_KEY_ENV).ok();
        std::env::remove_var(ACCESS_KEY_ENV);
        let err = search("mountain", 1, SearchFilters::default()).unwrap_err();
        assert!(err.contains(ACCESS_KEY_ENV), "{err}");
        match had {
            Some(key) => std::env::set_var(ACCESS_KEY_ENV, key),
            None => std::env::remove_var(ACCESS_KEY_ENV),
        }
    }

    #[test]
    fn panel_defaults_to_empty() {
        let p = Panel::default();
        assert!(p.query.is_empty());
        assert!(p.photos.is_empty());
        assert!(p.error.is_none());
        assert!(!p.searching);
        assert!(!p.reached_end);
        assert_eq!(p.filters, SearchFilters::default());
        assert_eq!(p.search_seq, 0);
    }

    #[test]
    fn orientation_maps_to_api_values() {
        assert_eq!(Orientation::Any.as_str(), None);
        assert_eq!(Orientation::Landscape.as_str(), Some("landscape"));
        assert_eq!(Orientation::Portrait.as_str(), Some("portrait"));
        assert_eq!(Orientation::Squarish.as_str(), Some("squarish"));
    }

    #[test]
    fn color_maps_to_api_values() {
        assert_eq!(ColorFilter::Any.as_str(), None);
        assert_eq!(ColorFilter::BlackAndWhite.as_str(), Some("black_and_white"));
        assert_eq!(ColorFilter::Red.as_str(), Some("red"));
        assert_eq!(ColorFilter::Teal.as_str(), Some("teal"));
        // Todos los colores tienen etiqueta y punto de UI (excepto «Any»).
        for c in ColorFilter::ALL {
            assert!(!c.label().is_empty());
            if c == ColorFilter::Any {
                assert!(c.swatch().is_none());
            } else {
                assert!(c.swatch().is_some(), "{} sin swatch", c.label());
            }
        }
    }

    #[test]
    fn order_by_maps_to_api_values() {
        assert_eq!(OrderBy::Relevant.as_str(), "relevant");
        assert_eq!(OrderBy::Latest.as_str(), "latest");
        assert_eq!(OrderBy::default(), OrderBy::Relevant);
    }

    #[test]
    fn filters_default_to_no_restrictions() {
        let f = SearchFilters::default();
        assert_eq!(f.orientation, Orientation::Any);
        assert_eq!(f.color, ColorFilter::Any);
        assert_eq!(f.order_by, OrderBy::Relevant);
    }

    #[test]
    fn reached_end_is_true_when_total_pages_says_so() {
        assert!(reached_end(Some(1), 1, PER_PAGE as usize));
        assert!(reached_end(Some(3), 3, PER_PAGE as usize));
        assert!(!reached_end(Some(3), 2, PER_PAGE as usize));
    }

    #[test]
    fn reached_end_falls_back_to_short_page() {
        // Sin `total_pages` en la respuesta, una página incompleta es el fin.
        assert!(reached_end(None, 1, 7));
        // Una página completa sin `total_pages` no es necesariamente el fin.
        assert!(!reached_end(None, 1, PER_PAGE as usize));
    }

    #[test]
    fn search_response_parses_total_pages() {
        let json = r#"{"total_pages": 4, "results": []}"#;
        let parsed: SearchResponse = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.total_pages, Some(4));
        // El campo puede faltar (respuestas antiguas): se ignora.
        let parsed: SearchResponse = serde_json::from_str(r#"{"results": []}"#).unwrap();
        assert_eq!(parsed.total_pages, None);
    }
}
