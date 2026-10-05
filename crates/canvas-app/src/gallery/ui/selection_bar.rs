//! Búsqueda y acciones explícitas sobre la selección de la galería.
use crate::gallery::{GalleryAction, GalleryState};
use eframe::egui;

pub(super) fn show(
    state: &mut GalleryState,
    ui: &mut egui::Ui,
    action: &mut Option<GalleryAction>,
) {
    ui.horizontal_wrapped(|ui| {
        ui.label(crate::i18n::tr("Search"));
        let changed = ui
            .add(
                egui::TextEdit::singleline(&mut state.search)
                    .id_salt("gallery-search")
                    .hint_text(crate::i18n::tr("Search by name…"))
                    .desired_width(180.0),
            )
            .changed();
        if changed
            && state.selected.as_ref().is_some_and(|path| {
                !state
                    .items
                    .iter()
                    .any(|item| &item.path == path && state.matches_item(item))
            })
        {
            state.selected = None;
        }
        if !state.search.is_empty() && ui.button(crate::i18n::tr("Clear search")).clicked() {
            state.search.clear();
        }
        let count = state
            .items
            .iter()
            .filter(|item| state.matches_item(item))
            .count();
        ui.weak(format!("{count} / {}", state.items.len()));
    });
    ui.horizontal_wrapped(|ui| {
        if let Some(path) = state.selected.clone() {
            if ui
                .button(crate::i18n::tr("Open"))
                .on_hover_text(crate::i18n::tr("Enter or double-click"))
                .clicked()
            {
                *action = Some(GalleryAction::Open(path.clone()));
            }
            if ui
                .button(crate::i18n::tr("Rename"))
                .on_hover_text(crate::i18n::tr("F2"))
                .clicked()
            {
                if let Some(item) = state.items.iter().find(|item| item.path == path) {
                    super::cell::begin_rename(item, &mut state.rename_edit, ui.ctx());
                }
            }
            if ui.button(crate::i18n::tr("Duplicate")).clicked() {
                *action = Some(GalleryAction::Duplicate(path));
            }
        } else {
            ui.weak(crate::i18n::tr(
                "Click to select · Double-click to open · F2 to rename",
            ));
        }
    });
}
