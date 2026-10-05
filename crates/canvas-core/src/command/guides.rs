use super::Command;
use crate::{CoreError, Document, Guide};

/// Cambia las guías como un solo paso reversible.
#[derive(Debug)]
pub struct SetGuides {
    pub before: Vec<Guide>,
    pub after: Vec<Guide>,
}
impl Command for SetGuides {
    fn label(&self) -> &str {
        "Edit guides"
    }
    fn apply(&mut self, doc: &mut Document) -> Result<(), CoreError> {
        doc.page_mut()?.guides.clone_from(&self.after);
        Ok(())
    }
    fn revert(&mut self, doc: &mut Document) -> Result<(), CoreError> {
        doc.page_mut()?.guides.clone_from(&self.before);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn guides_round_trip_and_undo() {
        let mut doc = Document::new(800.0, 600.0);
        let mut history = crate::History::default();
        let guides = vec![Guide {
            vertical: true,
            position: 125.5,
            locked: true,
        }];
        history
            .apply(
                &mut doc,
                Box::new(SetGuides {
                    before: vec![],
                    after: guides.clone(),
                }),
            )
            .unwrap();
        let restored: Document =
            serde_json::from_str(&serde_json::to_string(&doc).unwrap()).unwrap();
        assert_eq!(restored.page().unwrap().guides, guides);
        history.undo(&mut doc).unwrap();
        assert!(doc.page().unwrap().guides.is_empty());
        history.redo(&mut doc).unwrap();
        assert_eq!(doc.page().unwrap().guides, guides);
    }
    #[test]
    fn legacy_pages_are_still_readable() {
        let page: crate::Page =
            serde_json::from_str(r#"{"width":800,"height":600,"background":null,"layers":[]}"#)
                .unwrap();
        assert!(page.guides.is_empty());
    }
}
