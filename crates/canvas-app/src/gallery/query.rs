//! Un único filtro compartido por cuadrícula, selección y teclado.
use super::{GalleryItem, GalleryState, ItemKind};
use crate::settings::MediaFilter;

impl GalleryState {
    pub(super) fn matches_item(&self, item: &GalleryItem) -> bool {
        self.matches_item_with(item, &self.search_needle())
    }

    /// Variante con la aguja ya calculada: los bucles calientes (cuadrícula
    /// y contadores, una vez por frame) la calculan una vez en vez de por
    /// elemento — en carpetas con miles de fotos se nota a 60 fps.
    pub(super) fn matches_item_with(&self, item: &GalleryItem, needle: &str) -> bool {
        let media = match self.media_filter {
            MediaFilter::All => true,
            MediaFilter::ImagesOnly => matches!(item.kind, ItemKind::Image | ItemKind::Design),
            MediaFilter::VideosOnly => item.kind == ItemKind::Video,
        };
        media && self.matches_search_and_framing_with(item, needle)
    }

    /// Aguja de búsqueda en minúsculas, una vez por pasada.
    pub(super) fn search_needle(&self) -> String {
        self.search.trim().to_lowercase()
    }

    /// Todo menos el filtro de medio (búsqueda + estado de framing): lo
    /// que los contadores de la cabecera describen.
    fn matches_search_and_framing_with(&self, item: &GalleryItem, needle: &str) -> bool {
        self.framings.matches(&item.path) && item.name.to_lowercase().contains(needle)
    }

    /// Contadores de la cabecera: total, imágenes (+diseños) y vídeos.
    /// Ignoran SOLO el filtro de medio —son botones que lo cambian— y
    /// respetan la búsqueda: con «solo vídeos» el grupo de imágenes sigue
    /// diciendo cuántas hay, no cero. Puro para probarlo sin egui.
    pub(super) fn visible_counts(&self) -> (usize, usize, usize) {
        let mut images = 0;
        let mut videos = 0;
        let needle = self.search_needle();
        for item in self
            .items
            .iter()
            .filter(|item| self.matches_search_and_framing_with(item, &needle))
        {
            if item.kind == ItemKind::Video {
                videos += 1;
            } else {
                images += 1;
            }
        }
        (images + videos, images, videos)
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
    fn visible_counts_split_images_and_videos() {
        let mut state = gallery();
        assert_eq!(state.visible_counts(), (2, 1, 1));
        // El filtro de medio no vacía los otros grupos: son sus botones.
        state.media_filter = MediaFilter::VideosOnly;
        assert_eq!(state.visible_counts(), (2, 1, 1));
        state.media_filter = MediaFilter::ImagesOnly;
        assert_eq!(state.visible_counts(), (2, 1, 1));
        // La búsqueda sí recorta todos los grupos por igual.
        state.search = "clip".into();
        assert_eq!(state.visible_counts(), (1, 0, 1));
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
