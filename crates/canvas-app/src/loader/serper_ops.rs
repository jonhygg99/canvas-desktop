//! Búsqueda web (Serper) en hilos aparte: la llamada a la API, la descarga
//! de miniaturas, la de la imagen completa y la creación masiva (cada
//! elegida, su propio lienzo PNG + sidecar). Nada de red ni de decodificado
//! toca la UI; los resultados viajan por el canal del workspace (`AppMsg`),
//! igual que el resto del loader. Cada `spawn_serper_search` es UNA llamada
//! (1 token, 1 página); el panel decide cuántas gasta por keyword (máx 2,
//! solo con 3 tokens).

use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

use eframe::egui;

use super::AppMsg;

/// Una imagen elegida en la ventana masiva para crear su lienzo. Las dims
/// de la API sirven para dimensionar la página común sin descargar nada;
/// `post_url` es la página del post enlazado (rescate `og:image`) y
/// `thumb_url`, la miniatura ya mostrada (último nivel del rescate, A08).
#[derive(Debug, Clone)]
pub struct BulkItem {
    pub url: String,
    pub label: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub post_url: String,
    pub thumb_url: Option<String>,
}

/// Lanza la búsqueda de `request` (página 1-based). `seq` identifica la
/// llamada para descartar respuestas caducas. Antes de tocar la red mira
/// la caché de disco (si hay hit fresco, se sirve sin gastar); tras éxito
/// guarda su copia — salvo en modo social, cuyas URLs caducan y no deben
/// re-servirse mañana. La respuesta trae las fotos SIN miniaturas: cada
/// tarjeta pide la suya al pintarse (`spawn_serper_thumb`).
pub fn spawn_serper_search(
    request: crate::serper::SearchRequest,
    tx: Sender<AppMsg>,
    ctx: egui::Context,
) {
    std::thread::spawn(move || {
        let crate::serper::SearchRequest {
            keyword,
            page,
            num,
            blocked,
            mode,
            seq,
        } = request;
        let cache_key = crate::serper::cache::cache_key(&keyword, &blocked, page, num, mode);
        // En modo social no se toca el disco en ningún sentido: las
        // imageUrl de Meta caducan y re-servirlas sería 403 seguro. Solo
        // vale la caché de memoria (la mira el panel antes de llamar).
        if mode == crate::serper::SearchMode::Web {
            if let Some(cached) = crate::serper::cache::load_disk(&cache_key) {
                let _ = tx.send(AppMsg::SerperSearch {
                    seq,
                    page,
                    cache_key,
                    result: Ok(cached.into_page()),
                });
                ctx.request_repaint();
                return;
            }
        }
        let result = crate::serper::search(&keyword, page, num, &blocked, mode);
        if mode == crate::serper::SearchMode::Web {
            if let Ok(page_result) = &result {
                crate::serper::cache::save_disk(&cache_key, page_result);
            }
        }
        let _ = tx.send(AppMsg::SerperSearch {
            seq,
            page,
            cache_key,
            result,
        });
        ctx.request_repaint();
    });
}

/// Lado mayor máximo de una miniatura del panel/bulk (A11): las tarjetas se
/// pintan a ~300 px; subir la foto completa (a veces varios MP) como textura
/// por cada resultado multiplica la RAM de GPU sin aportar nada visible.
/// El aspecto se conserva, así que el masonry y el filtro de banners no
/// cambian.
pub const THUMB_MAX_LONG: u32 = 512;

/// Dimensiones de una miniatura encajada en `THUMB_MAX_LONG` conservando el
/// aspecto. Pura y testeable (sin materializar nada).
pub(crate) fn fit_thumb_dims(width: u32, height: u32) -> (u32, u32) {
    let long = width.max(height);
    if long <= THUMB_MAX_LONG || long == 0 {
        return (width.max(1), height.max(1));
    }
    let scale = f64::from(THUMB_MAX_LONG) / f64::from(long);
    (
        (f64::from(width) * scale).round().max(1.0) as u32,
        (f64::from(height) * scale).round().max(1.0) as u32,
    )
}

/// Descarga y decodifica la miniatura de un resultado para mostrarla en la
/// tarjeta del panel. `post_url` rescata vía `og:image` si la URL es una
/// página embed social. Segundo filtro de proporción con las dimensiones
/// REALES: si la API no traía dims y resulta ser un banner, se informa
/// como `FilteredBanner` y la tarjeta se retira en silencio.
pub fn spawn_serper_thumb(
    id: String,
    url: String,
    post_url: String,
    tx: Sender<AppMsg>,
    ctx: egui::Context,
) {
    std::thread::spawn(move || {
        let result = crate::serper::fetch_image(&url, Some(&post_url), None)
            .map(|fetched| downscale_thumb(fetched.image))
            .and_then(|img| {
                if crate::serper::filter::ratio_blocked(img.width, img.height) {
                    Err(crate::serper::SerperError::FilteredBanner)
                } else {
                    Ok(img)
                }
            });
        let _ = tx.send(AppMsg::SerperThumb { id, result });
        ctx.request_repaint();
    });
}

/// Reduce una imagen descargada a tamaño de miniatura (`fit_thumb_dims`)
/// antes de subirla a GPU como textura (A11). Si los píxeles no cuadran con
/// sus dimensiones (imposible tras `decode`), se devuelve tal cual en vez
/// de fallar.
fn downscale_thumb(img: canvas_io::LoadedImage) -> canvas_io::LoadedImage {
    let (nw, nh) = fit_thumb_dims(img.width, img.height);
    if (nw, nh) == (img.width, img.height) {
        return img;
    }
    let Some(rgba) = image::RgbaImage::from_raw(img.width, img.height, img.rgba) else {
        // Imposible tras `decode` (los píxeles cuadran por construcción):
        // píxel negro 1×1 antes que una textura vacía.
        return canvas_io::LoadedImage {
            rgba: vec![0, 0, 0, 255],
            width: 1,
            height: 1,
        };
    };
    let small = image::imageops::resize(&rgba, nw, nh, image::imageops::FilterType::Triangle);
    canvas_io::LoadedImage {
        width: small.width(),
        height: small.height(),
        rgba: small.into_raw(),
    }
}

/// Descarga y decodifica la imagen completa de un resultado para insertarla
/// como capa nueva del documento abierto. `post_url` rescata vía `og:image`
/// si la URL directa es una página embed social. `target` identifica el
/// destino que la pidió y viaja con la respuesta para validarla (A07).
/// `thumb_url` alimenta el último nivel del rescate (la miniatura ya
/// mostrada); la inserción avisa si los píxeles vinieron de otra URL (A08).
pub struct SerperImageRequest {
    pub id: String,
    pub label: String,
    pub url: String,
    pub post_url: String,
    pub thumb_url: Option<String>,
    pub target: super::ImageInsertTarget,
}

/// Descarga y decodifica la imagen completa pedida en `req`.
pub fn spawn_serper_image(req: SerperImageRequest, tx: Sender<AppMsg>, ctx: egui::Context) {
    std::thread::spawn(move || {
        let result =
            crate::serper::fetch_image(&req.url, Some(&req.post_url), req.thumb_url.as_deref());
        let _ = tx.send(AppMsg::SerperImageReady {
            id: req.id,
            label: req.label,
            result,
            target: req.target,
        });
        ctx.request_repaint();
    });
}

/// Descarga cada elegida y la guarda como su propio lienzo (`stem-NN.png` +
/// sidecar editable) en `folder`, de forma SECUENCIAL en un solo hilo (RAM
/// acotada: una imagen cada vez). Las descargas directas no gastan créditos
/// Serper. Avisa con progreso por archivo y un mensaje final con lo creado
/// y los fallos.
pub fn spawn_serper_bulk_files(
    items: Vec<BulkItem>,
    folder: PathBuf,
    keyword: String,
    page: (f64, f64),
    tx: Sender<AppMsg>,
    ctx: egui::Context,
) {
    std::thread::spawn(move || {
        let total = items.len();
        let stem = slugify(&keyword);
        let mut created = Vec::new();
        let mut errors = Vec::new();
        // Lienzos cuyos píxeles vinieron de un rescate (`og:image`/thumb)
        // en vez de la URL directa (A08): se informa en el mensaje final,
        // nunca se aceptan en silencio.
        let mut substituted = 0usize;
        if let Err(e) = std::fs::create_dir_all(&folder) {
            let msg = format!("cannot create folder {}: {e}", folder.display());
            for item in &items {
                errors.push(format!("{}: {msg}", item.label));
            }
            let _ = tx.send(AppMsg::SerperBulkProgress { done: total, total });
            ctx.request_repaint();
            let _ = tx.send(AppMsg::SerperBulkDone {
                folder,
                created,
                errors,
                substituted,
            });
            ctx.request_repaint();
            return;
        }
        for (i, item) in items.into_iter().enumerate() {
            // Un pánico en una imagen (píxeles patológicos, OOM contenida)
            // no puede matar la tanda sin Done: el panel se quedaría en
            // "loading" para siempre. Se aísla por item.
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                save_bulk_one(&folder, &stem, i, &item, page)
            }));
            match outcome {
                Ok(Ok((path, was_substituted))) => {
                    substituted += usize::from(was_substituted);
                    created.push(path);
                }
                Ok(Err(e)) => errors.push(format!("{}: {e}", item.label)),
                Err(_) => errors.push(format!("{}: internal worker error", item.label)),
            }
            let _ = tx.send(AppMsg::SerperBulkProgress { done: i + 1, total });
            ctx.request_repaint();
        }
        let _ = tx.send(AppMsg::SerperBulkDone {
            folder,
            created,
            errors,
            substituted,
        });
        ctx.request_repaint();
    });
}

/// Marca el panel como ocupado y lanza la creación masiva en `folder` con
/// la página `page` (una por tanda, decidida en el pie del bulk).
/// Lo llama el pie del bulk (la carpeta ya viene resuelta o es la de por
/// defecto: nunca hay diálogo que bloquee).
pub fn begin_bulk_files(
    panel: &mut crate::serper::Panel,
    items: Vec<BulkItem>,
    folder: PathBuf,
    keyword: String,
    page: (f64, f64),
    tx: Sender<AppMsg>,
    ctx: egui::Context,
) {
    panel.bulk_busy = true;
    panel.bulk_progress = Some((0, items.len()));
    panel.bulk_progress_at = Some(std::time::Instant::now());
    panel.bulk_done_msg = None;
    panel.bulk_errors.clear();
    spawn_serper_bulk_files(items, folder, keyword, page, tx, ctx);
}

/// Carpeta de respaldo cuando no hay baraja, archivo ni última usada:
/// `Imágenes/Canvas Web Imports/<slug>/`, creada sola. Sin diálogos: el
/// Add funciona hasta en un `New design` sin guardar. `SERPER_BULK_DIR` la
/// redirige (solo la usan los tests).
pub fn default_bulk_dir(keyword: &str) -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("SERPER_BULK_DIR") {
        if !dir.trim().is_empty() {
            return Some(PathBuf::from(dir));
        }
    }
    let pictures = directories::UserDirs::new()?.picture_dir()?.to_owned();
    Some(pictures.join("Canvas Web Imports").join(slugify(keyword)))
}

/// Tamaño de página de la tanda según el ajuste: fijo, o el máximo de la
/// tanda cuando el ajuste es `BatchMax`. Pura y testeable. El resultado pasa
/// por `clamp_bulk_page` (A11): sin techo, unas dims mentirosas de la API
/// (p. ej. 20000×20000) asignarían ~1,5 GiB de RGBA en el horneado.
pub fn resolve_bulk_page(size: crate::settings::BulkCanvasSize, items: &[BulkItem]) -> (f64, f64) {
    let (w, h) = size.dims().unwrap_or_else(|| common_page_size(items));
    let (w, h) = clamp_bulk_page(w.round().max(1.0) as u32, h.round().max(1.0) as u32);
    (f64::from(w), f64::from(h))
}

/// Lado mayor máximo y techo de píxeles de un lienzo bulk (A11).
pub const MAX_BULK_PAGE_LONG: u32 = 4096;
pub const MAX_BULK_PAGE_PIXELS: u64 = 16_777_216; // 4096²

/// Recorta unas dimensiones al presupuesto del bulk conservando el aspecto
/// (A11). Aritmética comprobada en `u64` (un `u32 × u32` siempre cabe) y sin
/// materializar nada: rechazar 20000×20000 no asigna sus 1,5 GiB.
pub(crate) fn clamp_bulk_page(width: u32, height: u32) -> (u32, u32) {
    let (w, h) = (width.max(1), height.max(1));
    let pixels = u64::from(w) * u64::from(h);
    let long = w.max(h);
    if long <= MAX_BULK_PAGE_LONG && pixels <= MAX_BULK_PAGE_PIXELS {
        return (w, h);
    }
    let by_long = f64::from(MAX_BULK_PAGE_LONG) / f64::from(long);
    let by_pixels = (MAX_BULK_PAGE_PIXELS as f64 / pixels as f64).sqrt();
    let scale = by_long.min(by_pixels);
    (
        (f64::from(w) * scale).floor().max(1.0) as u32,
        (f64::from(h) * scale).floor().max(1.0) as u32,
    )
}

/// Tamaño de página COMÚN de la tanda: máx. ancho × máx. alto conocidos por
/// la API (sin descargar nada). Así todos los lienzos miden lo mismo y
/// ninguna foto se amplía ni se recorta; sin dims, 1920×1080.
fn common_page_size(items: &[BulkItem]) -> (f64, f64) {
    let mut mw = 0u32;
    let mut mh = 0u32;
    for item in items {
        if let (Some(w), Some(h)) = (item.width, item.height) {
            if w > 0 && h > 0 {
                mw = mw.max(w);
                mh = mh.max(h);
            }
        }
    }
    if mw == 0 || mh == 0 {
        (1920.0, 1080.0)
    } else {
        (f64::from(mw), f64::from(mh))
    }
}

/// Descarga una imagen y la guarda como `stem-NN.png` + sidecar editable en
/// la página común `page` (misma para toda la tanda). Devuelve la ruta y si
/// los píxeles vinieron de una URL de rescate en vez de la directa (A08).
fn save_bulk_one(
    folder: &Path,
    stem: &str,
    index: usize,
    item: &BulkItem,
    page: (f64, f64),
) -> Result<(PathBuf, bool), String> {
    let fetched =
        crate::serper::fetch_image(&item.url, Some(&item.post_url), item.thumb_url.as_deref())
            .map_err(|e| e.to_string())?;
    let substituted = fetched.substituted(&item.url);
    let img = fetched.image;
    // A06: el nombre se RESERVA atómicamente (`create_new`) antes de
    // escribir: dos ventanas importando la misma keyword a la misma carpeta
    // nunca obtienen el mismo nombre. La reserva vive hasta que la imagen
    // queda escrita; si algo falla antes, se retira el hueco vacío propio
    // (nunca un archivo ajeno: la reserva la creó este worker).
    let path =
        canvas_io::reserve_bulk_path(folder, stem, "png", index + 1).map_err(|e| e.to_string())?;
    let photo = image::RgbaImage::from_raw(img.width, img.height, img.rgba.clone())
        .ok_or_else(|| "decoded image has no pixels".to_owned())?;
    let (pw, ph) = (page.0.round().max(1.0), page.1.round().max(1.0));
    // Receta exacta de pegar: lienzo vacío + add_image_layer (contain +
    // fondo desenfocado si no cubre).
    let mut state = crate::editor::EditorState::new_blank_image(pw, ph);
    state.add_image_layer(item.label.clone(), Some(path.clone()), img);
    state.doc.source_path = Some(path.clone());
    let mut payload = state.sidecar_payload();
    // Todo lo que falla a partir de aquí deja la reserva a medio escribir:
    // si la imagen aún no quedó en disco, se retira el hueco propio; si la
    // imagen SÍ quedó pero falló el sidecar, se conserva (es un producto
    // visible y la tanda lo informa como error del item).
    let baked = payload_bake(&state, &photo).inspect_err(|_| {
        let _ = std::fs::remove_file(&path);
    })?;
    let (pw_px, ph_px) = (pw as u32, ph as u32);
    let written =
        canvas_io::save_rgba(&path, baked.clone(), pw_px, ph_px, 92, None).map_err(|e| {
            let _ = std::fs::remove_file(&path);
            e.to_string()
        })?;
    payload.preview = canvas_io::make_preview(&baked, pw_px, ph_px);
    canvas_io::write_sidecar(&path, &written, &payload).map_err(|e| e.to_string())?;
    Ok((path, substituted))
}

/// Hornea el PNG con la geometría ya calculada por `add_image_layer` (foto
/// en *contain* + fondo desenfocado si no cubre), para que el plano
/// coincida con las capas del sidecar.
fn payload_bake(
    state: &crate::editor::EditorState,
    photo: &image::RgbaImage,
) -> Result<Vec<u8>, String> {
    use crate::serper::bake::{bake_contain_blur, BakeGeom};
    let page = state.doc.page().map_err(|e| e.to_string())?;
    let (pw, ph) = (
        page.width.round().max(1.0) as u32,
        page.height.round().max(1.0) as u32,
    );
    let photo_layer = page.layers.last().ok_or_else(|| "empty page".to_owned())?;
    let has_bg = page
        .layers
        .first()
        .is_some_and(|l| l.name == "Blurred background");
    let t = &photo_layer.transform;
    Ok(bake_contain_blur(
        photo,
        &BakeGeom {
            pw,
            ph,
            bg: page.background.unwrap_or([255, 255, 255, 255]),
            fx: t.x,
            fy: t.y,
            fw: t.width,
            fh: t.height,
            has_bg,
        },
    ))
}

/// Trocea la keyword a un tallo de archivo seguro (minúsculas, guiones).
fn slugify(keyword: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in keyword.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
        if out.len() >= 30 {
            break;
        }
    }
    let slug = out.trim_matches('-').to_owned();
    if slug.is_empty() {
        "web".to_owned()
    } else {
        slug
    }
}

#[cfg(test)]
#[path = "serper_ops_tests.rs"]
mod tests;
