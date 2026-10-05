//! Búsqueda por nombre/tipo y seguimiento de la selección del lienzo.
use super::{push_rows, Row};
use crate::editor::EditorState;
use canvas_core::{LayerContent, LayerId};
use eframe::egui;

pub(super) fn rows(state: &mut EditorState, ui: &mut egui::Ui) -> Vec<Row> {
    let key = egui::Id::new("layer_browser");
    let (mut query, mut kind) = ui.data(|d| {
        d.get_temp::<(String, String)>(key)
            .unwrap_or((String::new(), "All layers".into()))
    });
    ui.add(
        egui::TextEdit::singleline(&mut query)
            .hint_text(crate::i18n::tr("Search layers"))
            .desired_width(ui.available_width()),
    );
    egui::ComboBox::from_id_salt("layer_type_filter")
        .selected_text(crate::i18n::tr(&kind))
        .show_ui(ui, |ui| {
            for value in [
                "All layers",
                "Text",
                "Shape",
                "Image",
                "Video",
                "Group",
                "SVG",
            ] {
                ui.selectable_value(&mut kind, value.to_owned(), crate::i18n::tr(value));
            }
        });
    ui.data_mut(|d| d.insert_temp(key, (query.clone(), kind.clone())));
    reveal(state, ui);
    let Ok(page) = state.doc.page() else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    if query.trim().is_empty() && kind == "All layers" {
        push_rows(page, None, 0, &mut rows);
    } else {
        for layer in page
            .layers
            .iter()
            .rev()
            .filter(|l| matches(&l.name, &l.content, &query, &kind))
        {
            rows.push(Row {
                id: layer.id,
                depth: 0,
                is_group: matches!(layer.content, LayerContent::Group(_)),
                collapsed: false,
            });
        }
    }
    rows
}
fn matches(name: &str, content: &LayerContent, query: &str, kind: &str) -> bool {
    let actual = match content {
        LayerContent::Text(_) => "Text",
        LayerContent::Shape(_) => "Shape",
        LayerContent::Image(_) => "Image",
        LayerContent::Video(_) => "Video",
        LayerContent::Group(_) => "Group",
        LayerContent::Svg(_) => "SVG",
    };
    (kind == "All layers" || kind == actual)
        && name.to_lowercase().contains(&query.trim().to_lowercase())
}
fn reveal(state: &mut EditorState, ui: &egui::Ui) {
    let id = egui::Id::new("last_layer_selection");
    let selected = (state.active_slot_id, state.selection.primary());
    if ui.data(|d| d.get_temp::<(u64, Option<LayerId>)>(id)) == Some(selected) {
        return;
    }
    ui.data_mut(|d| d.insert_temp(id, selected));
    let Some(mut current) = selected.1 else {
        return;
    };
    ui.data_mut(|d| d.insert_temp(egui::Id::new("reveal_layer"), current));
    for _ in 0..64 {
        let parent = state.doc.layer(current).ok().and_then(|l| l.parent_id);
        let Some(parent) = parent else { break };
        if let Ok(layer) = state.doc.layer_mut(parent) {
            if let LayerContent::Group(group) = &mut layer.content {
                group.collapsed = false;
            }
        }
        current = parent;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn search_combines_case_insensitive_name_and_type() {
        let content = LayerContent::Text(canvas_core::TextContent::default());
        assert!(matches("Main TITLE", &content, " title ", "Text"));
        assert!(!matches("Main TITLE", &content, "title", "Shape"));
        assert!(!matches("Main TITLE", &content, "badge", "All layers"));
    }
}
