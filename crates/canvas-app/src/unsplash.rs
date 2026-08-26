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

/// Envuelve el JSON de `/search/photos`: resultados o lista de errores.
#[derive(Debug, serde::Deserialize)]
struct SearchResponse {
    #[serde(default)]
    results: Vec<Photo>,
    #[serde(default)]
    errors: Vec<String>,
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

/// Busca fotos en Unsplash. `page` es 1-based; devuelve hasta `PER_PAGE`
/// resultados. Solo se llama desde hilos worker.
pub fn search(query: &str, page: u32) -> Result<Vec<Photo>, String> {
    let Some(key) = access_key() else {
        return Err(format!("{ACCESS_KEY_ENV} is not set"));
    };
    let query = query.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let resp = agent()
        .get(SEARCH_URL)
        .query("query", query)
        .query("page", &page.to_string())
        .query("per_page", &PER_PAGE.to_string())
        .query("client_id", &key)
        .call()
        .map_err(|e| format!("Unsplash search failed: {e}"))?;
    let body = resp
        .into_string()
        .map_err(|e| format!("Unsplash search failed: {e}"))?;
    let parsed: SearchResponse =
        serde_json::from_str(&body).map_err(|e| format!("Unsplash returned an invalid response: {e}"))?;
    if let Some(err) = parsed.errors.first() {
        return Err(err.clone());
    }
    Ok(parsed.results)
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

/// Estado del panel «Images» del sidebar del editor: consulta, resultados
/// (con sus miniaturas ya subidas a GPU) y errores. Vive en `EditorState`.
#[derive(Default)]
pub struct Panel {
    pub query: String,
    /// Última página cargada (1-based).
    pub page: u32,
    /// Hay una búsqueda o descarga de lote en vuelo (desactiva la UI).
    pub searching: bool,
    pub photos: Vec<PhotoItem>,
    pub error: Option<String>,
    /// Id de la foto cuya imagen completa se está descargando para insertar.
    pub inserting: Option<String>,
}

/// Un resultado con su miniatura (si ya llegó del worker).
pub struct PhotoItem {
    pub photo: Photo,
    pub thumb: Option<egui::TextureHandle>,
    pub thumb_failed: bool,
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
    ui.horizontal(|ui| {
        let width = (ui.available_width() - 58.0).max(110.0);
        let resp = ui.add(
            egui::TextEdit::singleline(&mut panel.query)
                .hint_text("Search Unsplash…")
                .desired_width(width),
        );
        let submit = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let clicked = ui.button("Search").clicked();
        if (submit || clicked) && !panel.searching && !panel.query.trim().is_empty() {
            panel.searching = true;
            panel.page = 1;
            panel.photos.clear();
            panel.error = None;
            loader::spawn_unsplash_search(
                panel.query.trim().to_owned(),
                1,
                tx.clone(),
                ui.ctx().clone(),
            );
        }
    });

    if panel.searching {
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.spinner();
            ui.weak("Searching…");
        });
        return;
    }
    if let Some(err) = &panel.error {
        ui.add_space(8.0);
        ui.colored_label(ui.visuals().error_fg_color, err);
        return;
    }
    if panel.photos.is_empty() {
        ui.add_space(8.0);
        ui.weak("Search for photos and click one\nto add it to the canvas.");
        return;
    }

    // Lista vertical: una tarjeta por foto, imagen grande a todo lo ancho.
    let row_w = ui.available_width();
    let img_h = (row_w * 0.66).clamp(150.0, 320.0);
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let inserting = &mut panel.inserting;
            for item in panel.photos.iter_mut() {
                photo_card_ui(item, inserting, row_w, img_h, ui, tx);
                ui.add_space(12.0);
            }
            if !panel.photos.is_empty() {
                ui.add_space(4.0);
                ui.vertical_centered(|ui| {
                    if ui.button("Load more").clicked() {
                        panel.searching = true;
                        panel.page += 1;
                        loader::spawn_unsplash_search(
                            panel.query.trim().to_owned(),
                            panel.page,
                            tx.clone(),
                            ui.ctx().clone(),
                        );
                    }
                });
            }
        });
    ui.add_space(4.0);
    ui.weak("Photos from Unsplash — unsplash.com/license");
}

/// Una tarjeta de la lista: la foto cubre TODA la tarjeta de borde a borde
/// (recorte «cover», sin cajas interiores ni franjas) y el nombre del
/// fotógrafo va superpuesto abajo con una barra semitransparente. Clic para
/// insertar la foto como capa nueva.
fn photo_card_ui(
    item: &mut PhotoItem,
    inserting: &mut Option<String>,
    w: f32,
    h: f32,
    ui: &mut egui::Ui,
    tx: &Sender<loader::AppMsg>,
) {
    let visuals = ui.visuals().clone();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::click());

    // Fondo de la tarjeta (visible solo mientras la foto no ha llegado).
    ui.painter()
        .rect_filled(rect, 4.0, visuals.extreme_bg_color);

    if let Some(tex) = &item.thumb {
        let img = tex.size_vec2();
        if img.x > 0.0 && img.y > 0.0 {
            // «Cover»: escala para llenar la tarjeta entera, recortando el
            // sobrante en horizontal o vertical — nunca hay huecos.
            let scale = (rect.width() / img.x).max(rect.height() / img.y);
            let size = img * scale;
            let pos = rect.center() - size * 0.5;
            ui.painter().with_clip_rect(rect).image(
                tex.id(),
                egui::Rect::from_min_size(pos, size),
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
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

    // Barra inferior semitransparente con la atribución (obligatoria) y la
    // pista de clic, superpuesta sobre la propia foto.
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
        &item.photo.user.name,
        egui::FontId::proportional(11.0),
        egui::Color32::WHITE,
    );
    if inserting.as_deref() != Some(item.photo.id.as_str()) {
        ui.painter().text(
            egui::pos2(bar.right() - 10.0, bar.center().y),
            egui::Align2::RIGHT_CENTER,
            "Click to add",
            egui::FontId::proportional(10.0),
            egui::Color32::from_white_alpha(210),
        );
    }

    if inserting.as_deref() == Some(item.photo.id.as_str()) {
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

    if resp.clicked() && inserting.is_none() {
        let photo = item.photo.clone();
        *inserting = Some(photo.id.clone());
        let label = format!("Unsplash · {}", photo.user.name);
        loader::spawn_unsplash_image(
            photo.id,
            label,
            photo.urls.regular,
            tx.clone(),
            ui.ctx().clone(),
        );
    }
    let _ = resp.on_hover_text("Click to insert this photo");
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
        let err = search("mountain", 1).unwrap_err();
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
    }
}
