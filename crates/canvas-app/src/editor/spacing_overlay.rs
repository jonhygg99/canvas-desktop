//! Distancias entre bordes vecinos durante transformaciones, en píxeles
//! del documento. Las cajas incluyen las esquinas de objetos rotados.
use super::{
    interaction::Gesture, layer_ops::selection_transforms, overlay::show_drag_tag,
    selection_geometry::bounds, viewport::page_to_screen, EditorState,
};
use canvas_core::Transform;
use eframe::egui;
type Gap = ((f64, f64), (f64, f64), f64);
fn nearest(selected: Transform, others: &[Transform], vertical: bool) -> Option<Gap> {
    let (a, b, size, cross) = if vertical {
        (selected.y, selected.x, selected.height, selected.width)
    } else {
        (selected.x, selected.y, selected.width, selected.height)
    };
    let mut best: Option<Gap> = None;
    for other in others {
        let (o, p, len, span) = if vertical {
            (other.y, other.x, other.height, other.width)
        } else {
            (other.x, other.y, other.width, other.height)
        };
        if p >= b + cross || p + span <= b {
            continue;
        }
        let (start, end) = if o + len <= a {
            (o + len, a)
        } else if o >= a + size {
            (a + size, o)
        } else {
            continue;
        };
        let gap = end - start;
        if best.as_ref().is_none_or(|(_, _, distance)| gap < *distance) {
            let c = (b.max(p) + (b + cross).min(p + span)) / 2.0;
            best = Some(if vertical {
                ((c, start), (c, end), gap)
            } else {
                ((start, c), (end, c), gap)
            });
        }
    }
    best
}
pub(super) fn draw(state: &EditorState, ui: &egui::Ui, coord: egui::Rect, clip: egui::Rect) {
    if !matches!(state.gesture, Gesture::Move { .. } | Gesture::Selection(_)) {
        return;
    }
    let selected = selection_transforms(state);
    let Some(box_) = bounds(&selected) else {
        return;
    };
    let Ok(page) = state.doc.page() else {
        return;
    };
    let others: Vec<_> = page
        .layers
        .iter()
        .filter(|l| {
            !matches!(l.content, canvas_core::LayerContent::Group(_))
                && !selected.iter().any(|(id, _)| *id == l.id)
                && page.effective_visible(l.id)
        })
        .filter_map(|l| bounds(&[(l.id, l.transform)]))
        .collect();
    let painter = ui.painter_at(clip);
    let stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(255, 64, 129));
    for vertical in [false, true] {
        if let Some((a, b, gap)) = nearest(box_, &others, vertical) {
            let a = page_to_screen(&state.viewport, coord, a.0, a.1);
            let b = page_to_screen(&state.viewport, coord, b.0, b.1);
            painter.line_segment([a, b], stroke);
            let tick = if vertical {
                egui::vec2(4.0, 0.0)
            } else {
                egui::vec2(0.0, 4.0)
            };
            for p in [a, b] {
                painter.line_segment([p - tick, p + tick], stroke);
            }
            let mid = a + (b - a) * 0.5;
            if clip.contains(mid) {
                show_drag_tag(ui, mid - egui::vec2(14.0, 16.0), format!("{gap:.0} px"));
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn measures_nearest_nonoverlapping_neighbor_and_requires_cross_axis_overlap() {
        let selected = Transform::new(20.0, 20.0, 40.0, 40.0);
        let others = [
            Transform::new(100.0, 20.0, 40.0, 40.0),
            Transform::new(65.0, 80.0, 20.0, 20.0),
        ];
        assert_eq!(nearest(selected, &others, false).unwrap().2, 40.0);
        assert!(nearest(selected, &others, true).is_none());
    }
}
