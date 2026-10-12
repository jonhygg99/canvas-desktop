//! Tests de `resolve_canvas_sidecar` (mapeo de `.canvas` a su imagen
//! hermana), de `bake_came_out_blank_or_incomplete` (la protección contra
//! horneados en blanco o incompletos) y del aviso de poca RAM antes de
//! «Save all». Hermano de `persistence.rs` (convención `*_tests.rs`).

use std::path::{Path, PathBuf};

#[test]
fn saved_exit_rechecks_changes_after_the_queue_drains() {
    let mut deck = Deck::from_seed(seed(&["a.png", "b.png"]), Path::new("b.png"));
    let mut state = EditorState::new_blank_image(10.0, 10.0);
    let mut save = crate::app::SaveFlow {
        close_after_save: true,
        after_save: Some(crate::app::Nav::CloseProject),
        ..Default::default()
    };
    deck.slots[0].content = SlotContent::Ready(Box::new(slot_doc(false)));
    start_save_all_flow(&mut state, &mut deck, &mut save);
    assert!(save.save_all_queue.is_empty());
    // A se vuelve a editar después de su guardado; B ya está limpio.
    let SlotContent::Ready(doc) = &mut deck.slots[0].content else {
        unreachable!()
    };
    doc.history.mark_unsaved();
    assert!(!saved_exit_ready(&mut state, &deck, &mut save));
    assert!(!save.close_after_save);
    assert!(save.after_save.is_none());
    assert!(!save.allow_close);
    assert!(state.save_error.is_some());
}

#[test]
fn saved_exit_allows_a_clean_session_but_waits_for_the_queue() {
    let deck = Deck::single("a.png".into());
    let mut state = EditorState::new_blank_image(10.0, 10.0);
    let mut save = crate::app::SaveFlow::default();
    save.save_all_queue.push(deck.slots[0].id);
    assert!(!saved_exit_ready(&mut state, &deck, &mut save));
    save.save_all_queue.clear();
    assert!(saved_exit_ready(&mut state, &deck, &mut save));
    state.history.mark_unsaved();
    assert!(!saved_exit_ready(&mut state, &deck, &mut save));
}

#[test]
fn saved_exit_keeps_an_uncommitted_edit_open() {
    let deck = Deck::single("a.png".into());
    let mut state = EditorState::new_blank_image(10.0, 10.0);
    state.doc = doc_with_visible_image();
    state.rename_edit = Some((LayerId::from_raw(1), "before".into(), "after".into()));
    let mut save = crate::app::SaveFlow {
        close_after_save: true,
        ..Default::default()
    };
    assert!(!state.is_dirty());
    assert!(!saved_exit_ready(&mut state, &deck, &mut save));
    assert!(!save.close_after_save);
}

#[test]
fn save_all_keeps_dirty_protected_formats_in_the_queue() {
    let mut deck = Deck::from_seed(
        seed(&["a.png", "b.svg", "c.gif", "d.mp4"]),
        Path::new("a.png"),
    );
    for slot in deck.slots.iter_mut().skip(1) {
        slot.content = SlotContent::Ready(Box::new(slot_doc(true)));
    }
    let mut state = EditorState::new_blank_image(10.0, 10.0);
    let mut save = crate::app::SaveFlow::default();
    start_save_all_flow(&mut state, &mut deck, &mut save);
    assert_eq!(
        save.save_all_queue,
        deck.slots
            .iter()
            .skip(1)
            .map(|slot| slot.id)
            .collect::<Vec<_>>()
    );
    assert_eq!(save_all_doc_count(&deck, &state), 3);
}

#[test]
fn save_all_keeps_all_edited_new_design_canvases_in_the_queue() {
    let mut deck = Deck::new_design((100.0, 100.0));
    let mut state = EditorState::new_blank(100.0, 100.0);
    state.add_image_layer(
        "Photo",
        None,
        canvas_io::LoadedImage {
            rgba: vec![255; 20 * 20 * 4],
            width: 20,
            height: 20,
        },
    );
    let copy = deck
        .duplicate_placeholder(deck.slots[0].id, &state, "canvas")
        .unwrap();
    deck.push_placeholder((100.0, 100.0), "canvas").unwrap();
    let mut save = super::super::SaveFlow::default();
    super::start_save_all_flow(&mut state, &mut deck, &mut save);
    assert_eq!(
        save.save_all_queue,
        vec![deck.slots[0].id, deck.slots[copy].id]
    );
    assert_eq!(super::save_all_doc_count(&deck, &state), 2);
}

use canvas_core::{Document, History, ImageContent, LayerContent, LayerId, Selection, Transform};
use canvas_render::ImageMap;

use crate::deck::{Deck, DeckSeed, SeedItem, SlotContent, SlotDoc};
use crate::editor::EditorState;
use crate::gallery::ItemKind;
use crate::settings::GallerySort;

use super::{
    bake_came_out_blank_or_incomplete, bakes_match, doc_needs_bake_verify, has_raster_layers,
    resolve_canvas_sidecar, save_all_doc_count, saved_exit_ready, should_warn_low_memory,
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

// ——— bake_came_out_blank_or_incomplete: nunca escribir un horneado en blanco o incompleto ———

#[test]
fn guard_rejects_blank_and_incomplete_bakes_by_table() {
    let with_img = doc_with_visible_image();
    let vector = Document::new(100.0, 100.0);
    let uniform = vec![255u8; 100 * 100 * 4];
    let mut varied = vec![255u8; 100 * 100 * 4];
    varied[0..4].copy_from_slice(&[10, 20, 30, 255]);

    // (doc, bake, capas omitidas) → ¿se rechaza el guardado?
    let cases: [(&Document, Vec<u8>, usize, bool); 5] = [
        // El caso 14.png: imágenes visibles, horneado blanco uniforme.
        (&with_img, uniform.clone(), 0, true),
        // Blanco uniforme y además capa omitida: ambas señales a la vez.
        (&with_img, uniform.clone(), 1, true),
        // NUEVO: el bake NO es uniforme, pero una capa visible se omitió al
        // construir la escena: el archivo resultante está incompleto.
        (&with_img, varied.clone(), 1, true),
        // Horneado completo y variado con imágenes visibles: permitido.
        (&with_img, varied.clone(), 0, false),
        // Diseño vectorial monocromo legítimo, 0 omitidas: permitido.
        (&vector, uniform.clone(), 0, false),
    ];
    for (i, (doc, rgba, skipped, rejected)) in cases.iter().enumerate() {
        assert_eq!(
            bake_came_out_blank_or_incomplete(doc, rgba, *skipped),
            *rejected,
            "caso {i}: skipped={skipped} → rechazado={rejected}"
        );
    }
}

#[test]
fn uniform_bake_with_only_hidden_image_layers_is_allowed() {
    // La capa de imagen está oculta: el blanco uniforme puede ser legítimo.
    let mut doc = doc_with_visible_image();
    doc.layer_mut(doc.page().expect("página").layers.first().expect("capa").id)
        .expect("capa")
        .visible = false;
    let rgba = vec![255u8; 100 * 100 * 4];
    assert!(!bake_came_out_blank_or_incomplete(&doc, &rgba, 0));
}

// ——— has_raster_layers: ¿el documento hornea algún píxel de imagen? ———

#[test]
fn raster_layers_presence_by_table() {
    // Imagen visible → sí.
    let img_visible = doc_with_visible_image();
    // Imagen oculta → no aporta píxeles al horneado → no.
    let img_hidden = {
        let mut doc = doc_with_visible_image();
        doc.layer_mut(doc.page().unwrap().layers[0].id)
            .unwrap()
            .visible = false;
        doc
    };
    // Solo texto/formas (p. ej. tras borrar la última foto) → no.
    let vector_only = Document::new(100.0, 100.0);
    // SVG visible cuenta como contenido que hornear → sí.
    let svg = {
        let mut doc = Document::new(100.0, 100.0);
        doc.add_layer(
            "svg",
            Transform::new(0.0, 0.0, 50.0, 50.0),
            LayerContent::Svg(canvas_core::SvgContent {
                source: "<svg/>".to_owned(),
                natural_width: 10,
                natural_height: 10,
            }),
        )
        .expect("añadir capa svg");
        doc
    };

    let cases = [
        (&img_visible, true),
        (&img_hidden, false),
        (&vector_only, false),
        (&svg, true),
    ];
    for (i, (doc, expected)) in cases.iter().enumerate() {
        assert_eq!(has_raster_layers(doc), *expected, "caso {i}");
    }
}

// ——— doc_needs_bake_verify + bakes_match: doble horneado solo con riesgo GPU ———

/// Documento de 100×100 con una capa de imagen visible de `nw×nh` naturales.
fn doc_with_sized_image(nw: u32, nh: u32) -> Document {
    let mut doc = Document::new(100.0, 100.0);
    doc.add_layer(
        "img",
        Transform::new(0.0, 0.0, 50.0, 50.0),
        LayerContent::Image(ImageContent {
            source_path: None,
            natural_width: nw,
            natural_height: nh,
            crop: None,
        }),
    )
    .expect("añadir capa");
    doc
}

#[test]
fn bake_verify_flag_by_table() {
    // Blur activo en foto pequeña → sí (textura de efectos en el atlas).
    let blur = {
        let mut doc = doc_with_sized_image(4, 2);
        doc.layer_mut(doc.page().unwrap().layers[0].id)
            .unwrap()
            .effects
            .blur_radius = 50.0;
        doc
    };
    // Ajuste de color sin blur → sí (también pasa por el pipeline GPU).
    let color = {
        let mut doc = doc_with_sized_image(4, 2);
        doc.layer_mut(doc.page().unwrap().layers[0].id)
            .unwrap()
            .effects
            .brightness = 0.5;
        doc
    };
    // Foto grande sin efectos → sí (puede desbordar el atlas).
    let big = doc_with_sized_image(2000, 2000);
    // Foto pequeña sin efectos → no (no hay nada que vello pueda soltar).
    let small = doc_with_sized_image(4, 2);
    // Capa de riesgo pero oculta → no.
    let hidden = {
        let mut doc = doc_with_sized_image(2000, 2000);
        doc.layer_mut(doc.page().unwrap().layers[0].id)
            .unwrap()
            .visible = false;
        doc
    };
    // Solo vector → no.
    let vector = Document::new(100.0, 100.0);

    let cases = [
        (&blur, true),
        (&color, true),
        (&big, true),
        (&small, false),
        (&hidden, false),
        (&vector, false),
    ];
    for (i, (doc, expected)) in cases.iter().enumerate() {
        assert_eq!(doc_needs_bake_verify(doc), *expected, "caso {i}");
    }
}

#[test]
fn bakes_match_needs_identical_bytes() {
    let a = vec![1u8, 2, 3, 4];
    assert!(bakes_match(&a, &a.clone()), "idénticos → sí");
    let mut one_off = a.clone();
    one_off[2] = 9;
    assert!(!bakes_match(&a, &one_off), "1 byte distinto → no");
    assert!(!bakes_match(&a, &[1, 2, 3]), "longitud distinta → no");
    assert!(bakes_match(&[], &[]), "vacíos → sí");
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
        saving_capture: None,
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

/// «Save all» cuenta cada documento sucio, incluso una provisional editada.
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
        3,
        "activa sucia + b sucia + provisional sucia; c está limpia"
    );
}

/// El flujo real de «Save all» (lo que dispara el botón del modal): guarda
/// el activo sucio y encola las ranuras de fondo sucias por id estable,
/// dejando fuera solo las limpias; los formatos protegidos piden Guardar como.
#[test]
fn save_all_flow_saves_the_active_doc_and_queues_dirty_background_slots() {
    use crate::app::SaveFlow;

    let mut deck = Deck::from_seed(
        seed(&["a.png", "b.png", "c.png", "d.svg"]),
        Path::new("a.png"),
    );
    let mut save = SaveFlow::default();
    // Activo (0) sucio y sobrescribible; b sucia; c limpia; d.svg sucia
    // pero no sobrescribible: necesita Guardar como.
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
        vec![deck.slots[1].id, deck.slots[3].id],
        "las dos ranuras sucias entran en la cola"
    );
    assert!(!save.save_all_attempted);
}

/// Un documento limpio no cuenta; un origen protegido sí requiere guardar.
#[test]
fn save_all_count_skips_clean_but_includes_protected_active_doc() {
    let deck = Deck::from_seed(seed(&["a.png"]), Path::new("a.png"));
    // Limpia y sin ruta: 0.
    let state = EditorState::new_blank(100.0, 100.0);
    assert_eq!(save_all_doc_count(&deck, &state), 0);

    // Sucia con un origen no sobrescribible: necesita Guardar como.
    let mut state = EditorState::new_blank(100.0, 100.0);
    state.doc.source_path = Some(PathBuf::from(r"C:\folder\a.svg"));
    state.history.push_applied(Box::new(canvas_core::Rename {
        layer: LayerId::from_raw(1),
        before: "a".to_string(),
        after: "b".to_string(),
    }));
    assert_eq!(save_all_doc_count(&deck, &state), 1);
}

/// Un vídeo (o una extensión sin codificador) se rechaza ANTES de hornear:
/// el informe era un `UnsupportedFormat` tras todo el trabajo de GPU.
#[test]
fn raster_save_refuses_video_before_baking() {
    use super::raster_target_error;
    let error =
        raster_target_error(Path::new(r"G:\vids\1.mp4")).expect("el mp4 no es destino raster");
    assert!(
        error.to_string().contains("not a format"),
        "mismo mensaje que el worker, pero sin hornear: {error}"
    );
    assert!(raster_target_error(Path::new("foto.png")).is_none());
    assert!(raster_target_error(Path::new("foto.jpg")).is_none());
    assert!(raster_target_error(Path::new("sin-extension")).is_some());
}
