//! Colocación de texto/formas: clic con tamaño por defecto o arrastre para
//! dibujar su caja. El documento solo cambia al confirmar la colocación.
use super::{
    viewport::{page_to_screen, screen_to_page},
    EditorState, ACCENT,
};
use canvas_core::{LayerContent, Transform};
use eframe::egui;
pub(crate) struct InsertTool {
    pub(crate) name: String,
    pub(crate) width: f64,
    pub(crate) height: f64,
    pub(crate) content: LayerContent,
    start: Option<(f64, f64)>,
    preview: Option<Transform>,
}
impl InsertTool {
    pub(crate) fn new(name: &str, width: f64, height: f64, content: LayerContent) -> Self {
        Self {
            name: name.to_owned(),
            width,
            height,
            content,
            start: None,
            preview: None,
        }
    }
    pub(crate) fn dragging(&self) -> bool {
        self.start.is_some()
    }
}
pub(super) fn handle(
    state: &mut EditorState,
    ui: &egui::Ui,
    r: &egui::Response,
    rect: egui::Rect,
) -> bool {
    let Some(mut tool) = state.insert_tool.take() else {
        return false;
    };
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        return true;
    }
    ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
    let Ok(page) = state.doc.page() else {
        return true;
    };
    let (pw, ph) = (page.width, page.height);
    let clamp = |(x, y): (f64, f64)| (x.clamp(0.0, pw), y.clamp(0.0, ph));
    if r.drag_started_by(egui::PointerButton::Primary) {
        tool.start = ui
            .input(|i| i.pointer.press_origin())
            .map(|p| clamp(screen_to_page(&state.viewport, rect, p)));
    }
    if let Some(pos) = r.interact_pointer_pos().or_else(|| r.hover_pos()) {
        let point = clamp(screen_to_page(&state.viewport, rect, pos));
        tool.preview = Some(if let Some(start) = tool.start {
            Transform::new(
                start.0.min(point.0),
                start.1.min(point.1),
                (point.0 - start.0).abs().max(1.0),
                (point.1 - start.1).abs().max(1.0),
            )
        } else {
            let scale = (pw / tool.width).min(ph / tool.height).min(1.0);
            let (w, h) = (tool.width * scale, tool.height * scale);
            Transform::new(
                point.0.min((pw - w).max(0.0)),
                point.1.min((ph - h).max(0.0)),
                w,
                h,
            )
        });
    }
    if r.clicked_by(egui::PointerButton::Primary) || r.drag_stopped_by(egui::PointerButton::Primary)
    {
        if let Some(t) = tool.preview {
            let text = matches!(tool.content, LayerContent::Text(_));
            state.insert_layer_at(&tool.name, t, tool.content);
            if text {
                if let Some(id) = state.selection.primary() {
                    super::inline_text::begin(state, id);
                }
            }
        }
    } else {
        state.insert_tool = Some(tool);
    }
    true
}
pub(super) fn draw(state: &EditorState, ui: &egui::Ui, coord: egui::Rect, clip: egui::Rect) {
    let Some(tool) = &state.insert_tool else {
        return;
    };
    let Some(t) = tool.preview else {
        return;
    };
    let a = page_to_screen(&state.viewport, coord, t.x, t.y);
    let b = page_to_screen(&state.viewport, coord, t.x + t.width, t.y + t.height);
    let r = egui::Rect::from_two_pos(a, b);
    let painter = ui.painter_at(clip);
    painter.rect_filled(r, 2.0, ACCENT.gamma_multiply(0.12));
    painter.rect_stroke(
        r,
        2.0,
        egui::Stroke::new(1.5, ACCENT),
        egui::StrokeKind::Inside,
    );
    if clip.contains(a) {
        super::overlay::show_drag_tag(
            ui,
            a,
            format!(
                "{} · {}",
                crate::i18n::tr(&tool.name),
                super::overlay::format_dims(&t)
            ),
        );
    }
}
