//! Fixture de un lienzo de clip para las tablas de `app/`: la usan
//! `clip_save_tests` (¿dónde se guarda?) y `autosave_tests` (¿cuándo se
//! guarda solo?). Vive aparte — compilado solo en `cfg(test)` — para que las
//! dos tablas no mantengan dos copias que se separan con el tiempo.

use std::path::{Path, PathBuf};

use crate::deck;
use crate::editor;

/// Lienzo de clip estándar: fondo desenfocado + capa de vídeo. `doc_source`
/// es el archivo del propio lienzo (lo que se escribe) y `layer_source` el
/// que lee ffmpeg; separarlos deja probar las dos mitades de la comprobación
/// de fuente de `clip_save_target`. Nace sucio (`mark_unsaved`): el
/// autoguardado necesita un cambio pendiente; la política de destino lo
/// ignora.
pub(super) fn clip_state(
    page: (f64, f64),
    doc_source: &Path,
    layer_source: &Path,
    trim: (f64, Option<f64>),
) -> editor::EditorState {
    use canvas_core::{ImageContent, LayerContent, Transform, VideoContent};
    let mut state = editor::EditorState::new_blank_image(page.0, page.1);
    state.doc.source_path = Some(doc_source.to_path_buf());
    let bg = state
        .doc
        .add_layer(
            "Blurred background",
            Transform::new(0.0, 0.0, page.0, page.1),
            LayerContent::Image(ImageContent {
                source_path: None,
                natural_width: 16,
                natural_height: 9,
                crop: None,
            }),
        )
        .unwrap();
    state.doc.layer_mut(bg).unwrap().effects.blur_radius = 20.0;
    state.background_layer = Some(bg);
    state
        .doc
        .add_layer(
            "Clip",
            Transform::new(0.0, 0.0, page.0, page.1),
            LayerContent::Video(VideoContent {
                source_path: Some(layer_source.to_path_buf()),
                natural_width: 540,
                natural_height: 960,
                crop: None,
                duration_secs: Some(20.0),
                poster_time: trim.0,
                trim_start: trim.0,
                trim_end: trim.1,
            }),
        )
        .unwrap();
    state.history.mark_unsaved();
    state
}

/// Lienzo de clip vertical con el mismo archivo como origen del documento y
/// de la capa: el caso normal.
pub(super) fn clip_canvas(source: &Path) -> editor::EditorState {
    clip_state((1080.0, 1920.0), source, source, (0.0, None))
}

/// Un archivo cualquiera con bytes: solo importa que exista.
pub(super) fn fake(path: &Path) -> PathBuf {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, b"x").unwrap();
    path.to_path_buf()
}

/// Baraja de proyecto con la ranura provisional del clip (nombre reservado).
pub(super) fn clip_deck(project: &Path) -> deck::Deck {
    let mut deck = deck::Deck::new_design((1080.0, 1920.0));
    deck.folder = Some(project.to_path_buf());
    deck.slots[0].path = project.join("clip-01.mp4");
    deck.slots[0].name = "clip-01.mp4".to_owned();
    deck.slots[0].is_placeholder = true;
    deck.slots[0].content = deck::SlotContent::Active;
    deck
}
