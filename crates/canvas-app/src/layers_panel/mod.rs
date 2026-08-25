//! Panel lateral izquierdo del editor: pestañas Page y Layers en
//! disposición vertical solo con icono (el nombre es la tooltip y el
//! título del menú que hay debajo), barra de iconos en estado
//! colapsado. Las pestañas son arrastrables entre sí
//! para reordenarlas y el orden queda persistido en los ajustes.

use canvas_core::{LayerContent, LayerId, Page};
use eframe::egui;

use crate::app_icons::{draw_layers_icon, draw_page_icon, draw_sparkle_icon};
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
) -> Option<LayersTabOrder> {
    sidebar::compact(ui);
    let mut new_order = None;
    ui.horizontal(|ui| {
        new_order = vertical_tab_strip_ui(ui, &mut state.active_left_tab, layers_collapsed, order, false);
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
            };
            sidebar::title(ui, tab_name);
            ui.add_space(6.0);
            match state.active_left_tab {
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
                LeftTab::Insert => {
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("T Text").clicked() {
                            state.insert_layer_centered(
                                "Text",
                                500.0,
                                120.0,
                                LayerContent::Text(canvas_core::TextContent::default()),
                            );
                        }
                        if ui.small_button("R").on_hover_text("Rectangle").clicked() {
                            state.insert_layer_centered(
                                "Rectangle",
                                320.0,
                                220.0,
                                LayerContent::Shape(canvas_core::ShapeContent::default()),
                            );
                        }
                        if ui.small_button("O").on_hover_text("Ellipse").clicked() {
                            state.insert_layer_centered(
                                "Ellipse",
                                280.0,
                                280.0,
                                LayerContent::Shape(canvas_core::ShapeContent {
                                    kind: canvas_core::ShapeKind::Ellipse,
                                    ..Default::default()
                                }),
                            );
                        }
                        if ui.small_button("L").on_hover_text("Line").clicked() {
                            state.insert_layer_centered(
                                "Line",
                                400.0,
                                24.0,
                                LayerContent::Shape(canvas_core::ShapeContent {
                                    kind: canvas_core::ShapeKind::Line,
                                    stroke: [30, 30, 30, 255],
                                    stroke_width: 6.0,
                                    ..Default::default()
                                }),
                            );
                        }
                    });
                }
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

/// Las tres pestañas: Page y Layers reordenables según los ajustes,
/// Insert siempre al final con un separador.
fn ordered_tabs(order: LayersTabOrder) -> [LeftTab; 3] {
    let (first, second) = match order {
        LayersTabOrder::PageFirst => (LeftTab::Page, LeftTab::Layers),
        LayersTabOrder::LayersFirst => (LeftTab::Layers, LeftTab::Page),
    };
    [first, second, LeftTab::Insert]
}

/// Nombre de la pestaña (tooltip del icono).
fn tab_tip(tab: LeftTab) -> &'static str {
    match tab {
        LeftTab::Page => "Page settings",
        LeftTab::Layers => "Layers",
        LeftTab::Insert => "Insert",
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
    let tab_h = 64.0;
    let icon_size = 20.0;

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
    let mut tab_rects: Vec<(LeftTab, egui::Rect)> = Vec::with_capacity(3);
    let mut y = strip_rect.top() + top_margin;
    for (i, tab) in ordered_tabs(order).iter().enumerate() {
        // Separador antes de Insert
        if *tab == LeftTab::Insert {
            y += TAB_GAP;  // espacio extra como separador visual
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
            strong,
            active_c,
            inactive_c,
            hover_fill,
            tab_icon(*tab),
            icon_size,
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
                ui.painter()
                    .rect_filled(ghost, 6.0, egui::Color32::from_rgba_unmultiplied(0, 122, 255, 40));
                draw_vertical_tab(
                    ui.painter(),
                    ghost,
                    false,
                    false,
                    strong,
                    active_c,
                    active_c,
                    hover_fill,
                    tab_icon(d.tab),
                    icon_size,
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

/// `icon_fade` (1.0 = opaco) atenúa solo el color del icono, no el fondo:
/// durante el cruce del intercambio, cuanto más cerca están las dos
/// pestañas de solaparse, más transparente se vuelve su icono.
fn draw_vertical_tab(
    painter: &egui::Painter,
    rect: egui::Rect,
    is_active: bool,
    hovered: bool,
    strong: egui::Color32,
    active_c: egui::Color32,
    inactive_c: egui::Color32,
    hover_fill: egui::Color32,
    draw_icon: fn(&egui::Painter, egui::Rect, egui::Color32),
    icon_size: f32,
    icon_fade: f32,
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
    } else if hovered {
        // Fondo sutil al hover: solo-icono, el área debe leerse clicable.
        painter.rect_filled(rect, 4.0, hover_fill);
    }

    let color = if is_active {
        strong
    } else if hovered {
        active_c
    } else {
        inactive_c
    };
    let color = color.gamma_multiply(icon_fade);

    // Icono centrado en la pestaña
    let icon_rect = egui::Rect::from_center_size(rect.center(), egui::vec2(icon_size, icon_size));
    draw_icon(painter, icon_rect, color);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::LayersTabOrder;

    #[test]
    fn ordered_tabs_follows_the_setting() {
        assert_eq!(
            ordered_tabs(LayersTabOrder::PageFirst),
            [LeftTab::Page, LeftTab::Layers, LeftTab::Insert]
        );
        assert_eq!(
            ordered_tabs(LayersTabOrder::LayersFirst),
            [LeftTab::Layers, LeftTab::Page, LeftTab::Insert]
        );
    }

    #[test]
    fn each_tab_appears_exactly_once() {
        let order = ordered_tabs(LayersTabOrder::LayersFirst);
        assert!(order.contains(&LeftTab::Page));
        assert!(order.contains(&LeftTab::Layers));
        assert!(order.contains(&LeftTab::Insert));
    }
}

