//! Saltos locales que respetan el orden global de ediciones entre diseños.
use super::{EditorState, GlobalStep};

impl EditorState {
    pub(crate) fn history_range(&self) -> std::ops::RangeInclusive<usize> {
        let current = self.history.undo_depth();
        let suffix = |steps: &[GlobalStep]| {
            steps
                .iter()
                .rev()
                .take_while(|step| **step == GlobalStep::Edit(self.active_slot_id))
                .count()
        };
        current.saturating_sub(suffix(&self.global_undo))..=current + suffix(&self.global_redo)
    }
    pub(crate) fn jump_history(&mut self, target: usize) {
        if !self.is_idle()
            || self.framing.is_some()
            || !self.history_range().contains(&target)
            || self.pending_global_undo.is_some()
            || self.pending_global_redo.is_some()
        {
            return;
        }
        while self.history.undo_depth() != target {
            let before = self.history.undo_depth();
            let undoing = before > target;
            let opposite_depth = if undoing {
                self.global_redo.len()
            } else {
                self.global_undo.len()
            };
            if before > target {
                self.undo();
            } else {
                self.redo();
            }
            let after = if undoing {
                self.global_redo.len()
            } else {
                self.global_undo.len()
            };
            if self.history.undo_depth() == before || after != opposite_depth + 1 {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use canvas_core::SetPageSize;
    fn resize(state: &mut EditorState, width: f64) {
        let before = state.doc.page().unwrap().width;
        state
            .apply_undo_step(Box::new(SetPageSize {
                before: (before, 500.0),
                after: (width, 500.0),
            }))
            .unwrap();
    }
    #[test]
    fn jumping_keeps_global_undo_and_redo_in_sync() {
        let mut state = EditorState::new_blank(600.0, 500.0);
        for width in [700.0, 800.0, 900.0] {
            resize(&mut state, width);
        }
        let original_steps = state.global_undo.clone();
        state.jump_history(1);
        assert_eq!(state.doc.page().unwrap().width, 700.0);
        assert_eq!(state.global_redo.len(), 2);
        state.jump_history(3);
        assert_eq!(state.doc.page().unwrap().width, 900.0);
        assert_eq!(state.global_undo, original_steps);
        assert!(state.history.redo_labels().next().is_none());
    }
    #[test]
    fn jumping_cannot_cross_a_foreign_design_edit() {
        let mut state = EditorState::new_blank(600.0, 500.0);
        resize(&mut state, 700.0);
        state.global_undo.push(GlobalStep::Edit(99));
        resize(&mut state, 800.0);
        assert_eq!(state.history_range(), 1..=2);
        state.jump_history(0);
        assert_eq!(state.doc.page().unwrap().width, 800.0);
        assert!(state.pending_global_undo.is_none());
        state.jump_history(1);
        assert_eq!(state.doc.page().unwrap().width, 700.0);
    }
    #[test]
    fn previous_save_error_does_not_stop_a_valid_history_jump() {
        let mut state = EditorState::new_blank(600.0, 500.0);
        for width in [700.0, 800.0, 900.0] {
            resize(&mut state, width);
        }
        state.save_error = Some("Previous save failed".into());
        state.jump_history(0);
        assert_eq!(state.doc.page().unwrap().width, 600.0);
        assert_eq!(state.history.undo_depth(), 0);
    }
}
