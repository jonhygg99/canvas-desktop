//! Acciones de la paleta: reutilizan las operaciones y guardas del editor.
use super::{
    layer_ops,
    selection_layout::{self, Layout},
    EditorState,
};
use canvas_core::LayerContent;

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Action {
    Undo,
    Redo,
    Duplicate,
    Repeat,
    Delete,
    Group,
    Ungroup,
    SelectAll,
    EditText,
    Fit,
    ZoomIn,
    ZoomOut,
    ActualSize,
    Rulers,
    Align(bool, f64),
    Distribute(bool),
}
pub(super) const ACTIONS: &[Action] = &[
    Action::Undo,
    Action::Redo,
    Action::Duplicate,
    Action::Repeat,
    Action::Delete,
    Action::Group,
    Action::Ungroup,
    Action::SelectAll,
    Action::EditText,
    Action::Fit,
    Action::ZoomIn,
    Action::ZoomOut,
    Action::ActualSize,
    Action::Rulers,
    Action::Align(false, 0.0),
    Action::Align(false, 0.5),
    Action::Align(false, 1.0),
    Action::Align(true, 0.0),
    Action::Align(true, 0.5),
    Action::Align(true, 1.0),
    Action::Distribute(false),
    Action::Distribute(true),
];
impl Action {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Undo => "Undo",
            Self::Redo => "Redo",
            Self::Duplicate => "Duplicate selection",
            Self::Repeat => "Repeat duplication",
            Self::Delete => "Delete",
            Self::Group => "Group layers",
            Self::Ungroup => "Ungroup layers",
            Self::SelectAll => "Select all",
            Self::EditText => "Edit text",
            Self::Fit => "Fit page",
            Self::ZoomIn => "Zoom in",
            Self::ZoomOut => "Zoom out",
            Self::ActualSize => "Actual size",
            Self::Rulers => "Show rulers",
            Self::Align(false, 0.0) => "Left",
            Self::Align(false, 0.5) => "Center",
            Self::Align(false, _) => "Right",
            Self::Align(true, 0.0) => "Top",
            Self::Align(true, 0.5) => "Middle",
            Self::Align(true, _) => "Bottom",
            Self::Distribute(false) => "Distribute horizontally",
            Self::Distribute(true) => "Distribute vertically",
        }
    }
    pub(super) fn shortcut(self) -> &'static str {
        match self {
            Self::Undo => "+Z",
            Self::Redo => "+Shift+Z",
            Self::Duplicate => "+D",
            Self::Repeat => "+Shift+D",
            Self::Group => "+G",
            Self::Ungroup => "+Shift+G",
            Self::SelectAll => "+A",
            Self::Fit => "+0",
            _ => "",
        }
    }
    pub(super) fn enabled(self, state: &EditorState) -> bool {
        if !state.is_idle()
            || state.framing.is_some()
            || state.pending_global_undo.is_some()
            || state.pending_global_redo.is_some()
        {
            return false;
        }
        let editable = !layer_ops::selection_transforms(state).is_empty();
        match self {
            Self::Undo => state.can_undo(),
            Self::Redo => state.can_redo(),
            Self::Delete => layer_ops::has_deletable_selection(state),
            Self::Group => editable && state.selection.len() > 1,
            Self::Ungroup => state.doc.page().is_ok_and(|p| {
                state
                    .selection
                    .ids()
                    .iter()
                    .any(|id| p.is_group(*id) && !p.effective_locked(*id))
            }),
            Self::EditText => {
                editable
                    && state.selection.len() == 1
                    && state.selection.primary().is_some_and(|id| {
                        state
                            .doc
                            .layer(id)
                            .is_ok_and(|l| matches!(l.content, LayerContent::Text(_)))
                    })
            }
            Self::Duplicate | Self::Repeat | Self::Align(..) => editable,
            Self::Distribute(_) => editable && state.selection.len() >= 3,
            _ => true,
        }
    }
    pub(super) fn execute(self, state: &mut EditorState) {
        if !self.enabled(state) {
            return;
        }
        match self {
            Self::Undo => state.undo(),
            Self::Redo => state.redo(),
            Self::Duplicate => super::duplicate_gesture::duplicate(state, false),
            Self::Repeat => super::duplicate_gesture::duplicate(state, true),
            Self::Delete => layer_ops::delete_selected(state),
            Self::Group => crate::layers_panel::group_selection(state),
            Self::Ungroup => crate::layers_panel::ungroup_selection(state),
            Self::SelectAll => crate::clipboard::select_all(state),
            Self::EditText => {
                if let Some(id) = state.selection.primary() {
                    super::inline_text::begin(state, id);
                }
            }
            Self::Fit => state.viewport.request_fit(),
            Self::ZoomIn => state.pending_zoom_factor = Some(1.25),
            Self::ZoomOut => state.pending_zoom_factor = Some(0.8),
            Self::ActualSize => state.pending_zoom_factor = Some(1.0 / state.viewport.zoom),
            Self::Rulers => state.show_rulers = !state.show_rulers,
            Self::Align(vertical, fraction) => {
                selection_layout::apply(state, Layout::Align { vertical, fraction })
            }
            Self::Distribute(vertical) => {
                selection_layout::apply(state, Layout::Distribute { vertical })
            }
        }
    }
}
