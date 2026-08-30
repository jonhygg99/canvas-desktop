//! Tests de `resolve_canvas_sidecar` (mapeo de `.canvas` a su imagen
//! hermana), de `bake_came_out_blank` (la protección contra horneados en
//! blanco) y del aviso de poca RAM antes de «Save all». Hermano de
//! `persistence.rs` (convención `*_tests.rs`).

use std::path::{Path, PathBuf};

use canvas_core::{Document, History, ImageContent, LayerContent, LayerId, Selection, Transform};
use canvas_render::ImageMap;

use crate::deck::{Deck, DeckSeed, SeedItem, SlotContent, SlotDoc};
use crate::editor::EditorState;
use crate::gallery::ItemKind;
use crate::settings::GallerySort;

use super::{
    bake_came_out_blank, resolve_canvas_sidecar, save_all_doc_count, should_warn_low_memory,
    start_save_all_flow,
};

/// Documento de 100×100 con una capa de imagen visible 50×50.
fn doc_with_visible_image() -> Document {
    let mut doc = Document::new(100.0, 100.0);
    doc.add_layer(
        "img",
        Transform::new(0.0, 0.0, 50.0, 50.0),
        LayerContent::Image(ImageContent {
            source_path: None,
            natural_width: 4,
            natural_height: 2,
            crop: None,
        }),
    )
    .expect("añadir capa");
    doc
}

#[test]
fn resolves_a_sidecar_inside_the_dot_canvas_folder_to_its_image() {
    let dir = tempfile::tempdir().expect("tempdir");
    let image = dir.path().join("foto.png");
    std::fs::write(&image, b"x").unwrap();
    let sidecar_dir = dir.path().join(canvas_io::SIDECAR_DIR);
    std::fs::create_dir_all(&sidecar_dir).unwrap();
    let sidecar = sidecar_dir.join("foto.png.canvas");
    std::fs::write(&sidecar, b"{}").unwrap();

    assert_eq!(resolve_canvas_sidecar(sidecar), image);
}

#[test]
fn resolves_a_legacy_sibling_sidecar_to_its_image() {
    let dir = tempfile::tempdir().expect("tempdir");
    let image = dir.path().join("foto.png");
    std::fs::write(&image, b"x").unwrap();
    let sidecar = dir.path().join("foto.png.canvas");
    std::fs::write(&sidecar, b"{}").unwrap();

    assert_eq!(resolve_canvas_sidecar(sidecar), image);
}

#[test]
fn a_standalone_design_is_returned_as_is() {
    let dir = tempfile::tempdir().expect("tempdir");
    let design = dir.path().join("Untitled.canvas");
    std::fs::write(&design, b"{}").unwrap();

    assert_eq!(resolve_canvas_sidecar(design.clone()), design);
}

// ——— bake_came_out_blank: nunca escribir un horneado en blanco ———

#[test]
fn uniform_bake_with_visible_image_layers_is_blank() {
    // El caso 14.png: capas de imagen visibles, horneado blanco uniforme.
    let doc = doc_with_visible_image();
    let rgba = vec![255u8; 100 * 100 * 4];
    assert!(bake_came_out_blank(&doc, &rgba));
}

#[test]
fn uniform_transparent_bake_is_also_blank() {
    let doc = doc_with_visible_image();
    let rgba = vec![0u8; 100 * 100 * 4];
    assert!(bake_came_out_blank(&doc, &rgba));
}

#[test]
fn varied_bake_with_image_layers_is_not_blank() {
    // Un bake real con contenido: el primer píxel difiere del resto.
    let doc = doc_with_visible_image();
    let mut rgba = vec![255u8; 100 * 100 * 4];
    rgba[0..4].copy_from_slice(&[10, 20, 30, 255]);
    assert!(!bake_came_out_blank(&doc, &rgba));
}

#[test]
fn uniform_bake_without_image_layers_is_allowed() {
    // Diseño vectorial monocromo legítimo: no debe bloquearse.
    let doc = Document::new(100.0, 100.0);
    let rgba = vec![255u8; 100 * 100 * 4];
    assert!(!bake_came_out_blank(&doc, &rgba));
}

#[test]
fn uniform_bake_with_only_hidden_image_layers_is_allowed() {
    // La capa de imagen está oculta: el blanco uniforme puede ser legítimo.
    let mut doc = doc_with_visible_image();
    doc.layer_mut(doc.page().expect("página").layers.first().expect("capa").id)
        .expect("capa")
        .visible = false;
    let rgba = vec![255u8; 100 * 100 * 4];
    assert!(!bake_came_out_blank(&doc, &rgba));
}

// ——— Save All: aviso de poca RAM ———

fn seed(paths: &[&str]) -> DeckSeed {
    DeckSeed {
        folder: PathBuf::from(r"C:\folder"),
        sort: GallerySort::Name,
        items: paths
            .iter()
            .map(|p| SeedItem {
                path: PathBuf::from(p),
                name: p.to_string(),
                kind: ItemKind::Image,
                mtime: None,
                thumb: None,
                thumb_failed: false,
            })
            .collect(),
    }
}

/// `SlotDoc` de 10×10, limpio o sucio según `dirty` (el comando nunca se
/// aplica de verdad — `push_applied` no llama a `apply`).
fn slot_doc(dirty: bool) -> SlotDoc {
    let mut doc = SlotDoc {
        doc: Document::new(10.0, 10.0),
        history: History::default(),
        images: ImageMap::new(),
        selection: Selection::default(),
        background_layer: None,
        sidecar_enabled: true,
        is_design: false,
        source_metadata: None,
        saving: false,
        save_error: None,
        external_change: false,
        born_blank: false,
        pending_creation: false,
        bytes: 0,
    };
    if dirty {
        doc.history.push_applied(Box::new(canvas_core::Rename {
            layer: LayerId::from_raw(1),
            before: "a".to_string(),
            after: "b".to_string(),
        }));
    }
    doc
}

/// El aviso se dispara solo cuando hay medición de RAM libre y cae por
/// debajo del umbral (el mismo con el que la caché reduce su presupuesto).
#[test]
fn warns_only_when_measured_free_ram_is_below_the_threshold() {
    let threshold = crate::deck::FREE_RAM_REDUCTION_THRESHOLD_BYTES;
    assert!(!should_warn_low_memory(None), "sin medición no se avisa");
    assert!(should_warn_low_memory(Some(0)), "0 libres → avisa");
    assert!(
        should_warn_low_memory(Some(threshold - 1)),
        "justo por debajo del umbral → avisa"
    );
    assert!(
        !should_warn_low_memory(Some(threshold)),
        "en el umbral no se avisa"
    );
    assert!(
        !should_warn_low_memory(Some(threshold + 1)),
        "por encima no se avisa"
    );
}

/// «Save all» cuenta el activo sucio y las ranuras de fondo sucias; ni las
/// limpias ni las provisionales (aunque una provisional estuviera sucia —
/// no debería — no se escribe).
#[test]
fn save_all_count_adds_dirty_active_and_background_slots() {
    let mut deck = Deck::from_seed(seed(&["a.png", "b.png", "c.png"]), Path::new("a.png"));
    // b.png sucia de fondo; c.png limpia; y una provisional al final.
    deck.slots[1].content = SlotContent::Ready(Box::new(slot_doc(true)));
    deck.slots[2].content = SlotContent::Ready(Box::new(slot_doc(false)));
    let placeholder = deck
        .push_placeholder((10.0, 10.0), "png")
        .expect("con carpeta");
    deck.slots[placeholder].content = SlotContent::Ready(Box::new(slot_doc(true)));

    let mut state = EditorState::new_blank(100.0, 100.0);
    state.doc.source_path = Some(PathBuf::from(r"C:\folder\a.png"));
    state.history.push_applied(Box::new(canvas_core::Rename {
        layer: LayerId::from_raw(1),
        before: "a".to_string(),
        after: "b".to_string(),
    }));

    assert_eq!(
        save_all_doc_count(&deck, &state),
        2,
        "activa sucia + b sucia; ni c ni la provisional"
    );
}

/// El flujo real de «Save all» (lo que dispara el botón del modal): guarda
/// el activo sucio y encola las ranuras de fondo sucias por id estable,
/// dejando fuera limpias, provisionales y no sobrescribibles.
#[test]
fn save_all_flow_saves_the_active_doc_and_queues_dirty_background_slots() {
    use crate::app::SaveFlow;

    let mut deck = Deck::from_seed(
        seed(&["a.png", "b.png", "c.png", "d.svg"]),
        Path::new("a.png"),
    );
    let mut save = SaveFlow::default();
    // Activo (0) sucio y sobrescribible; b sucia; c limpia; d.svg sucia
    // pero no sobrescribible (SVG/GIF se dejan fuera).
    deck.slots[1].content = SlotContent::Ready(Box::new(slot_doc(true)));
    deck.slots[2].content = SlotContent::Ready(Box::new(slot_doc(false)));
    deck.slots[3].content = SlotContent::Ready(Box::new(slot_doc(true)));

    let mut state = EditorState::new_blank(100.0, 100.0);
    state.doc.source_path = Some(PathBuf::from(r"C:\folder\a.png"));
    state.history.push_applied(Box::new(canvas_core::Rename {
        layer: LayerId::from_raw(1),
        before: "a".to_string(),
        after: "b".to_string(),
    }));

    start_save_all_flow(&mut state, &mut deck, &mut save);

    assert!(state.save_clicked, "el activo sucio se guarda de inmediato");
    assert_eq!(
        save.save_all_queue,
        vec![deck.slots[1].id],
        "solo la ranura de fondo sucia y sobrescribible entra en la cola"
    );
    assert!(!save.save_all_attempted);
}

/// Un documento activo limpio (o sin ruta sobrescribible) no cuenta.
#[test]
fn save_all_count_skips_a_clean_or_non_overwritable_active_doc() {
    let deck = Deck::from_seed(seed(&["a.png"]), Path::new("a.png"));
    // Limpia y sin ruta: 0.
    let state = EditorState::new_blank(100.0, 100.0);
    assert_eq!(save_all_doc_count(&deck, &state), 0);

    // Sucia pero con un origen no sobrescribible (SVG/GIF): sigue en 0.
    let mut state = EditorState::new_blank(100.0, 100.0);
    state.doc.source_path = Some(PathBuf::from(r"C:\folder\a.svg"));
    state.history.push_applied(Box::new(canvas_core::Rename {
        layer: LayerId::from_raw(1),
        before: "a".to_string(),
        after: "b".to_string(),
    }));
    assert_eq!(save_all_doc_count(&deck, &state), 0);
}
