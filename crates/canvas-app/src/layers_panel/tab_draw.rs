//! Pintado de UNA pestaña de la tira vertical del panel (fondo, indicador
//! activo, icono) y sus accesorios (tooltip, id de hover, icono por
//! pestaña) y el pase completo de la tira (pestañas, hover, destino del
//! arrastre y fantasma). Lo que varía por pestaña se pasa por parámetro; el
//! estilo fijo del pase viaja agrupado en `TabStyle` y los nueve valores del
//! pase en `PaintPass`. La máquina de estados del clic y del arrastre vive
//! en `tab_strip`.

use eframe::egui;

use crate::app_icons::{draw_images_icon, draw_layers_icon, draw_page_icon, draw_sparkle_icon};
use crate::editor::state::LeftTab;

use super::tab_strip::{TabDrag, TabSwapAnim, STRIP_WIDTH, TAB_GAP};

/// Estilo fijo de las pestañas verticales de la tira del panel: idéntico
/// para todas las llamadas de un mismo pase. Lo que varía por llamada
/// (rect, estado, icono, fundido) queda fuera. Agrupado para reducir la
/// firma de `draw_vertical_tab` de 11 a 7 parámetros.
#[derive(Clone, Copy)]
pub(super) struct TabStyle {
    pub(super) strong: egui::Color32,
    pub(super) active_c: egui::Color32,
    pub(super) inactive_c: egui::Color32,
    pub(super) hover_fill: egui::Color32,
    pub(super) icon_size: f32,
}

impl TabStyle {
    /// Colores del pase, tomados de los visuals de egui del frame en curso.
    pub(super) fn from_ui(ui: &egui::Ui) -> Self {
        Self {
            strong: ui.visuals().strong_text_color(),
            active_c: ui.visuals().widgets.active.text_color(),
            inactive_c: ui.visuals().widgets.inactive.text_color(),
            hover_fill: ui.visuals().widgets.hovered.weak_bg_fill,
            icon_size: 20.0,
        }
    }
}

/// `icon_fade` (1.0 = opaco) atenúa solo el color del icono, no el fondo:
/// durante el cruce del intercambio, cuanto más cerca están las dos
/// pestañas de solaparse, más transparente se vuelve su icono.
pub(super) fn draw_vertical_tab(
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

/// Nombre de la pestaña (tooltip del icono).
pub(super) fn tab_tip(tab: LeftTab) -> &'static str {
    match tab {
        LeftTab::Page => "Page settings",
        LeftTab::Layers => "Layers",
        LeftTab::Insert => "Insert",
        LeftTab::Images => "Images (Unsplash)",
    }
}

/// Id del widget hover de una pestaña: solo para tooltips y hover; el clic y
/// el arrastre los gestiona la máquina de estados manual de `tab_strip`.
pub(super) fn tab_hover_id(tab: LeftTab) -> egui::Id {
    egui::Id::new(("left_tab", tab)).with("hover")
}

pub(super) fn tab_icon(tab: LeftTab) -> fn(&egui::Painter, egui::Rect, egui::Color32) {
    match tab {
        LeftTab::Page => draw_page_icon,
        LeftTab::Layers => draw_layers_icon,
        LeftTab::Insert => draw_sparkle_icon,
        LeftTab::Images => draw_images_icon,
    }
}

/// Lo que el pase de pintado necesita del frame en curso: agrupa los nueve
/// valores que serían parámetros sueltos (convención del repo: struct en
/// vez de `#[allow(too_many_arguments)]`).
pub(super) struct PaintPass<'a> {
    pub(super) tab_rects: &'a [(LeftTab, egui::Rect)],
    pub(super) active_tab: LeftTab,
    pub(super) drag: &'a Option<TabDrag>,
    pub(super) swap: &'a Option<TabSwapAnim>,
    pub(super) slide: f32,
    pub(super) style: &'a TabStyle,
    pub(super) hover_pos: Option<egui::Pos2>,
    pub(super) icon_fade: f32,
    pub(super) tab_h: f32,
}

/// Pintar las pestañas + hover + destino del arrastre. Durante la animación
/// de intercambio la pestaña ARRASTRADA vuela desde el punto de la soltada
/// (`release_pos`, donde estaba su fantasma) hasta su ranura definitiva,
/// y la OTRA se desliza (`slide`) desde su posición anterior: arrancan
/// las dos en el sitio exacto donde se les vio y terminan permutadas.
pub(super) fn paint_tabs(ui: &mut egui::Ui, p: PaintPass) {
    for (i, (tab, rect)) in p.tab_rects.iter().enumerate() {
        let is_active = p.active_tab == *tab;
        let is_drop_target = p.drag.as_ref().is_some_and(|d| d.moved && d.tab != *tab)
            && p.hover_pos.is_some_and(|pos| rect.contains(pos));

        let mut visual_rect = *rect;
        if let Some(sw) = p.swap {
            if *tab == sw.dragged && p.tab_rects.len() == 2 {
                // La arrastrada: desde el cursor hasta el centro de su ran.
                let ease = (p.slide / (p.tab_h + TAB_GAP)).clamp(0.0, 1.0);
                let center = rect.center().lerp(sw.release_pos, 1.0 - ease);
                visual_rect = egui::Rect::from_center_size(center, rect.size());
            } else {
                let dy = if i == 0 { p.slide } else { -p.slide };
                visual_rect = rect.translate(egui::vec2(0.0, dy));
            }
        }

        let resp = ui.interact(visual_rect, tab_hover_id(*tab), egui::Sense::hover());
        if resp.hovered() && p.drag.is_none() {
            ui.painter()
                .rect_filled(visual_rect, 4.0, p.style.hover_fill);
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        draw_vertical_tab(
            ui.painter(),
            visual_rect,
            is_active,
            false,
            p.style,
            tab_icon(*tab),
            p.icon_fade,
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
        if p.drag.is_none() {
            let _ = resp.on_hover_text(tab_tip(*tab));
        }
    }

    // Fantasma en el cursor mientras se arrastra.
    if let Some(d) = p.drag {
        if d.moved {
            if let Some(pos) = p.hover_pos {
                paint_drag_ghost(ui, pos, d.tab, p.tab_h, p.style);
            }
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        }
    }
}

/// Fantasma de la pestaña arrastrada, un poco por encima del cursor:
/// siempre inactivo y sin desvanecer, pero con la tinta del color ACTIVO
/// (así era antes del agrupado: se pasa `active_c` también en la ranura de
/// `inactive_c`).
fn paint_drag_ghost(ui: &egui::Ui, pos: egui::Pos2, tab: LeftTab, tab_h: f32, style: &TabStyle) {
    let ghost = egui::Rect::from_center_size(
        pos + egui::vec2(0.0, -10.0),
        egui::vec2(STRIP_WIDTH - 6.0, tab_h - 6.0),
    );
    ui.painter().rect_filled(
        ghost,
        6.0,
        egui::Color32::from_rgba_unmultiplied(0, 122, 255, 40),
    );
    let ghost_style = TabStyle {
        inactive_c: style.active_c,
        ..*style
    };
    draw_vertical_tab(
        ui.painter(),
        ghost,
        false,
        false,
        &ghost_style,
        tab_icon(tab),
        1.0,
    );
}
