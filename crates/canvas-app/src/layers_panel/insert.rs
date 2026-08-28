//! Pestaña Insert del panel lateral: cuadrícula de cajas visuales con la
//! silueta de cada elemento a insertar (texto y formas). Los clics llaman a
//! las mismas `insert_layer_centered` que los antiguos botones de texto.

use eframe::egui;

use canvas_core::LayerContent;

use crate::app_icons::{
    draw_arrow_preview, draw_cross_preview, draw_diamond_preview, draw_ellipse_preview,
    draw_heart_preview, draw_hexagon_preview, draw_line_preview, draw_pentagon_preview,
    draw_rect_preview, draw_star_preview, draw_text_preview, draw_triangle_preview,
};
use crate::editor::EditorState;
use crate::sidebar;

/// Altura de las cajas de la cuadrícula Insert (el ancho es la mitad del
/// panel: dos columnas, cada elemento ocupa el 50 % del ancho).
pub(super) const INSERT_TILE_H: f32 = 64.0;

/// Una entrada de la cuadrícula Insert: qué se inserta y cómo se pinta.
pub(super) struct InsertItem {
    pub(super) label: &'static str,
    pub(super) tip: &'static str,
    pub(super) draw: fn(&egui::Painter, egui::Rect, egui::Color32),
}

pub(super) const INSERT_ITEMS: [InsertItem; 12] = [
    InsertItem {
        label: "Text",
        tip: "Text",
        draw: draw_text_preview,
    },
    InsertItem {
        label: "Rect",
        tip: "Rectangle",
        draw: draw_rect_preview,
    },
    InsertItem {
        label: "Ellipse",
        tip: "Ellipse",
        draw: draw_ellipse_preview,
    },
    InsertItem {
        label: "Line",
        tip: "Line",
        draw: draw_line_preview,
    },
    InsertItem {
        label: "Triangle",
        tip: "Triangle",
        draw: draw_triangle_preview,
    },
    InsertItem {
        label: "Star",
        tip: "Star",
        draw: draw_star_preview,
    },
    InsertItem {
        label: "Arrow",
        tip: "Arrow",
        draw: draw_arrow_preview,
    },
    InsertItem {
        label: "Pentagon",
        tip: "Pentagon",
        draw: draw_pentagon_preview,
    },
    InsertItem {
        label: "Hexagon",
        tip: "Hexagon",
        draw: draw_hexagon_preview,
    },
    InsertItem {
        label: "Diamond",
        tip: "Diamond",
        draw: draw_diamond_preview,
    },
    InsertItem {
        label: "Cross",
        tip: "Cross",
        draw: draw_cross_preview,
    },
    InsertItem {
        label: "Heart",
        tip: "Heart",
        draw: draw_heart_preview,
    },
];

/// Inserta una capa centrada según la etiqueta del ítem del panel Insert.
pub(super) fn insert_item(state: &mut EditorState, label: &str) {
    match label {
        "Text" => state.insert_layer_centered(
            "Text",
            500.0,
            120.0,
            LayerContent::Text(canvas_core::TextContent::default()),
        ),
        "Rect" => state.insert_layer_centered(
            "Rectangle",
            320.0,
            220.0,
            LayerContent::Shape(canvas_core::ShapeContent::default()),
        ),
        "Ellipse" => state.insert_layer_centered(
            "Ellipse",
            280.0,
            280.0,
            LayerContent::Shape(canvas_core::ShapeContent {
                kind: canvas_core::ShapeKind::Ellipse,
                ..Default::default()
            }),
        ),
        "Line" => state.insert_layer_centered(
            "Line",
            400.0,
            48.0,
            LayerContent::Shape(canvas_core::ShapeContent {
                kind: canvas_core::ShapeKind::Line,
                stroke: [30, 30, 30, 255],
                stroke_width: 16.0,
                corner_radius: 8.0,
                ..Default::default()
            }),
        ),
        "Triangle" => state.insert_layer_centered(
            "Triangle",
            320.0,
            280.0,
            LayerContent::Shape(canvas_core::ShapeContent {
                kind: canvas_core::ShapeKind::Triangle,
                ..Default::default()
            }),
        ),
        "Star" => state.insert_layer_centered(
            "Star",
            320.0,
            300.0,
            LayerContent::Shape(canvas_core::ShapeContent {
                kind: canvas_core::ShapeKind::Star,
                ..Default::default()
            }),
        ),
        "Pentagon" => state.insert_layer_centered(
            "Pentagon",
            320.0,
            300.0,
            LayerContent::Shape(canvas_core::ShapeContent {
                kind: canvas_core::ShapeKind::Pentagon,
                ..Default::default()
            }),
        ),
        "Hexagon" => state.insert_layer_centered(
            "Hexagon",
            320.0,
            280.0,
            LayerContent::Shape(canvas_core::ShapeContent {
                kind: canvas_core::ShapeKind::Hexagon,
                ..Default::default()
            }),
        ),
        "Diamond" => state.insert_layer_centered(
            "Diamond",
            280.0,
            280.0,
            LayerContent::Shape(canvas_core::ShapeContent {
                kind: canvas_core::ShapeKind::Diamond,
                ..Default::default()
            }),
        ),
        "Cross" => state.insert_layer_centered(
            "Cross",
            300.0,
            300.0,
            LayerContent::Shape(canvas_core::ShapeContent {
                kind: canvas_core::ShapeKind::Cross,
                ..Default::default()
            }),
        ),
        "Heart" => state.insert_layer_centered(
            "Heart",
            300.0,
            280.0,
            LayerContent::Shape(canvas_core::ShapeContent {
                kind: canvas_core::ShapeKind::Heart,
                ..Default::default()
            }),
        ),
        _ => state.insert_layer_centered(
            "Arrow",
            400.0,
            200.0,
            LayerContent::Shape(canvas_core::ShapeContent {
                kind: canvas_core::ShapeKind::Arrow,
                stroke_width: 16.0,
                corner_radius: 28.0,
                ..Default::default()
            }),
        ),
    }
}

/// Pestaña Insert: cuadrícula de cajas visuales con la silueta de cada
/// elemento a insertar (texto y formas). Los clics llaman a las mismas
/// `insert_layer_centered` que los antiguos botones de texto.
pub(super) fn insert_tab_ui(state: &mut EditorState, ui: &mut egui::Ui) {
    let visuals = ui.visuals().clone();
    // Ancho exacto de cada tile: mitad del panel menos el padding lateral
    // y el espacio entre columnas. El tile se pinta con el painter sobre
    // un rect calculado a mano, así que no dependemos del layout de egui
    // para el posicionamiento horizontal.
    let pad = sidebar::PANEL_PAD * 2.0;
    let gap = 8.0;
    let tile_w = ((ui.available_width() - pad - gap) * 0.5).max(1.0);
    let row_h = INSERT_TILE_H + 10.0;
    let mut i = 0;
    while i < INSERT_ITEMS.len() {
        let left = &INSERT_ITEMS[i];
        let right = if i + 1 < INSERT_ITEMS.len() {
            Some(&INSERT_ITEMS[i + 1])
        } else {
            None
        };
        // Reservamos una fila completa para ambas columnas.
        let (row_rect, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), row_h),
            egui::Sense::hover(),
        );
        let x0 = row_rect.left() + pad / 2.0;
        let y0 = row_rect.top();
        let left_rect =
            egui::Rect::from_min_size(egui::pos2(x0, y0), egui::vec2(tile_w, INSERT_TILE_H));
        paint_insert_tile(ui, left, &visuals, left_rect, state);
        if let Some(item) = right {
            let right_rect = egui::Rect::from_min_size(
                egui::pos2(x0 + tile_w + gap, y0),
                egui::vec2(tile_w, INSERT_TILE_H),
            );
            paint_insert_tile(ui, item, &visuals, right_rect, state);
        }
        i += 2;
    }
}

/// Pinta un tile de Insert en un rect pre-calculado y gestiona el clic.
/// Usa `ui.interact` sobre el rect para detectar hover/click sin que
/// el layout de egui modifique el ancho.
fn paint_insert_tile(
    ui: &mut egui::Ui,
    item: &InsertItem,
    visuals: &egui::Visuals,
    rect: egui::Rect,
    state: &mut EditorState,
) {
    let resp = ui.interact(
        rect,
        egui::Id::new(("ins_tile", item.label)),
        egui::Sense::click(),
    );
    let bg = if resp.hovered() {
        visuals.widgets.hovered.bg_fill
    } else {
        visuals.widgets.inactive.bg_fill
    };
    ui.painter().rect(
        rect,
        8.0,
        bg,
        visuals.widgets.inactive.bg_stroke,
        egui::StrokeKind::Inside,
    );
    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(rect.center().x, rect.center().y - 6.0),
        egui::vec2(rect.width() - 20.0, rect.height() - 26.0),
    );
    let color = if resp.hovered() {
        visuals.widgets.active.text_color()
    } else {
        visuals.widgets.inactive.text_color()
    };
    (item.draw)(ui.painter(), icon_rect, color);
    ui.painter().text(
        egui::pos2(rect.center().x, rect.bottom() - 8.0),
        egui::Align2::CENTER_CENTER,
        item.label,
        egui::FontId::proportional(11.0),
        color,
    );
    let clicked = resp.clicked();
    resp.on_hover_text(item.tip);
    if clicked {
        insert_item(state, item.label);
    }
}
