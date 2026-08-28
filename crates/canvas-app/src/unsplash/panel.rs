//! UI de la pestaña «Images» del sidebar del editor: barra de búsqueda,
//! fila de filtros, lista vertical de tarjetas de foto y pie con «Load
//! more». El estado vive en `super::state::Panel`; la tarjeta individual en
//! `super::card`.

use std::sync::mpsc::Sender;

use eframe::egui;

use crate::editor::EditorState;
use crate::loader;

use super::api::access_key;
use super::card::photo_card_ui;
use super::state::Panel;
use super::types::{ColorFilter, OrderBy, Orientation};
use super::ACCESS_KEY_ENV;

/// Margen lateral (en puntos) a cada lado de las tarjetas de foto en la
/// lista: las imágenes quedan un poco más estrechas que el panel.
const CARD_INSET: f32 = 12.0;

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
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font.clone(), color);
    // Icono + texto como una unidad centrada en la píldora; el texto se
    // centra también en vertical (su esquina NO va al centro del botón).
    let total = icon_sz + gap + galley.size().x;
    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(
            rect.center().x - total * 0.5 + icon_sz * 0.5,
            rect.center().y,
        ),
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
