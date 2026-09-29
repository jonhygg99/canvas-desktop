//! Ventana de selección masiva del panel Web: masonry tipo Pinterest con
//! las imágenes a su tamaño natural, todas marcadas por defecto, botones
//! de marcar/desmarcar e inserción masiva (cada elegida, una capa del
//! lienzo abierto).
//!
//! El masonry es manual (egui no trae uno): columnas responsive (2–4 según
//! el ancho) con cada foto asignada a la columna más baja acumulada, y
//! cada celda alta según su aspecto real (thumb GPU > dims API > 4:3).

use std::path::PathBuf;
use std::sync::mpsc::Sender;

use eframe::egui;

use crate::loader::{self, BulkItem};

use super::bulk_layout::{
    assign_columns, column_count, estimate_h, masonry_col_width, masonry_h, paint_masonry_image,
    MASONRY_GAP,
};
use super::state::Panel;

/// Vista masiva «Select web images»: ventana movible y redimensionable con
/// masonry y pie de creación (cada elegida, su propio lienzo). Se abre con
/// todo seleccionado (ver `Panel::open_bulk`). `dest` es la carpeta ya
/// resuelta (`None` = pedirla con diálogo al pulsar Add).
pub(super) fn bulk_window_ui(
    panel: &mut Panel,
    dest: Option<PathBuf>,
    settings: &crate::settings::AppSettings,
    ctx: &egui::Context,
    tx: &Sender<loader::AppMsg>,
) {
    if !panel.bulk_open {
        return;
    }
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) && !panel.bulk_busy {
        panel.bulk_open = false;
        return;
    }
    // Tamaño moderado por defecto (a pantalla casi completa sobra hueco con
    // pocas imágenes); el usuario la agranda a gusto: es movible y
    // redimensionable. El `Window` reparte el alto disponible de verdad
    // (el `Area` manual anterior colapsaba el scroll).
    egui::Window::new("Select web images")
        .collapsible(false)
        .resizable(true)
        .movable(true)
        .min_size([560.0, 420.0])
        .default_size([800.0, 640.0])
        .show(ctx, |ui| {
            bulk_header(panel, ui);
            // El scroll acotado deja sitio al pie: sin `max_height` se come
            // todo el alto y el botón Add queda fuera de la vista.
            let grid_h = (ui.available_height() - FOOTER_H).max(200.0);
            bulk_grid_at_height(panel, ui, tx, grid_h);
            ui.separator();
            bulk_footer(panel, &dest, settings, ui, tx, ctx);
        });
    // Al cerrar a mitad de inserción el hilo sigue (ya está lanzado); el
    // progreso y el mensaje final esperan en el panel a la próxima apertura.
}

/// Reserva vertical del pie compacto (botones + mensaje + errores
/// colapsables): el masonry deja este hueco para que el botón Add siempre
/// quede visible sin scroll.
const FOOTER_H: f32 = 88.0;

/// Cabecera: título + Select all / Deselect + contador + ✕.
fn bulk_header(panel: &mut Panel, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.heading("Select web images");
        ui.separator();
        ui.add_enabled_ui(!panel.bulk_busy, |ui| {
            if ui.button("Select all").clicked() {
                panel.select_all_bulk();
            }
            if ui.button("Deselect").clicked() {
                panel.deselect_all_bulk();
            }
        });
        ui.weak(format!("{} selected", panel.bulk_selected.len()));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_enabled_ui(!panel.bulk_busy, |ui| {
                if ui.button("✕").on_hover_text("Close").clicked() {
                    panel.bulk_open = false;
                }
            });
        });
    });
    ui.separator();
}

/// Masonry de resultados con `max_h` explícito: un `ScrollArea` sin tope
/// dentro de la ventana se come todo el alto y empuja el pie (con el botón
/// Add) fuera de la vista — por eso el llamador reserva `FOOTER_H`.
fn bulk_grid_at_height(
    panel: &mut Panel,
    ui: &mut egui::Ui,
    tx: &Sender<loader::AppMsg>,
    max_h: f32,
) {
    if panel.photos.is_empty() {
        ui.weak("No images to choose from yet — run a search first.");
        return;
    }
    let ctx = ui.ctx().clone();
    request_thumbs(panel, tx, &ctx);
    let cols = column_count(ui.available_width());
    let col_w = masonry_col_width(ui.available_width(), cols);
    let heights: Vec<f32> = panel
        .photos
        .iter()
        .map(|item| estimate_h(item, col_w))
        .collect();
    let columns = assign_columns(&heights, cols);
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .max_height(max_h)
        .show(ui, |ui| {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = MASONRY_GAP;
                for col in &columns {
                    ui.vertical(|ui| {
                        ui.set_min_width(col_w);
                        ui.set_max_width(col_w);
                        for &idx in col {
                            bulk_masonry_cell(panel, idx, col_w, heights[idx], ui, tx);
                            ui.add_space(MASONRY_GAP);
                        }
                    });
                }
            });
        });
}

/// Pide los thumbs que falten, con tope por frame para no lanzar cientos
/// de hilos al abrir (cada frame reevalúa: al hacer scroll se piden los
/// siguientes). Esta es la pieza que faltaba: antes solo el sidebar pedía
/// thumbs, y el bulk mostraba placeholders al bajar.
fn request_thumbs(panel: &mut Panel, tx: &Sender<loader::AppMsg>, ctx: &egui::Context) {
    for (id, url) in panel.claim_thumbs(12) {
        loader::spawn_serper_thumb(id, url, tx.clone(), ctx.clone());
    }
}

/// Nº de columnas, reparto, aspecto y pintado del masonry: ver
/// `bulk_layout` (puros y testeables sin ventana).
/// Una celda del masonry: checkbox + imagen a su aspecto natural + dominio.
/// El CLIC en la imagen alterna la selección (no solo el checkbox); la
/// seleccionada lleva borde azul y la no seleccionada va atenuada. Sin
/// preview, la celda muestra el motivo y un Retry en vez de alternar.
fn bulk_masonry_cell(
    panel: &mut Panel,
    index: usize,
    col_w: f32,
    est_h: f32,
    ui: &mut egui::Ui,
    tx: &Sender<loader::AppMsg>,
) {
    let Some(item) = panel.photos.get(index) else {
        return;
    };
    // Datos propios antes de prestar `panel` en mutable (igual que antes).
    let id = item.photo.id.clone();
    let host = item.photo.source_host();
    let tex = item.thumb.clone();
    let error = item.thumb_error.clone();
    let h = masonry_h(&tex, est_h, col_w);
    let busy = panel.bulk_busy;
    let mut selected = panel.bulk_selected.contains(&id);
    ui.horizontal(|ui| {
        ui.add_enabled_ui(!busy, |ui| {
            if ui.checkbox(&mut selected, "").changed() {
                toggle_bulk(panel, &id, selected);
            }
        });
        ui.weak(host);
    });
    if let Some(tex) = tex {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(col_w, h), egui::Sense::click());
        paint_masonry_image(ui, &tex, rect, panel.bulk_selected.contains(&id));
        let click = ui.interact(
            rect,
            egui::Id::new(("bulk_img", id.as_str())),
            egui::Sense::click(),
        );
        if click.clicked() && !busy {
            let cur = panel.bulk_selected.contains(&id);
            toggle_bulk(panel, &id, !cur);
        }
        let _ = click.on_hover_text("Click to select / deselect");
    } else if let Some(err) = error {
        retry_cell(
            panel,
            &RetryPaint {
                id: id.clone(),
                err,
                col_w,
                est_h,
            },
            ui,
            tx,
        );
    } else {
        placeholder_cell(panel, &id, col_w, est_h, busy, ui);
    }
}

/// Celda aún sin thumb (descargando): placeholder con el dominio; el clic
/// alterna la selección igual que con imagen.
fn placeholder_cell(
    panel: &mut Panel,
    id: &str,
    col_w: f32,
    est_h: f32,
    busy: bool,
    ui: &mut egui::Ui,
) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(col_w, est_h), egui::Sense::click());
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "…",
        egui::FontId::proportional(12.0),
        ui.visuals().weak_text_color(),
    );
    let click = ui.interact(rect, egui::Id::new(("bulk_img", id)), egui::Sense::click());
    if click.clicked() && !busy {
        let cur = panel.bulk_selected.contains(id);
        toggle_bulk(panel, id, !cur);
    }
}

/// Parámetros de una celda fallida (convención del repo: struct en vez de
/// `#[allow(too_many_arguments)]`).
struct RetryPaint {
    id: String,
    err: String,
    col_w: f32,
    est_h: f32,
}

/// Celda con preview fallido: motivo corto + botón Retry (no alterna la
/// selección al pulsar el botón, solo al marcar el checkbox de arriba).
fn retry_cell(
    panel: &mut Panel,
    paint: &RetryPaint,
    ui: &mut egui::Ui,
    tx: &Sender<loader::AppMsg>,
) {
    let busy = panel.bulk_busy;
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(paint.col_w, paint.est_h), egui::Sense::click());
    ui.painter().text(
        rect.center() - egui::vec2(0.0, 10.0),
        egui::Align2::CENTER_CENTER,
        format!("no preview ({})", super::api::short_reason(&paint.err)),
        egui::FontId::proportional(12.0),
        ui.visuals().weak_text_color(),
    );
    let id_owned = paint.id.clone();
    let btn = egui::Rect::from_center_size(
        rect.center() + egui::vec2(0.0, 14.0),
        egui::vec2(70.0, 22.0),
    );
    ui.add_enabled_ui(!busy, |ui| {
        if ui
            .put(btn, egui::Button::new("Retry"))
            .on_hover_text(&paint.err)
            .clicked()
        {
            let mut spawn = None;
            if let Some(item) = panel.photos.iter_mut().find(|p| p.photo.id == id_owned) {
                item.retry_thumb();
                if item.claim_thumb() {
                    spawn = Some((item.photo.id.clone(), item.photo.image_url.clone()));
                }
            }
            if let Some((id, url)) = spawn {
                loader::spawn_serper_thumb(id, url, tx.clone(), ui.ctx().clone());
            }
        }
    });
}

/// Aplica el cambio de un checkbox al conjunto de selección.
fn toggle_bulk(panel: &mut Panel, id: &str, selected: bool) {
    if selected {
        panel.bulk_selected.insert(id.to_owned());
    } else {
        panel.bulk_selected.remove(id);
    }
}

/// Carpeta destino del bulk sin molestar: baraja abierta → carpeta del
/// archivo abierto → galería de origen → última usada → `None` (diálogo).
/// Pura y testeable.
pub(crate) fn resolve_bulk_folder(
    deck_folder: Option<PathBuf>,
    source_path: Option<PathBuf>,
    from_gallery: Option<PathBuf>,
    last: Option<PathBuf>,
) -> Option<PathBuf> {
    deck_folder
        .or_else(|| source_path.and_then(|p| p.parent().map(PathBuf::from)))
        .or(from_gallery)
        .or(last)
}

/// Pie compacto en UNA fila de botones a la derecha (Add a la derecha del
/// todo, Cancel a su izquierda) + mensaje y errores colapsables debajo.
/// `dest` ya viene resuelta; sin nada se usa la carpeta de respaldo, así el
/// Add funciona hasta en un `New design` sin guardar. El tamaño de cada
/// lienzo sale del ajuste y se muestra para que no haya dudas.
fn bulk_footer(
    panel: &mut Panel,
    dest: &Option<PathBuf>,
    settings: &crate::settings::AppSettings,
    ui: &mut egui::Ui,
    tx: &Sender<loader::AppMsg>,
    ctx: &egui::Context,
) {
    if panel.bulk_busy {
        bulk_progress_ui(panel, ui);
        return;
    }
    let selected: Vec<BulkItem> = collect_selected(panel);
    let (pw, ph) = loader::resolve_bulk_page(settings.serper_bulk_size, &selected);
    let keyword = panel.query.trim().to_owned();
    let folder = dest.clone().or_else(|| loader::default_bulk_dir(&keyword));
    // En right_to_left lo primero queda más a la derecha: Add, luego Cancel.
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if let Some(folder) = folder.clone() {
            let label = format!("Add {} images", selected.len());
            if ui
                .add_enabled(!selected.is_empty(), egui::Button::new(label))
                .on_hover_text(format!(
                    "Creates one {}×{} canvas per image in {}",
                    pw.round(),
                    ph.round(),
                    folder.display()
                ))
                .clicked()
            {
                loader::begin_bulk_files(
                    panel,
                    selected,
                    folder,
                    keyword,
                    (pw, ph),
                    tx.clone(),
                    ctx.clone(),
                );
            }
        } else {
            ui.weak("No folder available");
        }
        if ui.button("Cancel").clicked() {
            panel.bulk_open = false;
        }
    });
    ui.weak(format!(
        "Canvas size: {}×{} (Settings → Web bulk canvas size)",
        pw.round(),
        ph.round()
    ));
    if let Some(msg) = panel.bulk_done_msg.clone() {
        ui.label(msg);
    }
    show_bulk_errors(panel, ui);
}

/// Progreso de la creación masiva, A LA DERECHA (spinner al borde). Si lleva
/// demasiado sin avanzar (respuesta perdida al cambiar de vista o hilo
/// caído), aviso con Reset en vez de spinner eterno.
fn bulk_progress_ui(panel: &mut Panel, ui: &mut egui::Ui) {
    use super::state::{stalled, BULK_STALL_SECS};
    let stuck = stalled(
        panel.bulk_progress_at,
        std::time::Instant::now(),
        BULK_STALL_SECS,
    );
    if panel.stalled_ui(stuck, ui) {
        return;
    }
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.spinner();
        match panel.bulk_progress {
            Some((done, total)) => ui.weak(format!("Saving {done}/{total}…")),
            None => ui.weak("Saving…"),
        };
    });
}

/// Fotos elegidas en el orden de la lista (para nombres estables).
fn collect_selected(panel: &Panel) -> Vec<BulkItem> {
    panel.selected_bulk_items()
}

/// Errores de la última tanda en desplegable colapsado (una línea cuando
/// está cerrado) para que el pie siga compacto.
fn show_bulk_errors(panel: &Panel, ui: &mut egui::Ui) {
    if panel.bulk_errors.is_empty() {
        return;
    }
    ui.collapsing(format!("{} failed", panel.bulk_errors.len()), |ui| {
        for err in panel.bulk_errors.iter().take(4) {
            ui.weak(err);
        }
        if panel.bulk_errors.len() > 4 {
            ui.weak(format!("…and {} more", panel.bulk_errors.len() - 4));
        }
    });
}
