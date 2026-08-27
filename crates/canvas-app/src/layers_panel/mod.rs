//! Panel lateral izquierdo del editor: pestañas Page y Layers en
//! disposición vertical solo con icono (el nombre es la tooltip y el
//! título del menú que hay debajo), barra de iconos en estado
//! colapsado. Las pestañas son arrastrables entre sí
//! para reordenarlas y el orden queda persistido en los ajustes.

use canvas_core::{LayerContent, LayerId, Page};
use eframe::egui;

use crate::app_icons::{
    draw_arrow_preview, draw_cross_preview, draw_diamond_preview, draw_ellipse_preview,
    draw_heart_preview, draw_hexagon_preview, draw_images_icon, draw_layers_icon,
    draw_line_preview, draw_page_icon, draw_pentagon_preview, draw_rect_preview,
    draw_sparkle_icon, draw_star_preview, draw_text_preview, draw_triangle_preview,
};
use crate::editor::properties_panel::page::page_ui;
use crate::editor::state::LeftTab;
use crate::editor::EditorState;
use crate::settings::LayersTabOrder;
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

/// Devuelve el nuevo orden si una pestaña se soltó sobre la otra; el
/// llamador persiste el cambio en los ajustes.
pub fn left_panel_ui(
    state: &mut EditorState,
    ui: &mut egui::Ui,
    layers_collapsed: &mut bool,
    order: LayersTabOrder,
    tx: &std::sync::mpsc::Sender<crate::loader::AppMsg>,
) -> Option<LayersTabOrder> {
    sidebar::compact(ui);
    let mut new_order = None;
    // `ui.horizontal` arranca con altura = `interact_size.y` (22pt) y limita
    // a sus hijos a esa altura inicial: la tira de pestañas se dibuja
    // desbordando (el clic es geométrico, no de layout) pero el contenido
    // del panel —listas y scroll— quedaba aplastado a ~0pt. Con
    // `allocate_ui_with_layout` el layout horizontal recibe TODA la altura
    // disponible del panel y el contenido ocupa el vertical completo.
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), ui.available_height()),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
        new_order = vertical_tab_strip_ui(
            ui,
            &mut state.active_left_tab,
            layers_collapsed,
            order,
            false,
        );
        ui.separator();
        // El nombre de la pestaña activa es también el TÍTULO del menú:
        // la tira quedó solo con iconos, y el título vuelve aquí, encima
        // de los elementos, con algo de aire arriba.
        ui.vertical(|ui| {
            ui.add_space(8.0);
            let tab_name = match state.active_left_tab {
                LeftTab::Page => "Page",
                LeftTab::Layers => "Layers",
                LeftTab::Insert => "Insert",
                LeftTab::Images => "Images",
            };
            sidebar::title(ui, tab_name);
            ui.add_space(6.0);
            match state.active_left_tab {
                LeftTab::Page => {
                    page_ui(state, ui);
                }
                LeftTab::Images => crate::unsplash::panel_ui(state, ui, tx),
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
                LeftTab::Insert => insert_tab_ui(state, ui),
            }
        });
    });
    new_order
}

/// Estado de un arrastre manual de pestaña, persistido entre frames en el
/// `data` de egui (la función de UI en sí no guarda estado).
#[derive(Clone)]
struct TabDrag {
    tab: LeftTab,
    /// Dónde se pulsó, dentro del rect de `tab`.
    origin: egui::Pos2,
    /// `true` en cuanto el puntero supera el umbral de clic: es un arrastre.
    moved: bool,
}

/// Estado de la animación de intercambio: en la soltada el orden lógico
/// cambia al instante y la pestaña arrastrada VUELA desde el punto de
/// soltura hasta su ranura nueva, mientras la otra se desliza a la suya
/// (ease-out cúbico, sin la interpolación de egui). La animación pide
/// repintado cada frame y arranca con el slide COMPLETO (la posición
/// anterior) en su primer frame visible, para que no haya ningún salto.
#[derive(Clone)]
struct TabSwapAnim {
    /// Reloj de egui (`InputState::time`) del instante de la soltada.
    start: f64,
    /// `false` hasta el primer frame que pinte la animación: ahí se reancla
    /// `start` para arrancar desde la posición anterior sin discontinuidad.
    started: bool,
    /// Pestaña que se estaba arrastrando (la que vuela desde el cursor).
    dragged: LeftTab,
    /// Punto (coordenadas de pantalla) donde se soltó: origen del vuelo.
    release_pos: egui::Pos2,
}

/// Duración del deslizamiento del intercambio de pestañas.
const SWAP_ANIM_SECS: f64 = 0.2;

/// Las cuatro pestañas: Page y Layers reordenables según los ajustes, e
/// Insert e Images fijas detrás con un separador delante de Insert.
fn ordered_tabs(order: LayersTabOrder) -> [LeftTab; 4] {
    let (first, second) = match order {
        LayersTabOrder::PageFirst => (LeftTab::Page, LeftTab::Layers),
        LayersTabOrder::LayersFirst => (LeftTab::Layers, LeftTab::Page),
    };
    [first, second, LeftTab::Insert, LeftTab::Images]
}

/// Altura de las cajas de la cuadrícula Insert (el ancho es la mitad del
/// panel: dos columnas, cada elemento ocupa el 50 % del ancho).
const INSERT_TILE_H: f32 = 64.0;

/// Una entrada de la cuadrícula Insert: qué se inserta y cómo se pinta.
struct InsertItem {
    label: &'static str,
    tip: &'static str,
    draw: fn(&egui::Painter, egui::Rect, egui::Color32),
}

const INSERT_ITEMS: [InsertItem; 12] = [
    InsertItem { label: "Text", tip: "Text", draw: draw_text_preview },
    InsertItem { label: "Rect", tip: "Rectangle", draw: draw_rect_preview },
    InsertItem { label: "Ellipse", tip: "Ellipse", draw: draw_ellipse_preview },
    InsertItem { label: "Line", tip: "Line", draw: draw_line_preview },
    InsertItem { label: "Triangle", tip: "Triangle", draw: draw_triangle_preview },
    InsertItem { label: "Star", tip: "Star", draw: draw_star_preview },
    InsertItem { label: "Arrow", tip: "Arrow", draw: draw_arrow_preview },
    InsertItem { label: "Pentagon", tip: "Pentagon", draw: draw_pentagon_preview },
    InsertItem { label: "Hexagon", tip: "Hexagon", draw: draw_hexagon_preview },
    InsertItem { label: "Diamond", tip: "Diamond", draw: draw_diamond_preview },
    InsertItem { label: "Cross", tip: "Cross", draw: draw_cross_preview },
    InsertItem { label: "Heart", tip: "Heart", draw: draw_heart_preview },
];

/// Inserta una capa centrada según la etiqueta del ítem del panel Insert.
fn insert_item(state: &mut EditorState, label: &str) {
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
fn insert_tab_ui(state: &mut EditorState, ui: &mut egui::Ui) {
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
        let left_rect = egui::Rect::from_min_size(
            egui::pos2(x0, y0),
            egui::vec2(tile_w, INSERT_TILE_H),
        );
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
    let resp = ui.interact(rect, egui::Id::new(("ins_tile", item.label)), egui::Sense::click());
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

/// Nombre de la pestaña (tooltip del icono).
fn tab_tip(tab: LeftTab) -> &'static str {
    match tab {
        LeftTab::Page => "Page settings",
        LeftTab::Layers => "Layers",
        LeftTab::Insert => "Insert",
        LeftTab::Images => "Images (Unsplash)",
    }
}

/// Id del widget hover de una pestaña: solo para tooltips y hover; el clic y
/// el arrastre los gestiona la máquina de estados manual de abajo.
fn tab_hover_id(tab: LeftTab) -> egui::Id {
    egui::Id::new(("left_tab", tab)).with("hover")
}

fn tab_icon(tab: LeftTab) -> fn(&egui::Painter, egui::Rect, egui::Color32) {
    match tab {
        LeftTab::Page => draw_page_icon,
        LeftTab::Layers => draw_layers_icon,
        LeftTab::Insert => draw_sparkle_icon,
        LeftTab::Images => draw_images_icon,
    }
}

const TAB_GAP: f32 = 8.0;

pub(crate) fn vertical_tab_strip_ui(
    ui: &mut egui::Ui,
    active_tab: &mut LeftTab,
    layers_collapsed: &mut bool,
    order: LayersTabOrder,
    collapsed: bool,
) -> Option<LayersTabOrder> {
    let strong = ui.visuals().strong_text_color();
    let active_c = ui.visuals().widgets.active.text_color();
    let inactive_c = ui.visuals().widgets.inactive.text_color();
    let hover_fill = ui.visuals().widgets.hovered.weak_bg_fill;
    // El estilo es común a todas las pestañas del pase (y al fantasma del
    // drag): se arma una vez y se presta a cada `draw_vertical_tab`.
    let tab_h = 64.0;
    let icon_size = 20.0;
    let style = TabStyle {
        strong,
        active_c,
        inactive_c,
        hover_fill,
        icon_size,
    };

    let available_h = ui.available_height();
    // Margen superior fijo, no centrado: así las pestañas no saltan de
    // posición cuando el panel pasa de colapsado a expandido (y viceversa)
    // — la animación de `show_switched` puede cambiar `available_h` a
    // medio frame y el centrado hacía que los iconos bailaran.
    let top_margin = 8.0;

    let (strip_rect, _) =
        ui.allocate_exact_size(egui::vec2(STRIP_WIDTH, available_h), egui::Sense::hover());

    // Estado del arrastre a través de los frames. El clic y el arrastre NO
    // usan el drag-and-drop de egui (dos widgets solapados peleándose por el
    // hit-test resultaba en clic muerto o en arrastre muerto): aquí todo se
    // decide por geometría directa del puntero, que no puede fallar.
    let drag_salt = egui::Id::new("left_tabs_drag");
    let mut drag: Option<TabDrag> = ui.data_mut(|d| d.get_temp(drag_salt).unwrap_or(None));
    let anim_salt = egui::Id::new("left_tabs_swap_anim");
    let mut anim: Option<TabSwapAnim> = ui.data_mut(|d| d.get_temp(anim_salt).unwrap_or(None));
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        drag = None;
    }

    // Rectos de las pestañas, en el orden de los ajustes.
    let mut tab_rects: Vec<(LeftTab, egui::Rect)> = Vec::with_capacity(4);
    let mut y = strip_rect.top() + top_margin;
    for tab in ordered_tabs(order).iter() {
        // Separador antes de Insert
        if *tab == LeftTab::Insert {
            y += TAB_GAP; // espacio extra como separador visual
        }
        let rect = egui::Rect::from_min_size(
            egui::pos2(strip_rect.left(), y),
            egui::vec2(STRIP_WIDTH, tab_h),
        );
        tab_rects.push((*tab, rect));
        y += tab_h + TAB_GAP;
    }

    // Animación de intercambio: el orden lógico cambió justo en la
    // soltada; aquí se calcula cuánto se alejan visualmente las pestañas de
    // su posición definitiva. Primer frame: se reancla el reloj y el slide
    // queda completo (la posición ANTERIOR) — sin salto de fotograma.
    // Luego decae a 0 con ease-out cúbico. Mientras esté activa, se pide
    // repintado cada frame: sin esto, al no haber input el glide no avanzaba
    // y las pestañas aparecían intercambiadas de golpe en el siguiente
    // evento (ese era el «buggy»).
    let now: f64 = ui.input(|i| i.time);
    let mut slide = 0.0f32;
    let mut swap: Option<TabSwapAnim> = None;
    if let Some(mut a) = anim.take() {
        if !a.started {
            a.start = now;
            a.started = true;
        }
        let t = ((now - a.start) / SWAP_ANIM_SECS).clamp(0.0, 1.0) as f32;
        if t >= 1.0 {
            anim = None;
        } else {
            let ease = 1.0 - (1.0 - t).powi(3);
            slide = (tab_h + TAB_GAP) * (1.0 - ease);
            ui.ctx().request_repaint();
            // Mantener la animación viva para el siguiente frame: `swap`
            // consume una copia y `anim` se vuelve a insertar abajo.
            swap = Some(a.clone());
            anim = Some(a);
        }
    }

    // Desvanecimiento del icono mientras las pestañas se cruzan: cuando
    // `slide` llega a la mitad del recorrido, las dos pestañas están
    // exactamente sobre el mismo sitio — el icono baja a ~35% de opacidad
    // con una rampa suave (smoothstep) y recupera el 100% a ambos lados.
    let icon_fade = if slide > 0.0 && tab_rects.len() == 2 {
        let m = (tab_h + TAB_GAP) * 0.5;
        let d = ((slide - m).abs() / m).clamp(0.0, 1.0);
        let d = d * d * (3.0 - 2.0 * d);
        0.35 + 0.65 * d
    } else {
        1.0
    };

    // Arranque: el frame en el que el botón primario baja dentro de una
    // pestaña (aún no sabemos si será un clic o un arrastre). Se ignora
    // mientras la animación de intercambio está en curso: las pestañas
    // están desplazadas visualmente y sus rectos lógicos no corresponden
    // con lo que el usuario ve, así que un pulsado ahí sería ambiguo.
    if drag.is_none() && slide == 0.0 {
        if let Some(origin) = ui.input(|i| i.pointer.press_origin()) {
            if let Some(tab) = tab_rects
                .iter()
                .find(|(_, r)| r.contains(origin))
                .map(|(t, _)| *t)
            {
                drag = Some(TabDrag {
                    tab,
                    origin,
                    moved: false,
                });
            }
        }
    }

    // Umbral: superado el radio de un clic, es un arrastre.
    if let Some(d) = drag.as_mut() {
        if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
            if d.origin.distance(pos) > 6.0 {
                d.moved = true;
            }
        }
    }

    let hover_pos = ui.input(|i| i.pointer.hover_pos());
    let mut new_order = None;

    // Pintar + hover + destino del arrastre. Durante la animación de
    // intercambio la pestaña ARRASTRADA vuela desde el punto de la soltada
    // (`release_pos`, donde estaba su fantasma) hasta su ranura definitiva,
    // y la OTRA se desliza (`slide`) desde su posición anterior: arrancan
    // las dos en el sitio exacto donde se les vio y terminan permutadas.
    for (i, (tab, rect)) in tab_rects.iter().enumerate() {
        let is_active = *active_tab == *tab;
        let is_drop_target = drag.as_ref().is_some_and(|d| d.moved && d.tab != *tab)
            && hover_pos.is_some_and(|p| rect.contains(p));

        let mut visual_rect = *rect;
        if let Some(sw) = &swap {
            if *tab == sw.dragged && tab_rects.len() == 2 {
                // La arrastrada: desde el cursor hasta el centro de su ran.
                let ease = (slide / (tab_h + TAB_GAP)).clamp(0.0, 1.0);
                let center = rect.center().lerp(sw.release_pos, 1.0 - ease);
                visual_rect = egui::Rect::from_center_size(center, rect.size());
            } else {
                let dy = if i == 0 { slide } else { -slide };
                visual_rect = rect.translate(egui::vec2(0.0, dy));
            }
        }

        let resp = ui.interact(visual_rect, tab_hover_id(*tab), egui::Sense::hover());
        if resp.hovered() && drag.is_none() {
            ui.painter().rect_filled(visual_rect, 4.0, hover_fill);
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        draw_vertical_tab(
            ui.painter(),
            visual_rect,
            is_active,
            false,
            &style,
            tab_icon(*tab),
            icon_fade,
        );
        if is_drop_target {
            // El destino se marca en el recto lógico (geométrico), no en el
            // desplazado: durante un arrastre no hay animación en curso.
            ui.painter().rect_stroke(
                *rect,
                4.0,
                egui::Stroke::new(1.5, egui::Color32::from_rgb(0, 122, 255)),
                egui::StrokeKind::Inside,
            );
        }
        if drag.is_none() {
            let _ = resp.on_hover_text(tab_tip(*tab));
        }
    }

    // Fantasma en el cursor mientras se arrastra.
    if let Some(d) = &drag {
        if d.moved {
            if let Some(pos) = hover_pos {
                let ghost = egui::Rect::from_center_size(
                    pos + egui::vec2(0.0, -10.0),
                    egui::vec2(STRIP_WIDTH - 6.0, tab_h - 6.0),
                );
                ui.painter().rect_filled(
                    ghost,
                    6.0,
                    egui::Color32::from_rgba_unmultiplied(0, 122, 255, 40),
                );
                // Fantasma: siempre inactivo y sin desvanecer, pero usa el
                // color ACTIVO como tinta del icono (así era antes del
                // agrupado: se pasa `active_c` también en la ranura de
                // `inactive_c`).
                let ghost_style = TabStyle {
                    inactive_c: style.active_c,
                    ..style
                };
                draw_vertical_tab(
                    ui.painter(),
                    ghost,
                    false,
                    false,
                    &ghost_style,
                    tab_icon(d.tab),
                    1.0,
                );
            }
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        }
    }

    // Soltada: sobre otra pestaña y tras haber arrastrado -> intercambiar;
    // sin haber arrastrado -> clic, activar la pestaña pulsada.
    if ui.input(|i| i.pointer.primary_released()) {
        if let Some(d) = drag.take() {
            if d.moved {
                if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
                    if let Some(target) = tab_rects
                        .iter()
                        .find(|(_, r)| r.contains(pos))
                        .map(|(t, _)| *t)
                    {
                        if target != d.tab {
                            new_order = Some(order.swapped());
                            // Ya con el nuevo orden, la próxima vez que se
                            // pinte la tira la arrastrada saldrá del cursor
                            // y la otra del otro lado; `started: false` hace
                            // que el primer fotograma reancle el reloj SIN
                            // saltarse la posición inicial, y
                            // `request_repaint` mantiene el glide en marcha
                            // aunque no haya input.
                            anim = Some(TabSwapAnim {
                                start: now,
                                started: false,
                                dragged: d.tab,
                                // El fantasma se dibuja 10 px por encima del
                                // puntero: el vuelo sale exactamente de ahí.
                                release_pos: pos + egui::vec2(0.0, -10.0),
                            });
                        }
                    }
                }
            } else if collapsed {
                *active_tab = d.tab;
                *layers_collapsed = false;
            } else if d.tab == *active_tab {
                // Clic en la pestaña ya activa: colapsar en vez de no hacer nada.
                *layers_collapsed = true;
            } else {
                *active_tab = d.tab;
            }
        }
    }
    // Red de seguridad: si el puntero ya no está pulsando nada, olvidar el
    // gesto (por ejemplo si la pulsación se perdió fuera de la ventana).
    if !ui.input(|i| i.pointer.any_down()) {
        drag = None;
    }
    ui.data_mut(|d| d.insert_temp(drag_salt, drag));
    ui.data_mut(|d| d.insert_temp(anim_salt, anim));

    new_order
}

/// Estilo fijo de las pestañas verticales de la tira del panel: idéntico
/// para todas las llamadas de un mismo pase. Lo que varía por llamada
/// (rect, estado, icono, fundido) queda fuera. Agrupado para reducir la
/// firma de `draw_vertical_tab` de 11 a 7 parámetros.
#[derive(Clone, Copy)]
struct TabStyle {
    strong: egui::Color32,
    active_c: egui::Color32,
    inactive_c: egui::Color32,
    hover_fill: egui::Color32,
    icon_size: f32,
}

/// `icon_fade` (1.0 = opaco) atenúa solo el color del icono, no el fondo:
/// durante el cruce del intercambio, cuanto más cerca están las dos
/// pestañas de solaparse, más transparente se vuelve su icono.
fn draw_vertical_tab(
    painter: &egui::Painter,
    rect: egui::Rect,
    is_active: bool,
    hovered: bool,
    style: &TabStyle,
    draw_icon: fn(&egui::Painter, egui::Rect, egui::Color32),
    icon_fade: f32,
) {
    let is_dark = style.strong.r() > 128;
    if is_active {
        painter.rect_filled(
            rect,
            0.0,
            egui::Color32::from_gray(if is_dark { 45 } else { 225 }),
        );
        // Left edge accent
        let indicator = egui::Rect::from_min_size(rect.left_top(), egui::vec2(2.5, rect.height()));
        painter.rect_filled(indicator, 2.5, style.strong);
    } else if hovered {
        // Fondo sutil al hover: solo-icono, el área debe leerse clicable.
        painter.rect_filled(rect, 4.0, style.hover_fill);
    }

    let color = if is_active {
        style.strong
    } else if hovered {
        style.active_c
    } else {
        style.inactive_c
    };
    let color = color.gamma_multiply(icon_fade);

    // Icono centrado en la pestaña
    let icon_rect =
        egui::Rect::from_center_size(rect.center(), egui::vec2(style.icon_size, style.icon_size));
    draw_icon(painter, icon_rect, color);
}

#[cfg(test)]
mod tests {
    use super::*;
    use canvas_core::{LayerId, Selection, ShapeKind};
    use crate::settings::LayersTabOrder;
    use eframe::egui;

    /// Lo que debe crear `insert_item` para cada etiqueta del panel Insert:
    /// nombre de la capa, tamaño y tipo de contenido. Espejo de `insert_item`
    /// para detectar cualquier desvío entre lo que ofrece la cuadrícula y lo
    /// que realmente se inserta.
    struct InsertCase {
        label: &'static str,
        name: &'static str,
        w: f64,
        h: f64,
        kind: LayerKind,
    }

    #[derive(Clone, Copy, Debug, PartialEq)]
    enum LayerKind {
        Text,
        Shape(ShapeKind),
    }

    const INSERT_CASES: [InsertCase; 12] = [
        InsertCase { label: "Text", name: "Text", w: 500.0, h: 120.0, kind: LayerKind::Text },
        InsertCase { label: "Rect", name: "Rectangle", w: 320.0, h: 220.0, kind: LayerKind::Shape(ShapeKind::Rect) },
        InsertCase { label: "Ellipse", name: "Ellipse", w: 280.0, h: 280.0, kind: LayerKind::Shape(ShapeKind::Ellipse) },
        InsertCase { label: "Line", name: "Line", w: 400.0, h: 48.0, kind: LayerKind::Shape(ShapeKind::Line) },
        InsertCase { label: "Triangle", name: "Triangle", w: 320.0, h: 280.0, kind: LayerKind::Shape(ShapeKind::Triangle) },
        InsertCase { label: "Star", name: "Star", w: 320.0, h: 300.0, kind: LayerKind::Shape(ShapeKind::Star) },
        InsertCase { label: "Arrow", name: "Arrow", w: 400.0, h: 200.0, kind: LayerKind::Shape(ShapeKind::Arrow) },
        InsertCase { label: "Pentagon", name: "Pentagon", w: 320.0, h: 300.0, kind: LayerKind::Shape(ShapeKind::Pentagon) },
        InsertCase { label: "Hexagon", name: "Hexagon", w: 320.0, h: 280.0, kind: LayerKind::Shape(ShapeKind::Hexagon) },
        InsertCase { label: "Diamond", name: "Diamond", w: 280.0, h: 280.0, kind: LayerKind::Shape(ShapeKind::Diamond) },
        InsertCase { label: "Cross", name: "Cross", w: 300.0, h: 300.0, kind: LayerKind::Shape(ShapeKind::Cross) },
        InsertCase { label: "Heart", name: "Heart", w: 300.0, h: 280.0, kind: LayerKind::Shape(ShapeKind::Heart) },
    ];

    /// La tabla de casos es un espejo exacto de la cuadrícula: ni etiquetas
    /// del panel sin caso, ni casos huérfanos.
    #[test]
    fn insert_cases_match_the_panel_tiles() {
        assert_eq!(INSERT_CASES.len(), INSERT_ITEMS.len());
        for item in &INSERT_ITEMS {
            assert!(
                INSERT_CASES.iter().any(|c| c.label == item.label),
                "la etiqueta '{}' del panel no tiene caso esperado",
                item.label
            );
        }
        for case in &INSERT_CASES {
            assert!(
                INSERT_ITEMS.iter().any(|i| i.label == case.label),
                "el caso '{}' no corresponde a ninguna etiqueta del panel",
                case.label
            );
        }
    }

    #[test]
    fn insert_item_creates_each_tile_centered_with_expected_content() {
        for case in &INSERT_CASES {
            let mut state = EditorState::new_blank(800.0, 600.0);
            insert_item(&mut state, case.label);
            let page = state.doc.page().expect("un documento en blanco tiene página");
            assert_eq!(page.layers.len(), 1, "{}", case.label);
            let layer = &page.layers[0];
            assert_eq!(layer.name, case.name, "{}", case.label);
            assert_eq!(layer.transform.width, case.w, "{}", case.label);
            assert_eq!(layer.transform.height, case.h, "{}", case.label);
            // Centrada en la página (origen + mitad del tamaño = centro).
            let (cx, cy) = layer.transform.center();
            assert!(
                (cx - page.width / 2.0).abs() < 1e-9,
                "{}: centrado en x",
                case.label
            );
            assert!(
                (cy - page.height / 2.0).abs() < 1e-9,
                "{}: centrado en y",
                case.label
            );
            match (case.kind, &layer.content) {
                (LayerKind::Text, LayerContent::Text(_)) => {}
                (LayerKind::Shape(k), LayerContent::Shape(s)) => {
                    assert_eq!(s.kind, k, "{}", case.label)
                }
                (expected, got) => panic!(
                    "{}: esperaba {:?}, la capa tiene {:?}",
                    case.label, expected, got
                ),
            }
            // La capa nueva queda seleccionada (la inserta y selecciona).
            assert_eq!(
                state.selection,
                Selection::single(layer.id),
                "{}",
                case.label
            );
        }
    }

    /// Cualquier etiqueta desconocida cae al caso por defecto: una flecha.
    #[test]
    fn insert_item_unknown_label_falls_back_to_an_arrow() {
        let mut state = EditorState::new_blank(800.0, 600.0);
        insert_item(&mut state, "NoSuchItem");
        let page = state.doc.page().expect("un documento en blanco tiene página");
        assert_eq!(page.layers.len(), 1);
        let layer = &page.layers[0];
        assert_eq!(layer.name, "Arrow");
        assert_eq!(layer.transform.width, 400.0);
        assert_eq!(layer.transform.height, 200.0);
        let LayerContent::Shape(s) = &layer.content else {
            panic!("el caso por defecto debe crear una forma, no {:?}", layer.content);
        };
        assert_eq!(s.kind, ShapeKind::Arrow);
        let (cx, cy) = layer.transform.center();
        assert!((cx - page.width / 2.0).abs() < 1e-9);
        assert!((cy - page.height / 2.0).abs() < 1e-9);
        assert_eq!(state.selection, Selection::single(layer.id));
    }

    /// `insert_item` es deshacible: cada inserción apila un paso en el
    /// historial, un `undo()` devuelve la página a su estado anterior (sin
    /// capas) y el `redo()` restaura la capa. Se comprueba para cada etiqueta
    /// del panel y para el caso por defecto (etiqueta desconocida).
    #[test]
    fn insert_item_is_undoable_and_redoable_for_every_tile() {
        for label in INSERT_ITEMS.iter().map(|i| i.label).chain(["NoSuchItem"]) {
            let mut state = EditorState::new_blank(800.0, 600.0);
            insert_item(&mut state, label);
            let page = state.doc.page().expect("un documento en blanco tiene página");
            assert_eq!(page.layers.len(), 1, "{label}: debe insertar una capa");
            let inserted = page.layers[0].id;

            state.undo();
            let page = state.doc.page().expect("un documento en blanco tiene página");
            assert!(
                page.layers.is_empty(),
                "{label}: deshacer debe dejar la página sin capas"
            );
            assert!(
                !state.selection.contains(inserted),
                "{label}: la selección debe olvidar la capa deshecha"
            );

            state.redo();
            let page = state.doc.page().expect("un documento en blanco tiene página");
            assert_eq!(
                page.layers.len(),
                1,
                "{label}: rehacer debe restaurar la capa insertada"
            );
        }
    }

    /// Dos inserciones del panel se apilan en el orden de inserción (índice
    /// 0 = abajo, arriba del todo = última) y el deshacer las quita en orden
    /// inverso, restaurando el rehacer el apilado original.
    #[test]
    fn insert_item_stacks_layers_in_order_and_undo_removes_them_in_reverse() {
        let mut state = EditorState::new_blank(800.0, 600.0);
        insert_item(&mut state, "Rect"); // primera, abajo del todo
        insert_item(&mut state, "Heart"); // segunda, encima

        let page = state.doc.page().expect("un documento en blanco tiene página");
        assert_eq!(page.layers.len(), 2);
        assert_eq!(page.layers[0].name, "Rectangle", "la primera inserción queda abajo");
        assert_eq!(page.layers[1].name, "Heart", "la segunda inserción queda encima");
        // La última insertada es la que manda: seleccionada y en el tope.
        assert_eq!(state.selection, Selection::single(page.layers[1].id));

        state.undo();
        let page = state.doc.page().expect("un documento en blanco tiene página");
        assert_eq!(page.layers.len(), 1);
        assert_eq!(
            page.layers[0].name, "Rectangle",
            "el primer deshacer quita la de arriba (Heart), no la de abajo"
        );

        state.undo();
        let page = state.doc.page().expect("un documento en blanco tiene página");
        assert!(
            page.layers.is_empty(),
            "el segundo deshacer deja la página sin capas"
        );

        // El rehacer las restaura en el mismo orden de apilado original.
        state.redo();
        state.redo();
        let page = state.doc.page().expect("un documento en blanco tiene página");
        assert_eq!(page.layers.len(), 2);
        assert_eq!(page.layers[0].name, "Rectangle");
        assert_eq!(page.layers[1].name, "Heart");
    }

    /// Un documento con tres capas raíz (`Rect`, `Ellipse`, `Heart`, de
    /// abajo arriba) y sus ids, listo para reordenar.
    fn state_with_three_layers() -> (EditorState, [LayerId; 3]) {
        let mut state = EditorState::new_blank(800.0, 600.0);
        let mut ids = Vec::new();
        for label in ["Rect", "Ellipse", "Heart"] {
            insert_item(&mut state, label);
            let page = state.doc.page().expect("un documento en blanco tiene página");
            ids.push(page.layers.last().expect("insert_item añade una capa").id);
        }
        (state, [ids[0], ids[1], ids[2]])
    }

    /// `push_rows` recorre TODAS las capas del documento (las de los grupos
    /// incluidas) y `row_ui` las pinta sin romper: tantas filas como capas,
    /// en orden de panel (grupo primero, hijos después, raíz más alta la
    /// última) y con la sangría por profundidad.
    #[test]
    fn row_ui_renders_every_layer_of_the_document() {
        let mut state = EditorState::new_blank(800.0, 600.0);
        insert_item(&mut state, "Rect");
        insert_item(&mut state, "Ellipse");
        insert_item(&mut state, "Heart");
        // Grupo con Ellipse y Heart: [Rect, Group(Ellipse, Heart)].
        let page = state.doc.page().expect("un documento en blanco tiene página");
        let ids: Vec<LayerId> = page.layers.iter().map(|l| l.id).collect();
        state.selection = Selection::single(ids[1]);
        state.selection.toggle(ids[2]);
        group_selection(&mut state);
        let page = state.doc.page().expect("un documento en blanco tiene página");
        let group = page.layers[1].id;
        assert!(page.is_group(group), "Ellipse y Heart quedan en un grupo");

        // Una fila por capa, en orden de panel (de arriba a abajo).
        let mut rows = Vec::new();
        push_rows(page, None, 0, &mut rows);
        assert_eq!(rows.len(), 4, "todas las capas tienen fila");
        assert_eq!(rows[0].id, group, "el grupo va primero (arriba en el panel)");
        assert_eq!(rows[0].depth, 0);
        assert!(rows[0].is_group);
        assert_eq!(rows[1].id, ids[2], "Heart, hija del grupo, después");
        assert_eq!(rows[1].depth, 1);
        assert_eq!(rows[2].id, ids[1], "Ellipse, hija del grupo, después");
        assert_eq!(rows[2].depth, 1);
        assert_eq!(rows[3].id, ids[0], "Rect, raíz, la última (abajo)");
        assert_eq!(rows[3].depth, 0);

        // Pintar cada fila en un frame headless: sin gesto de arrastre no
        // debe devolver ninguna soltada.
        let ctx = egui::Context::default();
        ctx.set_fonts(egui::FontDefinitions::empty());
        let mut drops = Vec::new();
        let _ = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 600.0),
                )),
                ..Default::default()
            },
            |ui| {
                for row in &rows {
                    if let Some(drop) = row_ui(&mut state, ui, row) {
                        drops.push(drop);
                    }
                }
            },
        );
        assert!(drops.is_empty(), "sin arrastre no debe haber soltada");
    }

    /// Arrastrar una capa «encima de» otra la sitúa justo encima del objetivo
    /// en la pila (más arriba en el panel).
    #[test]
    fn apply_reorder_above_places_the_layer_above_its_target() {
        let (mut state, [a, b, c]) = state_with_three_layers();
        // C (arriba del todo) arrastrada «encima de» A (abajo del todo).
        apply_reorder(&mut state, &[c], Drop::Above(a));
        let page = state.doc.page().expect("un documento en blanco tiene página");
        let ids: Vec<LayerId> = page.layers.iter().map(|l| l.id).collect();
        assert_eq!(ids, [a, c, b], "C queda entre A y B, justo encima de A");
    }

    /// Arrastrar una capa «debajo de» otra la sitúa justo debajo del objetivo
    /// en la pila (más abajo en el panel).
    #[test]
    fn apply_reorder_below_places_the_layer_below_its_target() {
        let (mut state, [a, b, c]) = state_with_three_layers();
        // A (abajo del todo) arrastrada «debajo de» C (arriba del todo).
        apply_reorder(&mut state, &[a], Drop::Below(c));
        let page = state.doc.page().expect("un documento en blanco tiene página");
        let ids: Vec<LayerId> = page.layers.iter().map(|l| l.id).collect();
        assert_eq!(ids, [b, a, c], "A queda entre B y C, justo debajo de C");
    }

    /// Arrastrar una capa «dentro de» un grupo la mete como último hijo.
    #[test]
    fn apply_reorder_into_puts_the_layer_inside_the_group() {
        let (mut state, [a, b, c]) = state_with_three_layers();
        // Grupo con A: [Group(A), B, C].
        state.selection = Selection::single(a);
        group_selection(&mut state);
        let page = state.doc.page().expect("un documento en blanco tiene página");
        let group = page.layers[0].id;
        assert!(page.is_group(group));

        // C arrastrada «dentro de» el grupo: queda como último hijo.
        apply_reorder(&mut state, &[c], Drop::Into(group));
        let page = state.doc.page().expect("un documento en blanco tiene página");
        assert_eq!(
            page.children_of(Some(group)),
            [a, c],
            "C entra como hijo del grupo, encima de A"
        );
        assert_eq!(
            page.children_of(None),
            [group, b],
            "B sigue como raíz; el grupo y B son las únicas"
        );
    }

    /// Un arrastre con varias capas seleccionadas (ids sin orden garantizado)
    /// conserva su apilamiento relativo dentro del destino.
    #[test]
    fn apply_reorder_with_several_layers_keeps_their_relative_order() {
        let (mut state, [a, b, c]) = state_with_three_layers();
        // Grupo con A: [Group(A), B, C].
        state.selection = Selection::single(a);
        group_selection(&mut state);
        let page = state.doc.page().expect("un documento en blanco tiene página");
        let group = page.layers[0].id;

        // B y C (pasadas al revés, como llegan de la selección) «dentro de»
        // el grupo: entran ordenadas por pila, no por el orden del payload.
        apply_reorder(&mut state, &[c, b], Drop::Into(group));
        let page = state.doc.page().expect("un documento en blanco tiene página");
        assert_eq!(
            page.children_of(Some(group)),
            [a, b, c],
            "el grupo recibe a B y C en su orden de pila"
        );
    }

    /// El reordenamiento es UN solo paso de deshacer: un undo restaura el
    /// orden original completo.
    #[test]
    fn apply_reorder_is_a_single_undo_step() {
        let (mut state, [a, b, c]) = state_with_three_layers();
        apply_reorder(&mut state, &[c], Drop::Above(a));
        let page = state.doc.page().expect("un documento en blanco tiene página");
        let ids: Vec<LayerId> = page.layers.iter().map(|l| l.id).collect();
        assert_ne!(ids, [a, b, c], "el reorden debe haber cambiado el orden");

        state.undo();
        let page = state.doc.page().expect("un documento en blanco tiene página");
        let ids: Vec<LayerId> = page.layers.iter().map(|l| l.id).collect();
        assert_eq!(ids, [a, b, c], "un solo undo restaura el orden original");
    }

    /// `group_selection` mete las capas seleccionadas en un grupo nuevo que
    /// ocupa el hueco de la más alta, conservando su orden de pila, y deja
    /// el grupo seleccionado.
    #[test]
    fn group_selection_groups_the_selected_layers_in_stack_order() {
        let (mut state, [a, b, c]) = state_with_three_layers();
        state.selection = Selection::single(b);
        state.selection.toggle(c);
        group_selection(&mut state);

        let page = state.doc.page().expect("un documento en blanco tiene página");
        let group = page.layers[1].id; // ocupa el hueco de B y C
        assert!(page.is_group(group));
        assert_eq!(page.children_of(None), [a, group], "A sigue de raíz");
        assert_eq!(
            page.children_of(Some(group)),
            [b, c],
            "los miembros conservan su orden de pila dentro del grupo"
        );
        assert_eq!(
            state.selection,
            Selection::single(group),
            "el grupo queda seleccionado"
        );
    }

    /// `ungroup_selection` disuelve el grupo seleccionado y sus hijos
    /// DIRECTOS vuelven a su hueco en la pila, en el mismo orden.
    #[test]
    fn ungroup_selection_restores_the_children_in_place_and_order() {
        let (mut state, [a, b, c]) = state_with_three_layers();
        state.selection = Selection::single(b);
        state.selection.toggle(c);
        group_selection(&mut state);

        ungroup_selection(&mut state);
        let page = state.doc.page().expect("un documento en blanco tiene página");
        let ids: Vec<LayerId> = page.layers.iter().map(|l| l.id).collect();
        assert_eq!(ids, [a, b, c], "los hijos vuelven a su hueco, en su orden");
        assert_eq!(page.children_of(None), [a, b, c]);
    }

    /// Agrupar es un solo paso de deshacer: un undo disuelve el grupo y
    /// restaura el orden y la selección originales.
    #[test]
    fn group_selection_is_undoable() {
        let (mut state, [a, b, c]) = state_with_three_layers();
        state.selection = Selection::single(b);
        state.selection.toggle(c);
        group_selection(&mut state);
        let page = state.doc.page().expect("un documento en blanco tiene página");
        assert_eq!(page.layers.len(), 4, "el grupo añade una capa");

        state.undo();
        let page = state.doc.page().expect("un documento en blanco tiene página");
        let ids: Vec<LayerId> = page.layers.iter().map(|l| l.id).collect();
        assert_eq!(ids, [a, b, c], "un undo disuelve el grupo y restaura el orden");
        assert!(
            state.selection.is_empty(),
            "la selección olvida el grupo deshecho"
        );
    }

    /// Desagrupar es un solo paso de deshacer: un undo vuelve a crear el
    /// grupo con sus hijos dentro.
    #[test]
    fn ungroup_selection_is_undoable() {
        let (mut state, [_a, b, c]) = state_with_three_layers();
        state.selection = Selection::single(b);
        state.selection.toggle(c);
        group_selection(&mut state);
        let group = state
            .selection
            .primary()
            .expect("agrupar deja el grupo seleccionado");

        ungroup_selection(&mut state);
        let page = state.doc.page().expect("un documento en blanco tiene página");
        assert_eq!(page.layers.len(), 3, "el grupo se disuelve");

        state.undo();
        let page = state.doc.page().expect("un documento en blanco tiene página");
        assert_eq!(page.layers.len(), 4, "un undo restaura el grupo");
        assert!(page.is_group(group));
        assert_eq!(
            page.children_of(Some(group)),
            [b, c],
            "el grupo recupera a sus hijos en orden"
        );
    }

    /// Con varios grupos seleccionados, `ungroup_selection` los disuelve
    /// todos y TODO el conjunto es un único paso de deshacer.
    #[test]
    fn ungroup_selection_dissolves_every_selected_group_in_one_undo_step() {
        let mut state = EditorState::new_blank(800.0, 600.0);
        let mut ids = Vec::new();
        for label in ["Rect", "Ellipse", "Heart", "Star"] {
            insert_item(&mut state, label);
            let page = state.doc.page().expect("un documento en blanco tiene página");
            ids.push(page.layers.last().expect("insert_item añade una capa").id);
        }
        // Grupo con las dos de abajo y otro con las dos de arriba.
        state.selection = Selection::single(ids[0]);
        state.selection.toggle(ids[1]);
        group_selection(&mut state);
        state.selection = Selection::single(ids[2]);
        state.selection.toggle(ids[3]);
        group_selection(&mut state);
        let page = state.doc.page().expect("un documento en blanco tiene página");
        let groups: Vec<LayerId> = page
            .layers
            .iter()
            .filter(|l| page.is_group(l.id))
            .map(|l| l.id)
            .collect();
        assert_eq!(groups.len(), 2);

        state.selection = Selection::single(groups[0]);
        state.selection.toggle(groups[1]);
        ungroup_selection(&mut state);
        let page = state.doc.page().expect("un documento en blanco tiene página");
        assert_eq!(
            page.children_of(None),
            ids,
            "ambos grupos disueltos, las cuatro capas en el orden original"
        );

        state.undo();
        let page = state.doc.page().expect("un documento en blanco tiene página");
        let groups_now: Vec<LayerId> = page
            .layers
            .iter()
            .filter(|l| page.is_group(l.id))
            .map(|l| l.id)
            .collect();
        assert_eq!(
            groups_now, groups,
            "un solo undo restaura los dos grupos"
        );
    }

    /// Un frame headless de egui que pinta la fila `row` con los `events`
    /// dados. El mismo `ctx` sirve para varios frames (el foco persiste).
    fn run_row_frame(
        ctx: &egui::Context,
        state: &mut EditorState,
        row: &Row,
        events: Vec<egui::Event>,
    ) {
        let _ = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 600.0),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                let _ = row_ui(state, ui, row);
            },
        );
    }

    fn key_press(key: egui::Key) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }
    }

    /// Un clic real sobre la fila, en tres frames (mover el puntero, pulsar
    /// y soltar): egui decide hover/clic con lo registrado en el frame
    /// anterior, igual que en la app.
    fn click_at(ctx: &egui::Context, state: &mut EditorState, row: &Row, pos: egui::Pos2) {
        run_row_frame(ctx, state, row, vec![egui::Event::PointerMoved(pos)]);
        run_row_frame(
            ctx,
            state,
            row,
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        run_row_frame(
            ctx,
            state,
            row,
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
    }

    /// Renombrado in situ: editar el nombre y confirmar con Enter emite un
    /// `Rename` deshacible que cambia la capa y cierra la edición.
    #[test]
    fn renaming_commits_on_enter_with_an_undoable_rename() {
        let (mut state, [a, _b, _c]) = state_with_three_layers();
        let original = state.doc.layer(a).expect("la capa existe").name.clone();
        state.rename_edit = Some((a, "Renamed".to_owned(), original.clone()));
        let row = Row {
            id: a,
            depth: 0,
            is_group: false,
            collapsed: false,
        };
        let ctx = egui::Context::default();
        ctx.set_fonts(egui::FontDefinitions::empty());

        // Frame 1: se pinta el TextEdit; se le da el foco para el frame 2.
        run_row_frame(&ctx, &mut state, &row, vec![]);
        let text_id = egui::Id::new(("layer_row", a.raw())).with("rename");
        ctx.memory_mut(|m| m.request_focus(text_id));
        // Frame 2: Enter confirma la edición.
        run_row_frame(&ctx, &mut state, &row, vec![key_press(egui::Key::Enter)]);

        assert!(state.rename_edit.is_none(), "la edición se cierra al confirmar");
        assert_eq!(
            state.doc.layer(a).expect("la capa existe").name,
            "Renamed"
        );
        // El Rename es un paso de deshacer normal.
        state.undo();
        assert_eq!(
            state.doc.layer(a).expect("la capa existe").name,
            original,
            "un undo restaura el nombre original"
        );
    }

    /// Renombrado in situ: Escape cancela sin aplicar nada.
    #[test]
    fn renaming_cancels_with_escape() {
        let (mut state, [a, _b, _c]) = state_with_three_layers();
        let original = state.doc.layer(a).expect("la capa existe").name.clone();
        state.rename_edit = Some((a, "Renamed".to_owned(), original.clone()));
        let row = Row {
            id: a,
            depth: 0,
            is_group: false,
            collapsed: false,
        };
        let ctx = egui::Context::default();
        ctx.set_fonts(egui::FontDefinitions::empty());

        run_row_frame(&ctx, &mut state, &row, vec![key_press(egui::Key::Escape)]);

        assert!(state.rename_edit.is_none(), "Escape cierra la edición");
        assert_eq!(
            state.doc.layer(a).expect("la capa existe").name,
            original,
            "Escape no aplica el nombre editado"
        );
    }

    /// El botón del ojo (primer icono del prefijo: x ∈ [18, 36] en una fila
    /// de raíz sin grupo) alterna la visibilidad, y el cambio es deshacible.
    #[test]
    fn the_eye_button_toggles_visibility_undoably() {
        let (mut state, [a, _b, _c]) = state_with_three_layers();
        let row = Row {
            id: a,
            depth: 0,
            is_group: false,
            collapsed: false,
        };
        assert!(
            state.doc.layer(a).expect("la capa existe").visible,
            "arranca visible"
        );
        let ctx = egui::Context::default();
        ctx.set_fonts(egui::FontDefinitions::empty());

        click_at(&ctx, &mut state, &row, egui::pos2(27.0, 9.0)); // el ojo
        assert!(
            !state.doc.layer(a).expect("la capa existe").visible,
            "el ojo oculta la capa"
        );

        state.undo();
        assert!(
            state.doc.layer(a).expect("la capa existe").visible,
            "un undo restaura la visibilidad"
        );
    }

    /// El botón del candado (segundo icono del prefijo: x ∈ [44, 62]) alterna
    /// el bloqueo, y el cambio es deshacible.
    #[test]
    fn the_lock_button_toggles_locking_undoably() {
        let (mut state, [a, _b, _c]) = state_with_three_layers();
        let row = Row {
            id: a,
            depth: 0,
            is_group: false,
            collapsed: false,
        };
        assert!(
            !state.doc.layer(a).expect("la capa existe").locked,
            "arranca sin bloqueo"
        );
        let ctx = egui::Context::default();
        ctx.set_fonts(egui::FontDefinitions::empty());

        click_at(&ctx, &mut state, &row, egui::pos2(53.0, 9.0)); // el candado
        assert!(
            state.doc.layer(a).expect("la capa existe").locked,
            "el candado bloquea la capa"
        );

        state.undo();
        assert!(
            !state.doc.layer(a).expect("la capa existe").locked,
            "un undo quita el bloqueo"
        );
    }

    /// Un frame headless que pinta la tira de pestañas con los `events`
    /// dados y devuelve el posible cambio de orden (swap por arrastre).
    fn run_tab_frame(
        ctx: &egui::Context,
        active_tab: &mut LeftTab,
        layers_collapsed: &mut bool,
        order: LayersTabOrder,
        collapsed: bool,
        events: Vec<egui::Event>,
    ) -> Option<LayersTabOrder> {
        let mut out = None;
        let _ = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(400.0, 400.0),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                out = vertical_tab_strip_ui(ui, active_tab, layers_collapsed, order, collapsed);
            },
        );
        out
    }

    /// Un clic real sobre la tira en `pos` (mover, pulsar y soltar).
    fn click_tab(
        ctx: &egui::Context,
        active_tab: &mut LeftTab,
        layers_collapsed: &mut bool,
        order: LayersTabOrder,
        collapsed: bool,
        pos: egui::Pos2,
    ) -> Option<LayersTabOrder> {
        let mut out = None;
        for events in [
            vec![egui::Event::PointerMoved(pos)],
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        ] {
            if let Some(o) = run_tab_frame(ctx, active_tab, layers_collapsed, order, collapsed, events) {
                out = Some(o);
            }
        }
        out
    }

    /// Un clic sobre otra pestaña cambia la activa sin tocar el colapso.
    #[test]
    fn a_click_on_another_tab_changes_the_active_tab() {
        let ctx = egui::Context::default();
        ctx.set_fonts(egui::FontDefinitions::empty());
        let mut active = LeftTab::Page;
        let mut collapsed = false;

        // Con orden PageFirst, el segundo tab (y ∈ [80, 144]) es Layers.
        let swapped = click_tab(
            &ctx,
            &mut active,
            &mut collapsed,
            LayersTabOrder::PageFirst,
            false,
            egui::pos2(18.0, 112.0),
        );

        assert_eq!(active, LeftTab::Layers, "el clic activa la pestaña pulsada");
        assert!(!collapsed, "un clic normal no colapsa");
        assert!(swapped.is_none(), "un clic no reordena");
    }

    /// Un clic sobre la pestaña YA activa colapsa el panel.
    #[test]
    fn a_click_on_the_active_tab_collapses_the_panel() {
        let ctx = egui::Context::default();
        ctx.set_fonts(egui::FontDefinitions::empty());
        let mut active = LeftTab::Page;
        let mut collapsed = false;

        // PageFirst: el primer tab (y ∈ [8, 72]) es Page, la activa.
        let swapped = click_tab(
            &ctx,
            &mut active,
            &mut collapsed,
            LayersTabOrder::PageFirst,
            false,
            egui::pos2(18.0, 40.0),
        );

        assert!(collapsed, "clic en la activa colapsa el panel");
        assert_eq!(active, LeftTab::Page, "la activa no cambia");
        assert!(swapped.is_none());
    }

    /// Con el panel colapsado, un clic en cualquier pestaña lo expande y la
    /// activa.
    #[test]
    fn a_click_expands_a_collapsed_panel_and_activates_the_tab() {
        let ctx = egui::Context::default();
        ctx.set_fonts(egui::FontDefinitions::empty());
        let mut active = LeftTab::Page;
        let mut collapsed = true;

        click_tab(
            &ctx,
            &mut active,
            &mut collapsed,
            LayersTabOrder::PageFirst,
            true,
            egui::pos2(18.0, 112.0), // Layers
        );

        assert!(!collapsed, "el clic expande el panel");
        assert_eq!(active, LeftTab::Layers, "y activa la pestaña pulsada");
    }

    /// El orden físico de las pestañas sigue el ajuste: con LayersFirst,
    /// Layers ocupa el primer tab (arriba del todo), no Page.
    #[test]
    fn the_tab_order_follows_the_setting() {
        let ctx = egui::Context::default();
        ctx.set_fonts(egui::FontDefinitions::empty());

        // LayersFirst: el tab superior (y ∈ [8, 72]) es Layers.
        let mut active = LeftTab::Page;
        let mut collapsed = false;
        click_tab(
            &ctx,
            &mut active,
            &mut collapsed,
            LayersTabOrder::LayersFirst,
            false,
            egui::pos2(18.0, 40.0),
        );
        assert_eq!(active, LeftTab::Layers, "Layers va primero con LayersFirst");

        // PageFirst: el tab superior es Page.
        let ctx = egui::Context::default();
        ctx.set_fonts(egui::FontDefinitions::empty());
        let mut active = LeftTab::Layers;
        let mut collapsed = false;
        click_tab(
            &ctx,
            &mut active,
            &mut collapsed,
            LayersTabOrder::PageFirst,
            false,
            egui::pos2(18.0, 40.0),
        );
        assert_eq!(active, LeftTab::Page, "Page va primero con PageFirst");
    }

    /// Arrastrar una pestaña sobre otra (superado el umbral de clic) no
    /// cambia la activa, pero devuelve el orden intercambiado para que el
    /// llamador lo persista en los ajustes.
    #[test]
    fn dragging_a_tab_over_another_returns_the_swapped_order() {
        let ctx = egui::Context::default();
        ctx.set_fonts(egui::FontDefinitions::empty());
        let mut active = LeftTab::Page;
        let mut collapsed = false;
        let order = LayersTabOrder::PageFirst;

        // PageFirst: Page (y ∈ [8, 72]) arrastrada sobre Layers (y ∈ [80, 144]).
        let start = egui::pos2(18.0, 40.0);
        let target = egui::pos2(18.0, 112.0);
        let mut out = None;
        for events in [
            vec![egui::Event::PointerMoved(start)],
            vec![egui::Event::PointerButton {
                pos: start,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
            vec![egui::Event::PointerMoved(target)],
            vec![egui::Event::PointerButton {
                pos: target,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        ] {
            if let Some(o) = run_tab_frame(&ctx, &mut active, &mut collapsed, order, false, events) {
                out = Some(o);
            }
        }

        assert_eq!(
            out,
            Some(LayersTabOrder::LayersFirst),
            "el arrastre devuelve el orden intercambiado"
        );
        assert_eq!(active, LeftTab::Page, "la pestaña activa no cambia con el arrastre");
        assert!(!collapsed, "el arrastre no colapsa");
    }

    /// Un frame headless que pinta la cuadrícula Insert con los `events`
    /// dados, sobre un área de `width` (la mitad del panel, como en la app).
    fn run_insert_frame(
        ctx: &egui::Context,
        state: &mut EditorState,
        width: f32,
        events: Vec<egui::Event>,
    ) {
        let _ = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 600.0),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                insert_tab_ui(state, ui);
            },
        );
    }

    /// Un clic real sobre un tile de Insert (mover, pulsar y soltar).
    fn click_insert_tile(ctx: &egui::Context, state: &mut EditorState, width: f32, pos: egui::Pos2) {
        run_insert_frame(ctx, state, width, vec![egui::Event::PointerMoved(pos)]);
        run_insert_frame(
            ctx,
            state,
            width,
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        run_insert_frame(
            ctx,
            state,
            width,
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
    }

    /// Un clic sobre cada tile de la cuadrícula Insert inserta la capa de la
    /// etiqueta de ESE tile (el clic llama a `insert_item` con su label).
    #[test]
    fn clicking_each_insert_tile_inserts_the_matching_layer() {
        let width = 400.0;
        // El mismo layout que `insert_tab_ui`: dos columnas de tiles.
        let pad = sidebar::PANEL_PAD * 2.0;
        let gap = 8.0;
        let tile_w = ((width - pad - gap) * 0.5).max(1.0);
        let x0 = pad / 2.0;

        for (i, item) in INSERT_ITEMS.iter().enumerate() {
            let expected = INSERT_CASES
                .iter()
                .find(|c| c.label == item.label)
                .expect("toda etiqueta del panel tiene caso esperado");
            let mut state = EditorState::new_blank(800.0, 600.0);
            let ctx = egui::Context::default();
            ctx.set_fonts(egui::FontDefinitions::empty());
            // Centro del tile: fila = i/2, columna = i%2.
            let center = egui::pos2(
                x0 + (i % 2) as f32 * (tile_w + gap) + tile_w / 2.0,
                (i / 2) as f32 * (INSERT_TILE_H + 10.0) + INSERT_TILE_H / 2.0,
            );
            click_insert_tile(&ctx, &mut state, width, center);

            let page = state.doc.page().expect("un documento en blanco tiene página");
            assert_eq!(
                page.layers.len(),
                1,
                "{}: el clic inserta exactamente una capa",
                item.label
            );
            assert_eq!(
                page.layers[0].name,
                expected.name,
                "{}: el clic inserta la capa de la etiqueta del tile",
                item.label
            );
            assert_eq!(page.layers[0].transform.width, expected.w, "{}", item.label);
            assert_eq!(page.layers[0].transform.height, expected.h, "{}", item.label);
        }
    }

    #[test]
    fn ordered_tabs_follows_the_setting() {
        assert_eq!(
            ordered_tabs(LayersTabOrder::PageFirst),
            [LeftTab::Page, LeftTab::Layers, LeftTab::Insert, LeftTab::Images]
        );
        assert_eq!(
            ordered_tabs(LayersTabOrder::LayersFirst),
            [LeftTab::Layers, LeftTab::Page, LeftTab::Insert, LeftTab::Images]
        );
    }

    #[test]
    fn each_tab_appears_exactly_once() {
        let order = ordered_tabs(LayersTabOrder::LayersFirst);
        assert!(order.contains(&LeftTab::Page));
        assert!(order.contains(&LeftTab::Layers));
        assert!(order.contains(&LeftTab::Insert));
        assert!(order.contains(&LeftTab::Images));
    }
}
