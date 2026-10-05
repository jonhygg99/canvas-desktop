//! Cancelación de gestos simples: restaura la instantánea sin marcar cambios.
use super::{interaction::Gesture, EditorState};
use canvas_core::LayerContent;
pub(super) fn cancel_single_gesture(state: &mut EditorState) -> bool {
    if matches!(
        state.gesture,
        Gesture::None | Gesture::Selection(_) | Gesture::Marquee(_)
    ) {
        return false;
    }
    let gesture = std::mem::replace(&mut state.gesture, Gesture::None);
    match gesture {
        Gesture::Move { layer, start, .. }
        | Gesture::Resize { layer, start, .. }
        | Gesture::Rotate { layer, start, .. } => {
            if let Ok(l) = state.doc.layer_mut(layer) {
                l.transform = start;
            }
        }
        Gesture::Crop {
            layer,
            start_t,
            start_crop,
            ..
        } => {
            if let Ok(l) = state.doc.layer_mut(layer) {
                l.transform = start_t;
                match &mut l.content {
                    LayerContent::Image(c) => c.crop = start_crop,
                    LayerContent::Video(c) => c.crop = start_crop,
                    _ => {}
                }
            }
        }
        _ => {}
    }
    state.snap_guides = (Vec::new(), Vec::new());
    true
}
