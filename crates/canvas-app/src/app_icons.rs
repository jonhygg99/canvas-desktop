//! Iconografía unificada de Canvas Desktop: todos los iconos de la app
//! dibujados a mano con el `Painter` de egui, con el mismo estilo de
//! trazo que los triángulos de las cabeceras de lienzo y que la tira de
//! pestañas Page/Layers. Ningún icono depende de que la fuente traiga un
//! glifo Unicode (▲/▼ ya dieron problemas) y todos comparten materiales:
//! trazo fino (≈1.2-1.3 px) y la paleta del estado por el que pasan.
//!
//! Dos piezas de interacción compartidas:
//! - `icon_button_ui`: botón de icono puro (fondo suave al hover y color
//!   activo, igual que cualquier `small_button` emoji de la app, pero con
//!   el glifo dibujado a mano).
//! - `icon_text_button_ui`: botón con icono a la izquierda del texto en
//!   una sola área de clic (sustituye a los «💾 Save» / «✚ New design»).

use eframe::egui;

/// Dirección de `draw_triangle_icon`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum IconDir {
    Up,
    Down,
    Left,
    Right,
}

/// Botón de icono puro (sin texto): área clicable de `size`×`size`, fondo
/// suave al hover y color de icono activo — la misma mecánica que los
/// `small_button` con emoji a los que sustituye. El llamador encadena
/// `.clicked()` / `.on_hover_text` (y comprueba `enabled` al hacer clic:
/// el área sigue asignándose para no mover el layout).
pub fn icon_button_ui(
    ui: &mut egui::Ui,
    size: f32,
    enabled: bool,
    draw: impl FnOnce(&egui::Painter, egui::Rect, egui::Color32),
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::click());
    let visuals = ui.visuals();
    let hovered = enabled && resp.hovered();
    if hovered {
        ui.painter()
            .rect_filled(rect, 4.0, visuals.widgets.hovered.weak_bg_fill);
    }
    let color = if !enabled {
        visuals.weak_text_color()
    } else if hovered {
        visuals.widgets.active.text_color()
    } else {
        visuals.widgets.inactive.text_color()
    };
    draw(ui.painter(), rect, color);
    resp
}

/// Botón con icono a la izquierda y texto: una sola área de clic, con el
/// mismo aspecto de botón (fondo y borde de `widgets.inactive/hovered`).
/// `color_override` fuerza el color de icono y texto (p. ej. el rojo de
/// «Delete»); `min_size` impone un tamaño mínimo (botones grandes de la
/// bienvenida).
pub fn icon_text_button_ui(
    ui: &mut egui::Ui,
    enabled: bool,
    draw: impl FnOnce(&egui::Painter, egui::Rect, egui::Color32),
    text: &str,
    color_override: Option<egui::Color32>,
    min_size: egui::Vec2,
) -> egui::Response {
    let visuals = ui.visuals().clone();
    let font = egui::FontId::proportional(13.0);
    let base = color_override.unwrap_or(visuals.text_color());
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font.clone(), base);
    let icon_sz = 14.0;
    let pad_x = 7.0;
    let gap = 5.0;
    let size = egui::vec2(
        (pad_x * 2.0 + icon_sz + gap + galley.size().x).max(min_size.x),
        ((galley.size().y + 8.0).max(24.0)).max(min_size.y),
    );
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    let hovered = enabled && resp.hovered();
    let bg = if hovered {
        visuals.widgets.hovered.bg_fill
    } else {
        visuals.widgets.inactive.bg_fill
    };
    ui.painter().rect(
        rect,
        6.0,
        bg,
        visuals.widgets.inactive.bg_stroke,
        egui::StrokeKind::Inside,
    );
    let color = if !enabled {
        visuals.weak_text_color()
    } else if hovered {
        visuals.widgets.active.text_color()
    } else {
        base
    };
    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(rect.left() + pad_x + icon_sz / 2.0, rect.center().y),
        egui::vec2(icon_sz, icon_sz),
    );
    draw(ui.painter(), icon_rect, color);
    ui.painter().text(
        egui::pos2(icon_rect.right() + gap, rect.center().y),
        egui::Align2::LEFT_CENTER,
        text,
        font,
        color,
    );
    resp
}

/// Icono sin sentido de clic (marcador visual, p. ej. el fondo desenfocado
/// en la lista de capas): solo hover para tooltip y color.
pub fn icon_label_ui(
    ui: &mut egui::Ui,
    size: f32,
    draw: impl FnOnce(&egui::Painter, egui::Rect, egui::Color32),
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let color = if resp.hovered() {
        ui.visuals().widgets.active.text_color()
    } else {
        ui.visuals().widgets.inactive.text_color()
    };
    draw(ui.painter(), rect, color);
    resp
}

fn stroke(color: egui::Color32) -> egui::Stroke {
    egui::Stroke::new(1.3, color)
}

// ---------------------------------------------------------------------------
// Triángulos (también la flecha de los grupos y los botones de colapso)
// ---------------------------------------------------------------------------

/// Puntos a lo largo de un arco de circunferencia. Convención pensada para
/// que `start = -FRAC_PI_2`/`end = FRAC_PI_2` trace un semicírculo superior
/// de izquierda a derecha pasando por arriba (coordenadas de pantalla, eje Y
/// hacia abajo) — el arco del candado.
pub fn arc_points(
    center: egui::Pos2,
    radius: f32,
    start: f32,
    end: f32,
    segments: usize,
) -> Vec<egui::Pos2> {
    (0..=segments)
        .map(|i| {
            let t = start + (end - start) * (i as f32 / segments as f32);
            center + egui::vec2(t.sin(), -t.cos()) * radius
        })
        .collect()
}

/// Triángulo relleno apuntando en `dir`, centrado en `rect`.
pub fn draw_triangle_icon(
    painter: &egui::Painter,
    rect: egui::Rect,
    dir: IconDir,
    color: egui::Color32,
) {
    let c = rect.center();
    let s = rect.width().min(rect.height()) * 0.28;
    let points = match dir {
        IconDir::Up => vec![
            c + egui::vec2(0.0, -s),
            c + egui::vec2(-s, s * 0.8),
            c + egui::vec2(s, s * 0.8),
        ],
        IconDir::Down => vec![
            c + egui::vec2(0.0, s),
            c + egui::vec2(-s, -s * 0.8),
            c + egui::vec2(s, -s * 0.8),
        ],
        IconDir::Left => vec![
            c + egui::vec2(-s, 0.0),
            c + egui::vec2(s * 0.8, -s),
            c + egui::vec2(s * 0.8, s),
        ],
        IconDir::Right => vec![
            c + egui::vec2(s, 0.0),
            c + egui::vec2(-s * 0.8, -s),
            c + egui::vec2(-s * 0.8, s),
        ],
    };
    painter.add(egui::Shape::convex_polygon(
        points,
        color,
        egui::Stroke::NONE,
    ));
}

/// Serie de dos flechas (⇅ / ⇆ / ⇋): `horizontal` elige el eje; `bar`
/// añade la barra central de los iconos de volteo.
pub fn draw_double_arrow_icon(
    painter: &egui::Painter,
    rect: egui::Rect,
    horizontal: bool,
    bar: bool,
    color: egui::Color32,
) {
    let c = rect.center();
    let s = rect.width().min(rect.height());
    if horizontal {
        let l = egui::pos2(c.x - s * 0.42, c.y);
        let r_ = egui::pos2(c.x + s * 0.42, c.y);
        painter.add(egui::Shape::convex_polygon(
            vec![
                l,
                egui::pos2(c.x - s * 0.28, c.y - s * 0.24),
                egui::pos2(c.x - s * 0.28, c.y + s * 0.24),
            ],
            color,
            egui::Stroke::NONE,
        ));
        painter.add(egui::Shape::convex_polygon(
            vec![
                r_,
                egui::pos2(c.x + s * 0.28, c.y - s * 0.24),
                egui::pos2(c.x + s * 0.28, c.y + s * 0.24),
            ],
            color,
            egui::Stroke::NONE,
        ));
        if bar {
            painter.line_segment(
                [
                    egui::pos2(c.x, c.y - s * 0.12),
                    egui::pos2(c.x, c.y + s * 0.12),
                ],
                egui::Stroke::new(1.6, color),
            );
        }
    } else {
        let top = egui::pos2(c.x, c.y - s * 0.42);
        let bot = egui::pos2(c.x, c.y + s * 0.42);
        painter.add(egui::Shape::convex_polygon(
            vec![
                top,
                egui::pos2(c.x - s * 0.24, c.y - s * 0.28),
                egui::pos2(c.x + s * 0.24, c.y - s * 0.28),
            ],
            color,
            egui::Stroke::NONE,
        ));
        painter.add(egui::Shape::convex_polygon(
            vec![
                bot,
                egui::pos2(c.x - s * 0.24, c.y + s * 0.28),
                egui::pos2(c.x + s * 0.24, c.y + s * 0.28),
            ],
            color,
            egui::Stroke::NONE,
        ));
        if bar {
            painter.line_segment(
                [
                    egui::pos2(c.x - s * 0.12, c.y),
                    egui::pos2(c.x + s * 0.12, c.y),
                ],
                egui::Stroke::new(1.6, color),
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Candado, duplicar, borrar (movidos desde `slot_chrome::icons`)
// ---------------------------------------------------------------------------

/// Candado: cuerpo relleno (rect redondeado) + arco. Cerrado, el arco se
/// apoya simétrico sobre el cuerpo; abierto, se desplaza hacia arriba y a
/// la derecha, dejando el lado izquierdo suelto.
pub fn draw_lock_icon(
    painter: &egui::Painter,
    rect: egui::Rect,
    locked: bool,
    color: egui::Color32,
) {
    let half_pi = std::f32::consts::FRAC_PI_2;
    let body_w = rect.width() * 0.5;
    let body_h = rect.height() * 0.42;
    let body = egui::Rect::from_center_size(
        rect.center() + egui::vec2(0.0, body_h * 0.35),
        egui::vec2(body_w, body_h),
    );
    painter.rect_filled(body, 1.0, color);
    let shackle_r = body_w * 0.42;
    let stroke = egui::Stroke::new(1.3, color);
    let shackle_center = egui::pos2(body.center().x, body.top());
    let center = if locked {
        shackle_center
    } else {
        shackle_center + egui::vec2(shackle_r * 0.55, -shackle_r * 0.35)
    };
    let pts = arc_points(center, shackle_r, -half_pi, half_pi, 10);
    painter.add(egui::Shape::line(pts, stroke));
}

/// Duplicar: dos rects redondeados solapados (icono estándar de «copiar»).
/// `bg` es el fondo de la propia cabecera — rellena el rect de delante para
/// que de verdad tape la esquina del de detrás.
pub fn draw_duplicate_icon(
    painter: &egui::Painter,
    rect: egui::Rect,
    color: egui::Color32,
    bg: egui::Color32,
) {
    let s = rect.width().min(rect.height()) * 0.36;
    let stroke = egui::Stroke::new(1.2, color);
    let back = egui::Rect::from_center_size(
        rect.center() + egui::vec2(-s * 0.28, -s * 0.28),
        egui::vec2(s, s),
    );
    let front = egui::Rect::from_center_size(
        rect.center() + egui::vec2(s * 0.28, s * 0.28),
        egui::vec2(s, s),
    );
    painter.rect_stroke(back, 1.0, stroke, egui::StrokeKind::Outside);
    painter.rect_filled(front, 1.0, bg);
    painter.rect_stroke(front, 1.0, stroke, egui::StrokeKind::Outside);
}

/// Borrar: cubo de basura simple — cuerpo, tapa y un par de ranuras.
pub fn draw_delete_icon(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let stroke = egui::Stroke::new(1.2, color);
    let w = rect.width() * 0.42;
    let h = rect.height() * 0.4;
    let body =
        egui::Rect::from_center_size(rect.center() + egui::vec2(0.0, h * 0.2), egui::vec2(w, h));
    painter.rect_stroke(body, 0.5, stroke, egui::StrokeKind::Outside);
    painter.line_segment(
        [
            egui::pos2(body.left() - w * 0.15, body.top()),
            egui::pos2(body.right() + w * 0.15, body.top()),
        ],
        stroke,
    );
    for i in [-1.0_f32, 1.0] {
        let x = rect.center().x + i * w * 0.22;
        painter.line_segment(
            [
                egui::pos2(x, body.top() + h * 0.18),
                egui::pos2(x, body.bottom() - h * 0.12),
            ],
            stroke,
        );
    }
}

/// Grupo: dos cuadrados solapados (equivalente al «▣» de la barra del
/// panel de capas).
pub fn draw_group_icon(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let s = rect.width().min(rect.height()) * 0.62;
    let c = rect.center();
    let back = egui::Rect::from_center_size(c - egui::vec2(s * 0.22, s * 0.22), egui::vec2(s, s));
    let front = egui::Rect::from_center_size(c + egui::vec2(s * 0.22, s * 0.22), egui::vec2(s, s));
    painter.rect_stroke(back, 2.0, stroke(color), egui::StrokeKind::Outside);
    painter.rect_stroke(front, 2.0, stroke(color), egui::StrokeKind::Outside);
}

/// Desagrupar: dos cuadrados separados (equivalente al «▤»).
pub fn draw_ungroup_icon(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let s = rect.width().min(rect.height()) * 0.36;
    let yc = rect.center().y;
    for side in [-1.0_f32, 1.0] {
        let sq = egui::Rect::from_center_size(
            egui::pos2(rect.center().x + side * s * 0.85, yc),
            egui::vec2(s, s),
        );
        painter.rect_stroke(sq, 2.0, stroke(color), egui::StrokeKind::Outside);
    }
}

// ---------------------------------------------------------------------------
// Iconos de la tira de pestañas y el panel (movidos desde layers_panel)
// ---------------------------------------------------------------------------

pub fn draw_page_icon(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
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

pub fn draw_layers_icon(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
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

// ---------------------------------------------------------------------------
// Iconos de acción del panel de propiedades y de la galería
// ---------------------------------------------------------------------------

fn ellipse_points(center: egui::Pos2, rx: f32, ry: f32, segments: usize) -> Vec<egui::Pos2> {
    (0..=segments)
        .map(|i| {
            let t = std::f32::consts::TAU * i as f32 / segments as f32;
            center + egui::vec2(rx * t.cos(), ry * t.sin())
        })
        .collect()
}

/// Ojo (visibilidad). `visible = false` añade la barra diagonal de
/// «oculto» (el antiguo 🚫).
pub fn draw_eye_icon(
    painter: &egui::Painter,
    rect: egui::Rect,
    visible: bool,
    color: egui::Color32,
) {
    let c = rect.center();
    let rx = rect.width() * 0.42;
    let ry = rect.height() * 0.30;
    painter.add(egui::Shape::line(
        ellipse_points(c, rx, ry, 12),
        stroke(color),
    ));
    painter.circle_filled(c, ry * 0.34, color);
    if !visible {
        painter.line_segment(
            [
                rect.left_top() + egui::vec2(1.0, 1.0),
                rect.right_bottom() - egui::vec2(1.0, 1.0),
            ],
            egui::Stroke::new(1.8, color),
        );
    }
}

/// Ajustes: rueda dentada simple (anillo + radios).
pub fn draw_gear_icon(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let c = rect.center();
    let s = rect.width().min(rect.height());
    let r_in = s * 0.20;
    let r_out = s * 0.44;
    painter.circle_stroke(c, r_out, egui::Stroke::new(1.2, color));
    painter.circle_stroke(c, r_in, egui::Stroke::new(1.2, color));
    for i in 0..8 {
        let a = std::f32::consts::TAU * i as f32 / 8.0;
        let d = egui::vec2(a.cos(), a.sin());
        painter.line_segment([c + d * r_in, c + d * r_out], egui::Stroke::new(1.6, color));
    }
}

/// Renombrar (lápiz estilizado: cuerpo + punta + goma).
pub fn draw_pencil_icon(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let c = rect.center();
    let s = rect.width().min(rect.height());
    let a = c + egui::vec2(-s * 0.32, s * 0.16);
    let b = c + egui::vec2(s * 0.02, -s * 0.30);
    painter.line_segment([a, b], egui::Stroke::new(1.8, color));
    let tip = b + egui::vec2(s * 0.26, -s * 0.12);
    painter.add(egui::Shape::convex_polygon(
        vec![b, tip, b + egui::vec2(s * 0.10, s * 0.12)],
        color,
        egui::Stroke::NONE,
    ));
    let eraser = egui::Rect::from_center_size(
        a + egui::vec2(-s * 0.12, s * 0.02),
        egui::vec2(s * 0.18, s * 0.22),
    );
    painter.rect_stroke(
        eraser,
        1.0,
        egui::Stroke::new(1.2, color),
        egui::StrokeKind::Outside,
    );
}

/// Cerrar / descartar (X).
pub fn draw_close_icon(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let c = rect.center();
    let s = rect.width().min(rect.height()) * 0.36;
    let st = egui::Stroke::new(1.6, color);
    painter.line_segment([c - egui::vec2(s, s), c + egui::vec2(s, s)], st);
    painter.line_segment([c + egui::vec2(-s, s), c + egui::vec2(s, -s)], st);
}

/// Confirmar (✓).
pub fn draw_check_icon(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let c = rect.center();
    let s = rect.width().min(rect.height());
    let st = egui::Stroke::new(1.6, color);
    painter.line_segment(
        [
            c + egui::vec2(-s * 0.38, s * 0.02),
            c + egui::vec2(-s * 0.06, s * 0.30),
        ],
        st,
    );
    painter.line_segment(
        [
            c + egui::vec2(-s * 0.06, s * 0.30),
            c + egui::vec2(s * 0.42, -s * 0.30),
        ],
        st,
    );
}

/// Añadir (más).
pub fn draw_plus_icon(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let c = rect.center();
    let s = rect.width().min(rect.height());
    let st = egui::Stroke::new(1.6, color);
    painter.line_segment(
        [c - egui::vec2(0.0, s * 0.36), c + egui::vec2(0.0, s * 0.36)],
        st,
    );
    painter.line_segment(
        [c - egui::vec2(s * 0.36, 0.0), c + egui::vec2(s * 0.36, 0.0)],
        st,
    );
}

/// Quitar (menos).
pub fn draw_minus_icon(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let c = rect.center();
    let s = rect.width().min(rect.height());
    painter.line_segment(
        [c - egui::vec2(s * 0.36, 0.0), c + egui::vec2(s * 0.36, 0.0)],
        egui::Stroke::new(1.6, color),
    );
}

/// Documento (glifo de página de las miniaturas, el antiguo 🖹).
pub fn draw_doc_icon(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let c = rect.center();
    let s = rect.width().min(rect.height());
    let st = stroke(color);
    let body = egui::Rect::from_center_size(c, egui::vec2(s * 0.66, s * 0.82));
    painter.rect_stroke(body, 2.0, st, egui::StrokeKind::Outside);
    let fold = s * 0.22;
    painter.line_segment(
        [
            egui::pos2(body.right() - fold, body.top()),
            egui::pos2(body.right(), body.top() + fold),
        ],
        st,
    );
}

/// Aviso (⚠): triángulo + signo de exclamación.
pub fn draw_warning_icon(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let s = rect.width().min(rect.height());
    let c = rect.center();
    let top = egui::pos2(c.x, rect.top() + s * 0.08);
    let bl = egui::pos2(rect.left() + s * 0.08, rect.bottom() - s * 0.08);
    let br = egui::pos2(rect.right() - s * 0.08, rect.bottom() - s * 0.08);
    painter.add(egui::Shape::line(vec![top, bl, br, top], stroke(color)));
    let cx = c.x;
    painter.line_segment(
        [
            egui::pos2(cx, rect.top() + s * 0.32),
            egui::pos2(cx, rect.top() + s * 0.58),
        ],
        stroke(color),
    );
    painter.circle_filled(egui::pos2(cx, rect.top() + s * 0.72), 1.2, color);
}

/// Cargando (rueda de arco giratoria, sustituye al ⏳). `time` es el reloj
/// de egui: mantiene el giro aunque no entre input (el frame la repinta).
pub fn draw_spinner_icon(
    painter: &egui::Painter,
    rect: egui::Rect,
    time: f64,
    color: egui::Color32,
) {
    let s = rect.width().min(rect.height());
    let c = rect.center();
    let radius = s * 0.40;
    let start = (time as f32) * 3.0;
    let pts = arc_points(c, radius, start, start + 4.6, 14);
    painter.add(egui::Shape::line(pts, egui::Stroke::new(1.8, color)));
}

/// Carpeta (icono «Open folder» de la bienvenida).
pub fn draw_folder_icon(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let w = rect.width();
    let h = rect.height();
    let (l, t, r, b) = (rect.left(), rect.top(), rect.right(), rect.bottom());
    let pts = vec![
        egui::pos2(l + w * 0.04, t + h * 0.40),
        egui::pos2(l + w * 0.24, t + h * 0.40),
        egui::pos2(l + w * 0.34, t + h * 0.26),
        egui::pos2(r - w * 0.10, t + h * 0.26),
        egui::pos2(r - w * 0.04, b - h * 0.04),
        egui::pos2(l + w * 0.04, b - h * 0.04),
        egui::pos2(l + w * 0.04, t + h * 0.40),
    ];
    painter.add(egui::Shape::line(pts, stroke(color)));
}

/// Destello (✨ de la bienvenida): cuatro rayos concéntricos de longitud
/// alternada.
pub fn draw_sparkle_icon(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let c = rect.center();
    let s = rect.width().min(rect.height());
    let st = egui::Stroke::new(1.5, color);
    for (dx, dy, len) in [
        (1.0f32, 1.0f32, 0.52),
        (-1.0, 1.0, 0.52),
        (1.0, -1.0, 0.52),
        (-1.0, -1.0, 0.52),
        (1.0, 0.0, 0.30),
        (-1.0, 0.0, 0.30),
        (0.0, 1.0, 0.30),
        (0.0, -1.0, 0.30),
    ] {
        painter.line_segment(
            [c, c + egui::vec2(dx * s * len / 2.0, dy * s * len / 2.0)],
            st,
        );
    }
}

// ---------------------------------------------------------------------------
// Previews de la pestaña Insert: la silueta REAL de lo que se va a insertar
// (no un icono abstracto), dibujada con el mismo trazo que el resto.
// ---------------------------------------------------------------------------

/// Texto: glifo «Aa» pintado con la fuente de la app (el antiguo botón
/// «T Text»). Sin Unicode raro: son dos letras ASCII.
pub fn draw_text_preview(
    painter: &egui::Painter,
    rect: egui::Rect,
    color: egui::Color32,
) {
    let font = egui::FontId::proportional(rect.height() * 0.62);
    painter.text(rect.center(), egui::Align2::CENTER_CENTER, "Aa", font, color);
}

/// Rectángulo relleno suave con borde, como se verá en el lienzo.
pub fn draw_rect_preview(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let s = rect.width().min(rect.height()) * 0.58;
    let body = egui::Rect::from_center_size(rect.center(), egui::vec2(s * 1.15, s * 0.85));
    painter.rect_filled(body, 2.5, color.gamma_multiply(0.35));
    painter.rect_stroke(
        body,
        2.5,
        egui::Stroke::new(1.4, color),
        egui::StrokeKind::Outside,
    );
}

/// Elipse/círculo relleno suave con borde.
pub fn draw_ellipse_preview(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let s = rect.width().min(rect.height()) * 0.30;
    painter.circle_filled(rect.center(), s, color.gamma_multiply(0.35));
    painter.circle_stroke(rect.center(), s, egui::Stroke::new(1.4, color));
}

/// Línea gruesa con tapas redondas, como la capa Line del lienzo (el
/// trazo grueso es lo que la distingue de un palo fino: la mitad del
/// alto de la cabeza de la flecha, para que ambas concuerden).
pub fn draw_line_preview(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let s = rect.width().min(rect.height());
    let c = rect.center();
    let d = egui::vec2(s * 0.34, -s * 0.20);
    let (a, b) = (c - d, c + d);
    let w = (s * 0.20).max(3.0);
    painter.line_segment([a, b], egui::Stroke::new(w, color));
    // egui no tiene tapas redondas en `Stroke`: se simulan con un círculo
    // en cada extremo.
    let r = w / 2.0;
    painter.circle_filled(a, r, color);
    painter.circle_filled(b, r, color);
}

/// Triángulo regular apuntando hacia arriba (silueta real de la capa).
pub fn draw_triangle_preview(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let c = rect.center();
    let s = rect.width().min(rect.height());
    let pts = vec![
        c + egui::vec2(0.0, -s * 0.30),
        c + egui::vec2(-s * 0.30, s * 0.26),
        c + egui::vec2(s * 0.30, s * 0.26),
    ];
    painter.add(egui::Shape::convex_polygon(
        pts.clone(),
        color.gamma_multiply(0.35),
        egui::Stroke::NONE,
    ));
    painter.add(egui::Shape::closed_line(
        pts,
        egui::Stroke::new(1.4, color),
    ));
}

/// Estrella de cinco puntas (silueta real de la capa).
pub fn draw_star_preview(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let c = rect.center();
    let s = rect.width().min(rect.height());
    let r_out = s * 0.42;
    let r_in = r_out * 0.45;
    let mut pts = Vec::with_capacity(10);
    for i in 0..10 {
        let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * i as f32 / 5.0;
        let r = if i % 2 == 0 { r_out } else { r_in };
        pts.push(c + egui::vec2(a.cos() * r, a.sin() * r));
    }
    painter.add(egui::Shape::convex_polygon(
        pts.clone(),
        color.gamma_multiply(0.35),
        egui::Stroke::NONE,
    ));
    painter.add(egui::Shape::closed_line(
        pts,
        egui::Stroke::new(1.4, color),
    ));
}

/// Flecha apuntando a la derecha (astil + cabeza, como la capa Arrow).
/// Dibuja la MISMA geometría relativa que la capa (fracciones idénticas
/// sobre la caja), con la cabeza redondeada: astil y triángulo comparten
/// el estilo redondeado.
pub fn draw_arrow_preview(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let (w, h) = (rect.width(), rect.height());
    let y = rect.center().y;
    let shaft_end = rect.left() + w * 0.60;
    let start = egui::pos2(rect.left(), y);
    // Grosor del astil proporcional al de la capa (≈8 % de la altura).
    let shaft_w = (h * 0.08).max(2.5);
    painter.line_segment([start, egui::pos2(shaft_end, y)], egui::Stroke::new(shaft_w, color));
    painter.circle_filled(start, shaft_w / 2.0, color);
    // Cabeza redondeada: canvas-core devuelve la ruta en la caja local
    // (0,0)..(w,h) y aquí se traduce al rect del tile. egui no tiene
    // curvas, así que se aplana la ruta a segmentos rectos. El radio usa
    // el mismo ratio (~18 % del largo de la cabeza) que el default de
    // inserción, para que el preview muestre lo que se va a crear.
    let head = canvas_core::arrow_head_rounded(f64::from(w), f64::from(h), f64::from(w) * 0.38 * 0.18);
    let pts: Vec<egui::Pos2> = head
        .to_polyline(6)
        .iter()
        .map(|(x, y)| egui::pos2(rect.left() + *x as f32, rect.top() + *y as f32))
        .collect();
    painter.add(egui::Shape::convex_polygon(
        pts.clone(),
        color.gamma_multiply(0.35),
        egui::Stroke::NONE,
    ));
    painter.add(egui::Shape::closed_line(
        pts,
        egui::Stroke::new(1.4, color),
    ));
}

/// Centro en página (círculo + punto).
pub fn draw_target_icon(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let c = rect.center();
    let s = rect.width().min(rect.height());
    painter.circle_stroke(c, s * 0.34, egui::Stroke::new(1.3, color));
    painter.circle_filled(c, 1.4, color);
}

/// Cubrir la página (cuatro esquinas, el antiguo ⛶).
pub fn draw_fill_icon(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let s = rect.width().min(rect.height());
    let st = egui::Stroke::new(1.4, color);
    let corner = |corner: (f32, f32)| {
        let (sx, sy) = corner;
        let base = rect.center() + egui::vec2(sx * s * 0.34, sy * s * 0.34);
        let d = egui::vec2(sx * s * 0.22, sy * s * 0.22);
        painter.line_segment([base - egui::vec2(d.x, 0.0), base], st);
        painter.line_segment([base - egui::vec2(0.0, d.y), base], st);
    };
    corner((1.0, 1.0));
    corner((1.0, -1.0));
    corner((-1.0, 1.0));
    corner((-1.0, -1.0));
}

/// Recorte (tijeras estilizadas del botón «Crop»).
pub fn draw_crop_icon(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let c = rect.center();
    let s = rect.width().min(rect.height());
    let a = c + egui::vec2(-s * 0.30, s * 0.24);
    let b = c + egui::vec2(s * 0.30, -s * 0.24);
    painter.circle_stroke(a, s * 0.11, stroke(color));
    painter.circle_stroke(b, s * 0.11, stroke(color));
    let st = egui::Stroke::new(1.4, color);
    painter.line_segment(
        [
            a + egui::vec2(s * 0.10, -s * 0.08),
            b + egui::vec2(-s * 0.10, s * 0.08),
        ],
        st,
    );
    painter.line_segment(
        [
            a + egui::vec2(-s * 0.12, s * 0.06),
            b + egui::vec2(s * 0.12, -s * 0.06),
        ],
        st,
    );
}

/// Aislamiento (el antiguo «I» de la cabecera): recuadro con hueco
/// central — «solo esto».
pub fn draw_isolate_icon(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let c = rect.center();
    let s = rect.width().min(rect.height());
    let outer = egui::Rect::from_center_size(
        c + egui::vec2(s * 0.12, s * 0.12),
        egui::vec2(s * 0.40, s * 0.40),
    );
    painter.rect_stroke(outer, 1.0, stroke(color), egui::StrokeKind::Outside);
    let inner = egui::Rect::from_center_size(
        c - egui::vec2(s * 0.12, s * 0.12),
        egui::vec2(s * 0.34, s * 0.34),
    );
    painter.rect_stroke(inner, 1.0, stroke(color), egui::StrokeKind::Outside);
}

/// Fondo desenfocado (el antiguo 🌫): tres parches de niebla.
pub fn draw_blur_icon(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let c = rect.center();
    let s = rect.width().min(rect.height());
    painter.circle_filled(c + egui::vec2(-s * 0.22, 0.0), s * 0.13, color);
    painter.circle_filled(c + egui::vec2(s * 0.26, 0.0), s * 0.10, color);
    painter.circle_filled(c + egui::vec2(s * 0.02, -s * 0.02), s * 0.16, color);
}

/// Chincheta (pin): cabeza redonda arriba con la aguja vertical hacia
/// abajo. Para fijar una carpeta reciente arriba de la lista.
pub fn draw_pin_icon(
    painter: &egui::Painter,
    rect: egui::Rect,
    color: egui::Color32,
    filled: bool,
) {
    let c = rect.center();
    let s = rect.width().min(rect.height());
    let head_c = c - egui::vec2(0.0, s * 0.12);
    let head_r = s * 0.22;
    if filled {
        painter.circle_filled(head_c, head_r, color);
    } else {
        painter.circle_stroke(head_c, head_r, stroke(color));
    }
    // Aguja
    painter.line_segment(
        [
            head_c + egui::vec2(0.0, head_r),
            c + egui::vec2(0.0, s * 0.35),
        ],
        stroke(color),
    );
}
