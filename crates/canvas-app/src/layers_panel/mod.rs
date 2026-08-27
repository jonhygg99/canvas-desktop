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
    use canvas_core::{Selection, ShapeKind};
    use crate::settings::LayersTabOrder;

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
