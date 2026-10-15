//! Tests de `ImageSource`: la fuente por defecto, el juego completo del
//! selector (sin repetidos) y el mapeo a los modos de Serper — Unsplash no
//! es un modo de Serper, es «no lo toques».

use super::*;
use crate::serper::SearchMode;

#[test]
fn default_source_is_google_web_images() {
    assert_eq!(ImageSource::default(), ImageSource::Web);
}

#[test]
fn every_source_appears_exactly_once_in_the_selector() {
    assert_eq!(ImageSource::ALL.len(), 3, "el selector tiene tres fuentes");
    for (i, source) in ImageSource::ALL.iter().enumerate() {
        assert!(
            !ImageSource::ALL[..i].contains(source),
            "{source:?} aparece repetida en ALL"
        );
    }
}

#[test]
fn only_the_serper_sources_map_to_a_search_mode() {
    assert_eq!(ImageSource::Web.serper_mode(), Some(SearchMode::Web));
    assert_eq!(ImageSource::Social.serper_mode(), Some(SearchMode::Social));
    assert_eq!(
        ImageSource::Unsplash.serper_mode(),
        None,
        "Unsplash no toca el estado de Serper"
    );
}
