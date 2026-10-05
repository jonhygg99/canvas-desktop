//! Selector explícito de edición y encuadre; cada modo conserva su historial.
use super::EditorState;
use eframe::egui;

pub(crate) fn mode_ui(state: &mut EditorState, ui: &mut egui::Ui) {
    let framing = state.framing.is_some();
    let busy = state.framing.as_ref().is_some_and(|session| session.busy());
    if ui
        .add_enabled(!busy, egui::Button::new("Edit").selected(!framing))
        .clicked()
    {
        if let Some(session) = &mut state.framing {
            session.closed = true;
        }
    }
    if ui
        .add_enabled(!busy, egui::Button::new("Framing 9:16").selected(framing))
        .clicked()
        && !framing
    {
        state.framing_requested = true;
    }
}
