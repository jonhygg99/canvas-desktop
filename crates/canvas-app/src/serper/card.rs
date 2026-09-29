//! Tarjeta de foto del panel Web: portada «cover» con la fuente superpuesta,
//! clic para insertar y arrastre real hasta el lienzo para soltarla en una
//! posición concreta. Espejo de la tarjeta de Unsplash con dos diferencias:
//! la atribución es el dominio fuente (no hay fotógrafo) y el thumb se pide
//! de forma perezosa al pintarse por primera vez.

use std::sync::mpsc::Sender;

use eframe::egui;

use crate::loader;

use super::state::{DragSerper, PhotoItem};

/// Una tarjeta de la lista: la foto cubre TODA la tarjeta de borde a borde
/// (recorte «cover») y el dominio fuente va superpuesto abajo. Clic para
/// insertar centrada en el lienzo, o arrastrar hasta el lienzo para
/// soltarla en una posición concreta. El clic es «suave» (ver el helper de
/// arrastre): una pulsación simple nunca se convierte en arrastre.
pub(super) fn photo_card_ui(
    item: &mut PhotoItem,
    inserting: &mut Option<String>,
    w: f32,
    h: f32,
    ui: &mut egui::Ui,
    tx: &Sender<loader::AppMsg>,
) {
    let visuals = ui.visuals().clone();
    let photo = item.photo.clone();

    // Thumb perezoso: solo se pide la primera vez que la tarjeta se pinta
    // (lo que queda bajo el scroll no descarga nada).
    if item.claim_thumb() {
        loader::spawn_serper_thumb(
            photo.id.clone(),
            photo.image_url.clone(),
            tx.clone(),
            ui.ctx().clone(),
        );
    }

    let resp = card_drag_source(
        ui,
        egui::Id::new(("serper_card", photo.id.as_str())),
        DragSerper {
            id: photo.id.clone(),
            label: format!("Web · {}", photo.source_host()),
            url: photo.image_url.clone(),
        },
        |ui| paint_card(item, inserting, &photo_title(&photo), w, h, ui, &visuals),
    )
    .response;

    // Clic registrado DESPUÉS del drag source (queda ENCIMA): si un widget
    // de drag tapa a uno de clic, egui descarta el clic.
    let click = ui.interact(
        resp.rect,
        egui::Id::new(("serper_card_click", photo.id.as_str())),
        egui::Sense::click(),
    );
    if click.clicked() && inserting.is_none() {
        if item.thumb_error.is_some() {
            // Sin preview no hay nada que insertar: el clic reintenta la
            // descarga en vez de abrir una capa vacía (el hover lo avisa).
            item.retry_thumb();
            if item.claim_thumb() {
                loader::spawn_serper_thumb(
                    photo.id.clone(),
                    photo.image_url.clone(),
                    tx.clone(),
                    ui.ctx().clone(),
                );
            }
        } else {
            *inserting = Some(photo.id.clone());
            let label = format!("Web · {}", photo.source_host());
            loader::spawn_serper_image(
                photo.id,
                label,
                photo.image_url,
                tx.clone(),
                ui.ctx().clone(),
            );
        }
    }
    let hover = if item.thumb_error.is_some() {
        "Preview failed — click to retry"
    } else {
        "Click to insert · drag to the canvas to place it"
    };
    let _ = resp.on_hover_text(hover);
}

/// Título corto de la tarjeta: el título de Serper recortado (la URL no se
/// muestra; el dominio va en la barra inferior).
fn photo_title(photo: &super::types::SerperPhoto) -> String {
    const MAX: usize = 60;
    let mut title = photo.title.trim().to_owned();
    if title.is_empty() {
        title = photo.source_host();
    }
    if title.chars().count() > MAX {
        title = format!("{}…", title.chars().take(MAX).collect::<String>());
    }
    title
}

/// Pinta la tarjeta (este mismo pintado sirve de fantasma en el arrastre).
fn paint_card(
    item: &PhotoItem,
    inserting: &Option<String>,
    title: &str,
    w: f32,
    h: f32,
    ui: &mut egui::Ui,
    visuals: &egui::Visuals,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::click());
    ui.painter()
        .rect_filled(rect, 4.0, visuals.extreme_bg_color);

    if let Some(tex) = &item.thumb {
        paint_cover(ui, tex, rect);
    } else {
        let msg = match &item.thumb_error {
            Some(err) => format!(
                "no preview ({}) · click to retry",
                super::api::short_reason(err)
            ),
            None => "…".to_owned(),
        };
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            msg,
            egui::FontId::proportional(12.0),
            visuals.weak_text_color(),
        );
    }

    // Barra inferior con la fuente + pista de clic, sobre la propia foto.
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
        item.photo.source_host(),
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
    let _ = title;

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
    resp
}

/// La foto cubre la tarjeta entera (recorte «cover», sin huecos).
fn paint_cover(ui: &egui::Ui, tex: &egui::TextureHandle, rect: egui::Rect) {
    let img = tex.size_vec2();
    if img.x <= 0.0 || img.y <= 0.0 {
        return;
    }
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

/// Origen de arrastre con clic suave: el payload y el fantasma solo entran
/// en juego con arrastre REAL (más allá del umbral de clic). Espejo del
/// helper de la tarjeta de Unsplash (cada módulo posee el suyo).
fn card_drag_source<Payload, R>(
    ui: &mut egui::Ui,
    id: egui::Id,
    payload: Payload,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R>
where
    Payload: std::any::Any + Send + Sync,
{
    let dragging =
        ui.ctx().is_being_dragged(id) && ui.ctx().input(|i| i.pointer.is_decidedly_dragging());
    if dragging {
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
