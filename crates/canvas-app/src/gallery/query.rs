//! Un único filtro compartido por cuadrícula, selección y teclado.
use super::{GalleryItem, GalleryState, ItemKind};
use crate::settings::MediaFilter;

impl GalleryState {
    pub(super) fn matches_item(&self, item: &GalleryItem) -> bool {
        let media = match self.media_filter {
            MediaFilter::All => true,
            MediaFilter::ImagesOnly => matches!(item.kind, ItemKind::Image | ItemKind::Design),
            MediaFilter::VideosOnly => item.kind == ItemKind::Video,
        };
        media
            && self.framings.matches(&item.path)
            && item
                .name
                .to_lowercase()
                .contains(&self.search.trim().to_lowercase())
    }

    pub(super) fn move_selection(&mut self, offset: isize) {
        let visible: Vec<_> = self
            .items
            .iter()
            .filter(|item| self.matches_item(item))
            .collect();
        if visible.is_empty() {
            self.selected = None;
            return;
        }
        let current = visible
            .iter()
            .position(|item| Some(&item.path) == self.selected.as_ref());
        let next = current.map_or(0, |index| {
            index.saturating_add_signed(offset).min(visible.len() - 1)
        });
        self.selected = Some(visible[next].path.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    fn gallery() -> GalleryState {
        let mut state = GalleryState::new(
            "missing".into(),
            crate::settings::GallerySort::Name,
            crate::deck::StripSide::Left,
        );
        state.merge_files(vec![
            (PathBuf::from("Photo.png"), None),
            (PathBuf::from("clip.mp4"), None),
        ]);
        state
    }
    #[test]
    fn search_is_case_insensitive_and_combines_with_media_filter() {
        let mut state = gallery();
        state.search = " PHOTO ".into();
        let photo = state
            .items
            .iter()
            .find(|item| item.name == "Photo.png")
            .unwrap();
        let clip = state
            .items
            .iter()
            .find(|item| item.name == "clip.mp4")
            .unwrap();
        assert!(state.matches_item(photo));
        assert!(!state.matches_item(clip));
        state.media_filter = MediaFilter::VideosOnly;
        assert!(!state.matches_item(
            state
                .items
                .iter()
                .find(|item| item.name == "Photo.png")
                .unwrap()
        ));
    }
    #[test]
    fn keyboard_selection_stays_inside_filtered_results() {
        let mut state = gallery();
        state.search = "clip".into();
        state.move_selection(1);
        assert_eq!(state.selected, Some(PathBuf::from("clip.mp4")));
        state.move_selection(-5);
        assert_eq!(state.selected, Some(PathBuf::from("clip.mp4")));
        state.search = "unmatched".into();
        state.move_selection(1);
        assert!(state.selected.is_none());
    }
}
