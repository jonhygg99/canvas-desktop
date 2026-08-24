//! Panel lateral izquierdo del editor: pestañas Page y Layers en
//! disposición vertical (icono + texto), barra de iconos en estado
//! colapsado.

use canvas_core::{LayerContent, LayerId, Page};
use eframe::egui;

use crate::editor::properties_panel::page::page_ui;
use crate::editor::state::LeftTab;
use crate::editor::EditorState;
use crate::sidebar;

mod ops;
mod row;

pub(crate) use ops::{group_selection, ungroup_selection};

use ops::{apply_reorder, toolbar_ui};
use row::row_ui;

struct Row {
    id: LayerId,
    depth: usize,
    is_group: bool,
    collapsed: bool,
}

fn push_rows(page: &Page, parent: Option<LayerId>, depth: usize, out: &mut Vec<Row>) {
    if depth > 64 {
        return;
    }
    for id in page.children_of(parent).into_iter().rev() {
        let Some(layer) = page.layer(id) else {
            continue;
        };
        let is_group = matches!(layer.content, LayerContent::Group(_));
        let collapsed = match &layer.content {
            LayerContent::Group(g) => g.collapsed,
            _ => false,
        };
        out.push(Row {
            id,
            depth,
            is_group,
            collapsed,
        });
        if is_group && !collapsed {
            push_rows(page, Some(id), depth + 1, out);
        }
    }
}

#[derive(Debug, Clone)]
struct DragLayers(Vec<LayerId>);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Drop {
    Above(LayerId),
    Below(LayerId),
    Into(LayerId),
}

const STRIP_WIDTH: f32 = 36.0;

pub fn left_panel_ui(state: &mut EditorState, ui: &mut egui::Ui, layers_collapsed: &mut bool) {
    sidebar::compact(ui);
    ui.horizontal(|ui| {
        vertical_tab_strip_ui(ui, &mut state.active_left_tab, layers_collapsed);
        ui.separator();
        ui.vertical(|ui| match state.active_left_tab {
            LeftTab::Page => {
                page_ui(state, ui);
            }
            LeftTab::Layers => {
                toolbar_ui(state, ui);
                ui.separator();
                let Ok(page) = state.doc.page() else {
                    ui.weak("No document.");
                    return;
                };
                let mut rows = Vec::new();
                push_rows(page, None, 0, &mut rows);
                let is_empty = rows.is_empty();
                let mut pending_drop: Option<(Vec<LayerId>, Drop)> = None;
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for row in &rows {
                        if let Some(drop) = row_ui(state, ui, row) {
                            pending_drop = Some(drop);
                        }
                    }
                    if is_empty {
                        ui.weak("No layers yet.");
                    }
                });
                if let Some((ids, drop)) = pending_drop {
                    apply_reorder(state, &ids, drop);
                }
            }
        });
    });
}

fn vertical_tab_strip_ui(ui: &mut egui::Ui, active_tab: &mut LeftTab, layers_collapsed: &mut bool) {
    let tab_font = egui::FontId::proportional(9.0);
    let strong = ui.visuals().strong_text_color();
    let active_c = ui.visuals().widgets.active.text_color();
    let inactive_c = ui.visuals().widgets.inactive.text_color();
    let tab_h = 52.0;
    let icon_size = 16.0;

    let painter = ui.painter();
    let page_galley = painter.layout_no_wrap("Page".into(), tab_font.clone(), egui::Color32::BLACK);
    let layers_galley =
        painter.layout_no_wrap("Layers".into(), tab_font.clone(), egui::Color32::BLACK);
    let _ = painter;

    let available_h = ui.available_height();
    let collapse_h = 28.0;
    let tabs_total_h = tab_h * 2.0 + collapse_h + 12.0;
    let top_margin = 8.0_f32.max((available_h - tabs_total_h) * 0.15);

    let (strip_rect, _) =
        ui.allocate_exact_size(egui::vec2(STRIP_WIDTH, available_h), egui::Sense::hover());

    let mut y = strip_rect.top() + top_margin;

    // --- Page tab ---
    let page_rect = egui::Rect::from_min_size(
        egui::pos2(strip_rect.left(), y),
        egui::vec2(STRIP_WIDTH, tab_h),
    );
    let page_resp = ui.allocate_rect(page_rect, egui::Sense::click());
    y += tab_h;
    draw_vertical_tab(
        ui.painter(),
        page_rect,
        &page_galley,
        "Page",
        tab_font.clone(),
        *active_tab == LeftTab::Page,
        page_resp.hovered(),
        strong,
        active_c,
        inactive_c,
        draw_page_icon as fn(&egui::Painter, egui::Rect, egui::Color32),
        icon_size,
    );
    if page_resp.clicked() {
        *active_tab = LeftTab::Page;
    }
    page_resp.on_hover_text("Page settings");

    // --- Layers tab ---
    let layers_rect = egui::Rect::from_min_size(
        egui::pos2(strip_rect.left(), y),
        egui::vec2(STRIP_WIDTH, tab_h),
    );
    let layers_resp = ui.allocate_rect(layers_rect, egui::Sense::click());
    let _ = y; // keep for any future tabs
    draw_vertical_tab(
        ui.painter(),
        layers_rect,
        &layers_galley,
        "Layers",
        tab_font.clone(),
        *active_tab == LeftTab::Layers,
        layers_resp.hovered(),
        strong,
        active_c,
        inactive_c,
        draw_layers_icon as fn(&egui::Painter, egui::Rect, egui::Color32),
        icon_size,
    );
    if layers_resp.clicked() {
        *active_tab = LeftTab::Layers;
    }
    layers_resp.on_hover_text("Layers");

    // --- Collapse button at bottom ---
    let collapse_rect = egui::Rect::from_center_size(
        egui::pos2(
            strip_rect.center().x,
            strip_rect.bottom() - collapse_h / 2.0 - 4.0,
        ),
        egui::vec2(16.0, 16.0),
    );
    let collapse_resp = ui.allocate_rect(collapse_rect, egui::Sense::click());
    let col_color = if collapse_resp.hovered() {
        active_c
    } else {
        inactive_c
    };
    collapse_resp.clone().on_hover_text("Collapse panel");
    if collapse_resp.clicked() {
        *layers_collapsed = true;
    }
    draw_left_triangle(ui.painter(), collapse_rect, col_color);
}

#[allow(clippy::too_many_arguments)]
fn draw_vertical_tab(
    painter: &egui::Painter,
    rect: egui::Rect,
    galley: &egui::Galley,
    _label: &str,
    font: egui::FontId,
    is_active: bool,
    hovered: bool,
    strong: egui::Color32,
    active_c: egui::Color32,
    inactive_c: egui::Color32,
    draw_icon: fn(&egui::Painter, egui::Rect, egui::Color32),
    icon_size: f32,
) {
    let is_dark = strong.r() > 128;
    if is_active {
        painter.rect_filled(
            rect,
            0.0,
            egui::Color32::from_gray(if is_dark { 45 } else { 225 }),
        );
        // Left edge accent
        let indicator = egui::Rect::from_min_size(rect.left_top(), egui::vec2(2.5, rect.height()));
        painter.rect_filled(indicator, 2.5, strong);
    }

    let color = if is_active {
        strong
    } else if hovered {
        active_c
    } else {
        inactive_c
    };

    // Label above icon
    let text_y = rect.center().y - icon_size * 0.35 - galley.size().y;
    painter.text(
        egui::pos2(rect.center().x, text_y),
        egui::Align2::CENTER_CENTER,
        galley.text(),
        font,
        color,
    );

    // Icon below label
    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(rect.center().x, rect.center().y + icon_size * 0.3),
        egui::vec2(icon_size, icon_size),
    );
    draw_icon(painter, icon_rect, color);
}

pub fn collapsed_tab_ui(ui: &mut egui::Ui, active_tab: &mut LeftTab, layers_collapsed: &mut bool) {
    sidebar::compact(ui);
    let width = ui.available_width().max(1.0);
    let margin = 4.0;
    let gap = 6.0;
    let icon_size = width - margin * 2.0;
    let mut y = margin;
    for (tab, draw_fn, tip) in [
        (
            LeftTab::Page,
            draw_page_icon as fn(&egui::Painter, egui::Rect, egui::Color32),
            "Page",
        ),
        (
            LeftTab::Layers,
            draw_layers_icon as fn(&egui::Painter, egui::Rect, egui::Color32),
            "Layers",
        ),
    ] {
        let tab_rect =
            egui::Rect::from_min_size(egui::pos2(margin, y), egui::vec2(icon_size, icon_size));
        let resp = ui.allocate_rect(tab_rect, egui::Sense::click());
        if resp.clicked() {
            *active_tab = tab;
            *layers_collapsed = false;
        }
        let color = if resp.hovered() {
            ui.visuals().widgets.active.text_color()
        } else if *active_tab == tab {
            ui.visuals().strong_text_color()
        } else {
            ui.visuals().widgets.inactive.text_color()
        };
        draw_fn(ui.painter(), tab_rect, color);
        resp.on_hover_text(tip);
        y += icon_size + gap;
    }
}

fn draw_left_triangle(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let c = rect.center();
    let s = rect.width().min(rect.height()) * 0.32;
    let points = vec![
        c + egui::vec2(-s, 0.0),
        c + egui::vec2(s * 0.7, -s),
        c + egui::vec2(s * 0.7, s),
    ];
    painter.add(egui::Shape::convex_polygon(
        points,
        color,
        egui::Stroke::NONE,
    ));
}

fn draw_page_icon(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let stroke = egui::Stroke::new(1.3, color);
    let r = rect.shrink(2.0);
    let s = r.width().min(r.height()) * 0.55;
    let page_rect = egui::Rect::from_center_size(r.center(), egui::vec2(s * 0.9, s));
    painter.rect_stroke(page_rect, 2.0, stroke, egui::StrokeKind::Outside);
    let fold = s * 0.22;
    painter.line_segment(
        [
            egui::pos2(page_rect.right() - fold, page_rect.top()),
            egui::pos2(page_rect.right(), page_rect.top() + fold),
        ],
        stroke,
    );
}

fn draw_layers_icon(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let stroke = egui::Stroke::new(1.3, color);
    let r = rect.shrink(2.0);
    let s = r.width().min(r.height()) * 0.52;
    let ox = s * 0.25;
    let oy = s * 0.25;
    let back = egui::Rect::from_center_size(r.center() - egui::vec2(ox, oy), egui::vec2(s, s));
    painter.rect_stroke(back, 2.0, stroke, egui::StrokeKind::Outside);
    let mid = egui::Rect::from_center_size(r.center(), egui::vec2(s, s));
    painter.rect_stroke(mid, 2.0, stroke, egui::StrokeKind::Outside);
    let front = egui::Rect::from_center_size(r.center() + egui::vec2(ox, oy), egui::vec2(s, s));
    painter.rect_stroke(front, 2.0, stroke, egui::StrokeKind::Outside);
}
