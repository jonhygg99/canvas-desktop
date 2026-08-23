//! Ventanita para pegar una URL y reemplazar con ella la imagen de la capa
//! seleccionada.

use eframe::egui;

use super::super::EditorState;
use super::CanvasAction;

pub(super) fn replace_url_popup_ui(
    state: &mut EditorState,
    ctx: &egui::Context,
) -> Option<CanvasAction> {
    let (layer, mut url) = state.replace_url_popup.take()?;
    let mut open = true;
    let mut replace = false;
    let mut cancel = false;
    egui::Window::new("Replace from URL")
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut url)
                    .hint_text("https://example.com/image.jpg")
                    .desired_width(360.0),
            );
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!url.trim().is_empty(), egui::Button::new("Replace"))
                    .clicked()
                {
                    replace = true;
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        });

    if replace {
        Some(CanvasAction::ReplaceFromUrl(layer, url.trim().to_owned()))
    } else {
        if open && !cancel {
            state.replace_url_popup = Some((layer, url));
        }
        None
    }
}
