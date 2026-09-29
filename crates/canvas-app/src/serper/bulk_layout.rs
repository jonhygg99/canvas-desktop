//! Matemáticas puras del masonry de la ventana masiva (sin red ni egui más
//! allá de medir texturas): nº de columnas, reparto por columna más baja,
//! aspecto de celda y pintado de la foto a su rect. `bulk.rs` orquesta la
//! ventana; aquí solo vive lo testeable sin contexto.

use eframe::egui;

use super::state::PhotoItem;

/// Ancho objetivo de columna para el cálculo responsive.
pub(super) const COL_TARGET: f32 = 170.0;
/// Separación entre columnas y entre celdas.
pub(super) const MASONRY_GAP: f32 = 8.0;
/// Altura de celda: las panorámicas no rompen la columna.
pub(super) const CELL_MIN_H: f32 = 60.0;
pub(super) const CELL_MAX_H: f32 = 420.0;

/// Nº de columnas responsive según el ancho disponible (2–4).
pub(super) fn column_count(available: f32) -> usize {
    ((available / COL_TARGET).floor() as usize).clamp(2, 4)
}

/// Ancho de columna repartiendo el hueco de los gaps.
pub(super) fn masonry_col_width(available: f32, cols: usize) -> f32 {
    ((available - MASONRY_GAP * (cols as f32 - 1.0)) / cols as f32).max(120.0)
}

/// Altura estimada de una celda a `col_w` (para repartir y para el
/// placeholder antes de que llegue el thumb).
pub(super) fn estimate_h(item: &PhotoItem, col_w: f32) -> f32 {
    let tex = item.thumb.as_ref().map(|t| t.size_vec2());
    (col_w / cell_aspect(tex, item.photo.width, item.photo.height)).clamp(CELL_MIN_H, CELL_MAX_H)
}

/// Aspecto (ancho/alto) de una foto: thumb GPU si ya llegó, si no dims de
/// la API, si no 4:3.
pub(super) fn cell_aspect(tex: Option<egui::Vec2>, api_w: Option<u32>, api_h: Option<u32>) -> f32 {
    if let Some(s) = tex {
        if s.x > 0.0 && s.y > 0.0 {
            return s.x / s.y;
        }
    }
    match (api_w.unwrap_or(0), api_h.unwrap_or(0)) {
        (w, h) if w > 0 && h > 0 => w as f32 / h as f32,
        _ => 4.0 / 3.0,
    }
}

/// Reparte índices entre columnas, cada uno a la más baja acumulada (con
/// alturas iguales equivale a round-robin).
pub(super) fn assign_columns(heights: &[f32], cols: usize) -> Vec<Vec<usize>> {
    let cols = cols.max(1);
    let mut out: Vec<Vec<usize>> = vec![Vec::new(); cols];
    let mut acc = vec![0.0f32; cols];
    for (i, h) in heights.iter().enumerate() {
        let mut best = 0;
        for (c, a) in acc.iter().enumerate() {
            if *a < acc[best] {
                best = c;
            }
        }
        out[best].push(i);
        acc[best] += h.max(0.0);
    }
    out
}

/// Altura real de la celda: aspecto del thumb si ya está, si no la estimada
/// por dims API (estable desde el primer frame).
pub(super) fn masonry_h(tex: &Option<egui::TextureHandle>, est_h: f32, col_w: f32) -> f32 {
    match tex {
        Some(t) => {
            let s = t.size_vec2();
            if s.x > 0.0 && s.y > 0.0 {
                return (col_w * s.y / s.x).clamp(CELL_MIN_H, CELL_MAX_H);
            }
            est_h
        }
        None => est_h,
    }
}

/// Pinta la foto a su rect exacto, con borde azul si está seleccionada.
pub(super) fn paint_masonry_image(
    ui: &egui::Ui,
    tex: &egui::TextureHandle,
    rect: egui::Rect,
    selected: bool,
) {
    let tint = if selected {
        egui::Color32::WHITE
    } else {
        egui::Color32::from_white_alpha(170)
    };
    ui.painter().image(
        tex.id(),
        rect,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        tint,
    );
    if selected {
        ui.painter().rect_stroke(
            rect,
            4.0,
            egui::Stroke::new(2.0, egui::Color32::from_rgb(0, 122, 255)),
            egui::StrokeKind::Inside,
        );
    }
}
