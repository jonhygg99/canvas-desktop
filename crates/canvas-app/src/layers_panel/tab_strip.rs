//! La tira vertical de pestañas del panel (Page / Layers / Insert / Images):
//! clic para activar o colapsar, arrastre geométrico para intercambiar el
//! orden Page↔Layers con animación de intercambio. El clic y el arrastre NO
//! usan el drag-and-drop de egui (dos widgets solapados peleándose por el
//! hit-test daban clic muerto o arrastre muerto): aquí todo se decide por
//! geometría directa del puntero, que no puede fallar. La fase de pintado
//! completa (pestañas, hover, fantasma del arrastre) vive en `tab_draw`.
//!
//! `vertical_tab_strip_ui` es solo orquestación: el pase se divide en fases
//! (`tab_layout`, `tick_swap_anim`, `paint_tabs`, `handle_release`), cada
//! una por debajo del límite de 80 líneas.

use eframe::egui;

use crate::editor::state::LeftTab;
use crate::settings::LayersTabOrder;

use super::tab_draw::{paint_tabs, PaintPass, TabStyle};

/// Ancho de la tira de pestañas.
pub(super) const STRIP_WIDTH: f32 = 36.0;
/// Altura de cada pestaña.
const TAB_H: f32 = 64.0;
/// Separación entre pestañas (y extra delante de Insert, como separador).
pub(super) const TAB_GAP: f32 = 8.0;
/// Margen superior de la tira, fijo y no centrado: así las pestañas no
/// saltan de posición cuando el panel pasa de colapsado a expandido (y
/// viceversa) — la animación de `show_switched` puede cambiar la altura
/// disponible a medio frame y el centrado hacía que los iconos bailaran.
const TOP_MARGIN: f32 = 8.0;
/// Duración del deslizamiento del intercambio de pestañas.
const SWAP_ANIM_SECS: f64 = 0.2;

/// Estado de un arrastre manual de pestaña, persistido entre frames en el
/// `data` de egui (la función de UI en sí no guarda estado).
#[derive(Clone)]
pub(super) struct TabDrag {
    pub(super) tab: LeftTab,
    /// Dónde se pulsó, dentro del rect de `tab`.
    origin: egui::Pos2,
    /// `true` en cuanto el puntero supera el umbral de clic: es un arrastre.
    pub(super) moved: bool,
}

/// Estado de la animación de intercambio: en la soltada el orden lógico
/// cambia al instante y la pestaña arrastrada VUELA desde el punto de
/// soltura hasta su ranura nueva, mientras la otra se desliza a la suya
/// (ease-out cúbico, sin la interpolación de egui). La animación pide
/// repintado cada frame y arranca con el slide COMPLETO (la posición
/// anterior) en su primer frame visible, para que no haya ningún salto.
#[derive(Clone)]
pub(super) struct TabSwapAnim {
    /// Reloj de egui (`InputState::time`) del instante de la soltada.
    start: f64,
    /// `false` hasta el primer frame que pinte la animación: ahí se reancla
    /// `start` para arrancar desde la posición anterior sin discontinuidad.
    started: bool,
    /// Pestaña que se estaba arrastrando (la que vuela desde el cursor).
    pub(super) dragged: LeftTab,
    /// Punto (coordenadas de pantalla) donde se soltó: origen del vuelo.
    pub(super) release_pos: egui::Pos2,
}

/// Las cuatro pestañas: Page y Layers reordenables según los ajustes, e
/// Insert e Images fijas detrás con un separador delante de Insert.
pub(super) fn ordered_tabs(order: LayersTabOrder) -> [LeftTab; 4] {
    let (first, second) = match order {
        LayersTabOrder::PageFirst => (LeftTab::Page, LeftTab::Layers),
        LayersTabOrder::LayersFirst => (LeftTab::Layers, LeftTab::Page),
    };
    [first, second, LeftTab::Insert, LeftTab::Images]
}

/// Rects de las pestañas, en el orden de los ajustes (con el hueco extra
/// como separador delante de Insert).
fn tab_layout(strip_rect: egui::Rect, order: LayersTabOrder) -> Vec<(LeftTab, egui::Rect)> {
    let mut tab_rects: Vec<(LeftTab, egui::Rect)> = Vec::with_capacity(4);
    let mut y = strip_rect.top() + TOP_MARGIN;
    for tab in ordered_tabs(order).iter() {
        // Separador antes de Insert
        if *tab == LeftTab::Insert {
            y += TAB_GAP; // espacio extra como separador visual
        }
        let rect = egui::Rect::from_min_size(
            egui::pos2(strip_rect.left(), y),
            egui::vec2(STRIP_WIDTH, TAB_H),
        );
        tab_rects.push((*tab, rect));
        y += TAB_H + TAB_GAP;
    }
    tab_rects
}

/// Un frame de la animación de intercambio: el orden lógico cambió justo en
/// la soltada; aquí se calcula cuánto se alejan visualmente las pestañas de
/// su posición definitiva. Primer frame: se reancla el reloj y el slide
/// queda completo (la posición ANTERIOR) — sin salto de fotograma. Luego
/// decae a 0 con ease-out cúbico. Mientras esté activa, se pide repintado
/// cada frame: sin esto, al no haber input el glide no avanzaba y las
/// pestañas aparecían intercambiadas de golpe en el siguiente evento.
/// Devuelve `(slide, snapshot)`; al terminar, `anim` queda en `None`.
fn tick_swap_anim(
    anim: &mut Option<TabSwapAnim>,
    now: f64,
    ctx: &egui::Context,
) -> (f32, Option<TabSwapAnim>) {
    let Some(mut a) = anim.take() else {
        return (0.0, None);
    };
    if !a.started {
        a.start = now;
        a.started = true;
    }
    let t = ((now - a.start) / SWAP_ANIM_SECS).clamp(0.0, 1.0) as f32;
    if t >= 1.0 {
        return (0.0, None);
    }
    let ease = 1.0 - (1.0 - t).powi(3);
    let slide = (TAB_H + TAB_GAP) * (1.0 - ease);
    ctx.request_repaint();
    // Mantener la animación viva para el siguiente frame: `swap` consume una
    // copia y `anim` se vuelve a insertar.
    let swap = Some(a.clone());
    *anim = Some(a);
    (slide, swap)
}

/// Desvanecimiento del icono mientras las pestañas se cruzan: cuando
/// `slide` llega a la mitad del recorrido, las dos pestañas están
/// exactamente sobre el mismo sitio — el icono baja a ~35% de opacidad
/// con una rampa suave (smoothstep) y recupera el 100% a ambos lados.
fn icon_fade_factor(slide: f32, tab_count: usize, tab_h: f32) -> f32 {
    if slide > 0.0 && tab_count == 2 {
        let m = (tab_h + TAB_GAP) * 0.5;
        let d = ((slide - m).abs() / m).clamp(0.0, 1.0);
        let d = d * d * (3.0 - 2.0 * d);
        0.35 + 0.65 * d
    } else {
        1.0
    }
}

/// Arranque: el frame en el que el botón primario baja dentro de una
/// pestaña (aún no sabemos si será un clic o un arrastre). Se ignora
/// mientras la animación de intercambio está en curso: las pestañas
/// están desplazadas visualmente y sus rectos lógicos no corresponden
/// con lo que el usuario ve, así que un pulsado ahí sería ambiguo.
/// `dead_press` es un press cancelado por Escape: no debe reabrir gesto
/// mientras el botón siga abajo (ver el comentario en el llamador).
fn begin_drag(
    ui: &egui::Ui,
    tab_rects: &[(LeftTab, egui::Rect)],
    drag: &mut Option<TabDrag>,
    dead_press: Option<egui::Pos2>,
    slide: f32,
) {
    if drag.is_some() || slide != 0.0 {
        return;
    }
    if let Some(origin) = ui.input(|i| i.pointer.press_origin()) {
        if Some(origin) == dead_press {
            return;
        }
        if let Some(tab) = tab_rects
            .iter()
            .find(|(_, r)| r.contains(origin))
            .map(|(t, _)| *t)
        {
            *drag = Some(TabDrag {
                tab,
                origin,
                moved: false,
            });
        }
    }
}

/// Umbral: superado el radio de un clic, es un arrastre.
fn track_drag_threshold(ui: &egui::Ui, drag: &mut Option<TabDrag>) {
    if let Some(d) = drag.as_mut() {
        if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
            if d.origin.distance(pos) > 6.0 {
                d.moved = true;
            }
        }
    }
}

/// Todo lo que la soltada necesita decidir: agrupa los ocho valores que
/// serían parámetros sueltos (convención del repo: struct en vez de
/// `#[allow(too_many_arguments)]`).
struct ReleaseContext<'a> {
    drag: &'a mut Option<TabDrag>,
    anim: &'a mut Option<TabSwapAnim>,
    tab_rects: &'a [(LeftTab, egui::Rect)],
    order: LayersTabOrder,
    active_tab: &'a mut LeftTab,
    layers_collapsed: &'a mut bool,
    collapsed: bool,
    now: f64,
}

/// Soltada: sobre otra pestaña y tras haber arrastrado → intercambiar (con
/// animación); sin haber arrastrado → clic, activar la pestaña pulsada.
fn handle_release(ui: &egui::Ui, rc: ReleaseContext) -> Option<LayersTabOrder> {
    if !ui.input(|i| i.pointer.primary_released()) {
        return None;
    }
    let d = rc.drag.take()?;
    if !d.moved {
        if rc.collapsed {
            *rc.active_tab = d.tab;
            *rc.layers_collapsed = false;
        } else if d.tab == *rc.active_tab {
            // Clic en la pestaña ya activa: colapsar en vez de no hacer nada.
            *rc.layers_collapsed = true;
        } else {
            *rc.active_tab = d.tab;
        }
        return None;
    }
    let pos = ui.input(|i| i.pointer.interact_pos())?;
    let target = rc
        .tab_rects
        .iter()
        .find(|(_, r)| r.contains(pos))
        .map(|(t, _)| *t)?;
    if target == d.tab {
        return None;
    }
    // Ya con el nuevo orden, la próxima vez que se pinte la tira la
    // arrastrada saldrá del cursor y la otra del otro lado; `started: false`
    // hace que el primer fotograma reancle el reloj SIN saltarse la posición
    // inicial, y `request_repaint` mantiene el glide en marcha aunque no
    // haya input.
    *rc.anim = Some(TabSwapAnim {
        start: rc.now,
        started: false,
        dragged: d.tab,
        // El fantasma se dibuja 10 px por encima del puntero: el vuelo sale
        // exactamente de ahí.
        release_pos: pos + egui::vec2(0.0, -10.0),
    });
    Some(rc.order.swapped())
}

/// La tira vertical de pestañas: orquesta el pase completo del frame.
/// `pub(crate)`: también la llama la vista de editor colapsada
/// (`app/views/editor/panels.rs`).
pub(crate) fn vertical_tab_strip_ui(
    ui: &mut egui::Ui,
    active_tab: &mut LeftTab,
    layers_collapsed: &mut bool,
    order: LayersTabOrder,
    collapsed: bool,
) -> Option<LayersTabOrder> {
    let style = TabStyle::from_ui(ui);
    let available_h = ui.available_height();
    let (strip_rect, _) =
        ui.allocate_exact_size(egui::vec2(STRIP_WIDTH, available_h), egui::Sense::hover());

    // Estado del arrastre a través de los frames. El clic y el arrastre NO
    // usan el drag-and-drop de egui (ver el doc del módulo): aquí todo se
    // decide por geometría directa del puntero, que no puede fallar.
    let drag_salt = egui::Id::new("left_tabs_drag");
    let mut drag: Option<TabDrag> = ui.data_mut(|d| d.get_temp(drag_salt).unwrap_or(None));
    let anim_salt = egui::Id::new("left_tabs_swap_anim");
    let mut anim: Option<TabSwapAnim> = ui.data_mut(|d| d.get_temp(anim_salt).unwrap_or(None));
    // El press «muerto» por Escape: mientras el botón siga abajo, ese press
    // no debe resucitar el gesto. Sin esto, al soltar tras Escape el frame
    // de la soltada recrea el drag desde `press_origin`, el umbral vuelve a
    // dispararse y el intercambio ocurre igualmente — justo lo que Escape
    // prometía cancelar.
    let dead_salt = egui::Id::new("left_tabs_dead_press");
    let mut dead_press: Option<egui::Pos2> = ui.data_mut(|d| d.get_temp(dead_salt).unwrap_or(None));
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        drag = None;
        dead_press = ui.input(|i| i.pointer.press_origin());
    }

    let tab_rects = tab_layout(strip_rect, order);
    let now: f64 = ui.input(|i| i.time);
    let (slide, swap) = tick_swap_anim(&mut anim, now, ui.ctx());
    let icon_fade = icon_fade_factor(slide, tab_rects.len(), TAB_H);
    begin_drag(ui, &tab_rects, &mut drag, dead_press, slide);
    track_drag_threshold(ui, &mut drag);
    let hover_pos = ui.input(|i| i.pointer.hover_pos());

    paint_tabs(
        ui,
        PaintPass {
            tab_rects: &tab_rects,
            active_tab: *active_tab,
            drag: &drag,
            swap: &swap,
            slide,
            style: &style,
            hover_pos,
            icon_fade,
            tab_h: TAB_H,
        },
    );

    let new_order = handle_release(
        ui,
        ReleaseContext {
            drag: &mut drag,
            anim: &mut anim,
            tab_rects: &tab_rects,
            order,
            active_tab,
            layers_collapsed,
            collapsed,
            now,
        },
    );

    // Red de seguridad: si el puntero ya no está pulsando nada, olvidar el
    // gesto y su marca de press muerto (por ejemplo si la pulsación se
    // perdió fuera de la ventana).
    if !ui.input(|i| i.pointer.any_down()) {
        drag = None;
        dead_press = None;
    }
    ui.data_mut(|d| d.insert_temp(drag_salt, drag));
    ui.data_mut(|d| d.insert_temp(anim_salt, anim));
    ui.data_mut(|d| d.insert_temp(dead_salt, dead_press));

    new_order
}
