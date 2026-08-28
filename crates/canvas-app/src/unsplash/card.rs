//! Tarjeta de foto del panel de Unsplash: portada «cover» con la atribución
//! superpuesta, clic para insertar y arrastre real hasta el lienzo para
//! soltarla en una posición concreta.

use std::sync::mpsc::Sender;

use eframe::egui;

use crate::loader;

use super::state::{DragUnsplash, PhotoItem};

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
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                }
            } else {
                let msg = if item.thumb_failed {
                    "no preview"
                } else {
                    "…"
                };
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
    let dragging =
        ui.ctx().is_being_dragged(id) && ui.ctx().input(|i| i.pointer.is_decidedly_dragging());
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
