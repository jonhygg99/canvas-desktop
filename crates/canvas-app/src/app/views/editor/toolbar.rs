//! Navegación y acciones del documento, independientes del inspector.
use super::super::super::frame::EditorFrame;
use crate::{editor, loader, menus};
use eframe::egui;

pub(super) fn show(
    state: &mut editor::EditorState,
    ui: &mut egui::Ui,
    f: &mut EditorFrame<'_>,
) -> Option<menus::MenuAction> {
    let mut action = None;
    egui::Panel::top("document_toolbar").show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            if state.from_gallery.is_some() && ui.button("Back to gallery").clicked() {
                state.return_requested = true;
            }
            crate::editor::properties_panel::file_name_ui(state, ui);
            ui.separator();
            ui.label(save_status(state));
            if ui.button("Settings").clicked() {
                state.settings_clicked = true;
            }
        });
        ui.horizontal_wrapped(|ui| {
            editor::document_bar::mode_ui(state, ui);
            ui.separator();
            let busy = state.saving || state.exporting || state.framing.is_some();
            ui.add_enabled_ui(!busy, |ui| {
                if ui
                    .button("Save editable design…")
                    .on_hover_text("Preserves layers in a standalone .canvas file")
                    .clicked()
                {
                    if state.is_design {
                        state.save_clicked = true;
                    } else {
                        loader::spawn_pick_design_path(
                            Some(state.file_name()),
                            f.tx.clone(),
                            ui.ctx().clone(),
                        );
                    }
                }
                if !state.is_design
                    && ui
                        .button("Overwrite original")
                        .on_hover_text(
                            "Replaces the source image; the existing confirmation still applies",
                        )
                        .clicked()
                {
                    state.save_clicked = true;
                }
                if ui.button("Save as…").clicked() {
                    state.save_as_clicked = true;
                }
                if ui.button("Export image…").clicked() {
                    action = Some(menus::MenuAction::Export);
                }
            });
            if ui.button("Help").clicked() {
                ui.data_mut(|d| {
                    let id = egui::Id::new("editor-help");
                    let open = d.get_temp::<bool>(id).unwrap_or(false);
                    d.insert_temp(id, !open);
                });
            }
        });
        // Ayuda progresiva: no ocupa permanentemente el inspector.
        let help = egui::Id::new("editor-help");
        if ui.data(|d| d.get_temp::<bool>(help).unwrap_or(false)) {
            ui.weak("Wheel: pan · Ctrl/Cmd+wheel: zoom · Space: pan · Ctrl/Cmd+0: fit");
            ui.weak("Ctrl/Cmd+S: save · Ctrl/Cmd+Z: undo · Ctrl/Cmd+C/V: copy/paste layers");
        }
    });
    action
}

fn save_status(state: &editor::EditorState) -> &'static str {
    if let Some(session) = &state.framing {
        return if session.busy() {
            "Processing framing…"
        } else if session.is_dirty() {
            "Framing not saved"
        } else {
            "Framing saved"
        };
    }
    if state.saving {
        "Saving…"
    } else if state.exporting {
        "Exporting…"
    } else if state.save_error.is_some() {
        "Save failed"
    } else if state.is_dirty() {
        "Unsaved changes"
    } else if state.doc.source_path.is_none() {
        "Not saved yet"
    } else {
        "Saved"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unsaved_new_document_is_not_reported_as_saved() {
        let state = editor::EditorState::new_blank(800.0, 600.0);
        assert_ne!(save_status(&state), "Saved");
    }
    #[test]
    fn ongoing_save_takes_precedence_over_previous_error() {
        let mut state = editor::EditorState::new_blank(800.0, 600.0);
        state.save_error = Some("previous failure".into());
        state.saving = true;
        assert_eq!(save_status(&state), "Saving…");
        state.saving = false;
        assert_eq!(save_status(&state), "Save failed");
    }
}
