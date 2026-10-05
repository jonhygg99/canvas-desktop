//! Duplicación en memoria de documentos que aún no existen en disco.

use crate::editor::EditorState;

use super::{Deck, SlotContent};

impl Deck {
    pub fn duplicate_placeholder(
        &mut self,
        id: u64,
        active: &EditorState,
        ext: &str,
    ) -> Option<usize> {
        let source = self.slots.get(self.find_by_id(id)?)?;
        if !source.is_placeholder {
            return None;
        }
        let mut copy = EditorState::new_blank(1.0, 1.0);
        match &source.content {
            SlotContent::Active => {
                copy.doc = active.doc.clone();
                copy.images = active.images.clone();
                copy.selection = active.selection.clone();
                copy.background_layer = active.background_layer;
                copy.sidecar_enabled = active.sidecar_enabled;
                copy.is_design = active.is_design;
                copy.source_metadata = active.source_metadata.clone();
            }
            SlotContent::Ready(doc) => {
                copy.doc = doc.doc.clone();
                copy.images = doc.images.clone();
                copy.selection = doc.selection.clone();
                copy.background_layer = doc.background_layer;
                copy.sidecar_enabled = doc.sidecar_enabled;
                copy.is_design = doc.is_design;
                copy.source_metadata = doc.source_metadata.clone();
            }
            _ => return None,
        }
        copy.doc.source_path = None;
        copy.history.mark_unsaved();
        // La creación se registra al duplicar; el historial de edición es nuevo.
        copy.pending_creation = false;
        let page = copy.doc.page().ok()?;
        let index = self.push_placeholder((page.width, page.height), ext)?;
        self.slots[index].content = SlotContent::Ready(Box::new(copy.take_slot()));
        Some(index)
    }
}
