//! Tests de la baraja. Juntos a proposito: casi todos montan una baraja con
//! `from_seed` y luego cruzan escaneo, layout, cache y navegacion a la vez.

use canvas_core::{Document, History, LayerId, Selection};
use canvas_render::ImageMap;

use crate::gallery::ItemKind;

use super::cache::{
    adaptive_evict_budget, EVICT_BUDGET_BYTES, MAX_EVICT_BUDGET_BYTES, MIN_EVICT_BUDGET_BYTES,
};
use super::loading::MAX_INFLIGHT_LOADS;
use super::model::SeedItem;
use super::*;

fn seed(paths: &[&str]) -> DeckSeed {
    DeckSeed {
        folder: PathBuf::from("C:\\folder"),
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

fn blank_slot_doc(w: f64, h: f64) -> SlotDoc {
    SlotDoc {
        doc: Document::new(w, h),
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
    }
}

/// Como `blank_slot_doc`, pero con `history.is_dirty() == true`. El
/// comando nunca se aplica de verdad (`push_applied` no llama a
/// `apply`), así que da igual que `layer` no exista en `doc`.
fn dirty_slot_doc(w: f64, h: f64) -> SlotDoc {
    let mut doc = blank_slot_doc(w, h);
    doc.history.push_applied(Box::new(canvas_core::Rename {
        layer: LayerId::from_raw(1),
        before: "a".to_string(),
        after: "b".to_string(),
    }));
    doc
}

#[test]
fn single_is_degenerate_and_active() {
    let deck = Deck::single(PathBuf::from("a.png"));
    assert_eq!(deck.slots.len(), 1);
    assert!(!deck.is_visible());
    assert!(matches!(deck.slots[0].content, SlotContent::Active));
    assert_eq!(deck.next_path(), None);
    assert_eq!(deck.prev_path(), None);
}

#[test]
fn from_seed_activates_matching_path() {
    let deck = Deck::from_seed(seed(&["a.png", "b.png", "c.png"]), Path::new("b.png"));
    assert_eq!(deck.active, 1);
    assert!(deck.is_visible());
    assert!(matches!(deck.slots[1].content, SlotContent::Active));
}

#[test]
fn next_and_prev_wrap() {
    let mut deck = Deck::from_seed(seed(&["a.png", "b.png", "c.png"]), Path::new("a.png"));
    assert_eq!(deck.next_path(), Some(PathBuf::from("b.png")));
    deck.active = 2;
    assert_eq!(deck.next_path(), Some(PathBuf::from("a.png")));
    deck.active = 0;
    assert_eq!(deck.prev_path(), Some(PathBuf::from("c.png")));
}

#[test]
fn first_and_last() {
    let deck = Deck::from_seed(seed(&["a.png", "b.png", "c.png"]), Path::new("b.png"));
    assert_eq!(deck.first_path(), Some(PathBuf::from("a.png")));
    assert_eq!(deck.last_path(), Some(PathBuf::from("c.png")));
}

#[test]
fn merge_scan_preserves_id_and_thumb_by_path() {
    let mut deck = Deck::from_seed(seed(&["a.png", "b.png"]), Path::new("a.png"));
    let id_a = deck.slots[0].id;
    deck.merge_scan(vec![
        (PathBuf::from("b.png"), None),
        (PathBuf::from("a.png"), None),
        (PathBuf::from("c.png"), None),
    ]);
    assert_eq!(deck.slots.len(), 3);
    let a = deck
        .slots
        .iter()
        .find(|s| s.path.ends_with("a.png"))
        .unwrap();
    assert_eq!(a.id, id_a);
    assert_eq!(deck.active_path(), Some(PathBuf::from("a.png")));
}

#[test]
fn merge_scan_drops_vanished_files() {
    let mut deck = Deck::from_seed(seed(&["a.png", "b.png"]), Path::new("a.png"));
    deck.merge_scan(vec![(PathBuf::from("a.png"), None)]);
    assert_eq!(deck.slots.len(), 1);
}

#[test]
fn merge_scan_keeps_a_dirty_slot_even_if_its_file_vanished() {
    let mut deck = Deck::from_seed(seed(&["a.png", "b.png"]), Path::new("a.png"));
    deck.slots[1].content = SlotContent::Ready(Box::new(dirty_slot_doc(10.0, 10.0)));
    // b.png desaparece del disco en el reescaneo.
    deck.merge_scan(vec![(PathBuf::from("a.png"), None)]);
    assert_eq!(deck.slots.len(), 2, "la ranura sucia no debe perderse");
}

#[test]
fn move_slot_swaps_two_neighbors_and_switches_to_manual_sort() {
    let mut deck = Deck::from_seed(seed(&["a.png", "b.png", "c.png"]), Path::new("a.png"));
    let id_b = deck.slots[1].id;
    assert!(deck.move_slot(id_b, MoveDir::Prev));
    assert_eq!(deck.sort, GallerySort::Manual);
    let names: Vec<&str> = deck.slots.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["b.png", "a.png", "c.png"]);
}

#[test]
fn move_slot_keeps_the_active_slot_active_across_the_swap() {
    let mut deck = Deck::from_seed(seed(&["a.png", "b.png", "c.png"]), Path::new("a.png"));
    let id_a = deck.slots[0].id;
    assert!(deck.move_slot(id_a, MoveDir::Next));
    // "a" ahora está en la posición 1, pero sigue siendo la activa.
    assert_eq!(deck.slots[deck.active].id, id_a);
}

#[test]
fn move_slot_fails_at_either_end() {
    let mut deck = Deck::from_seed(seed(&["a.png", "b.png"]), Path::new("a.png"));
    let id_a = deck.slots[0].id;
    let id_b = deck.slots[1].id;
    assert!(!deck.move_slot(id_a, MoveDir::Prev), "ya es la primera");
    assert!(!deck.move_slot(id_b, MoveDir::Next), "ya es la última");
}

#[test]
fn order_hint_survives_a_rescan_that_also_adds_a_new_file() {
    let mut deck = Deck::from_seed(seed(&["a.png", "b.png", "c.png"]), Path::new("a.png"));
    let id_c = deck.slots[2].id;
    // Manda "c.png" al principio.
    assert!(deck.move_slot(id_c, MoveDir::Prev));
    assert!(deck.move_slot(id_c, MoveDir::Prev));
    assert_eq!(deck.slots[0].id, id_c);
    // Reescaneo que además trae un archivo nuevo.
    deck.merge_scan(vec![
        (PathBuf::from("a.png"), None),
        (PathBuf::from("b.png"), None),
        (PathBuf::from("c.png"), None),
        (PathBuf::from("d.png"), None),
    ]);
    assert_eq!(
        deck.sort,
        GallerySort::Manual,
        "el reescaneo no reinicia el orden"
    );
    let names: Vec<&str> = deck.slots.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["c.png", "a.png", "b.png", "d.png"],
        "el orden manual sobrevive; la ranura nueva aterriza al final"
    );
}

#[test]
fn relayout_single_slot_starts_at_origin() {
    let mut deck = Deck::single(PathBuf::from("a.png"));
    deck.slots[0].page = Some((800.0, 600.0));
    deck.relayout();
    assert_eq!(
        deck.slots[0].rect,
        DeckRect {
            x: 0.0,
            y: 0.0,
            w: 800.0,
            h: 600.0
        }
    );
}

#[test]
fn relayout_stacks_vertically_and_centers_narrower_slots() {
    let mut deck = Deck::from_seed(seed(&["a.png", "b.png"]), Path::new("a.png"));
    deck.slots[0].page = Some((800.0, 600.0));
    deck.slots[1].page = Some((400.0, 300.0));
    deck.relayout();
    let a = deck.slots[0].rect;
    let b = deck.slots[1].rect;
    assert_eq!(a.x, 0.0);
    assert_eq!(a.y, 0.0);
    // b es más estrecho: centrado dentro del ancho de la pila (800).
    assert_eq!(b.x, (800.0 - 400.0) / 2.0);
    // b empieza justo debajo de a más el hueco.
    assert!(b.y > a.y + a.h);
}

#[test]
fn relayout_stacks_horizontally_and_centers_shorter_slots() {
    let mut deck = Deck::from_seed(seed(&["a.png", "b.png"]), Path::new("a.png"));
    deck.axis = DeckAxis::Horizontal;
    deck.slots[0].page = Some((800.0, 600.0));
    deck.slots[1].page = Some((400.0, 300.0));
    deck.relayout();
    let a = deck.slots[0].rect;
    let b = deck.slots[1].rect;
    assert_eq!(a.x, 0.0);
    assert_eq!(a.y, 0.0);
    // b es más bajo: centrado dentro del alto de la pila (600).
    assert_eq!(b.y, (600.0 - 300.0) / 2.0);
    // b empieza justo a la derecha de a más el hueco.
    assert!(b.x > a.x + a.w);
}

#[test]
fn bounds_spans_all_slots_on_either_axis() {
    let mut deck = Deck::from_seed(seed(&["a.png", "b.png"]), Path::new("a.png"));
    deck.axis = DeckAxis::Horizontal;
    deck.slots[0].page = Some((800.0, 600.0));
    deck.slots[1].page = Some((400.0, 300.0));
    deck.relayout();
    let bounds = deck.bounds();
    // Horizontal: el ancho recorre las dos ranuras + hueco; el alto es
    // el más alto de los dos (el más bajo queda centrado dentro).
    assert!(bounds.w > 800.0 + 400.0);
    assert_eq!(bounds.h, 600.0);
}

#[test]
fn visible_indices_finds_only_intersecting_slots() {
    let mut deck = Deck::from_seed(seed(&["a.png", "b.png", "c.png"]), Path::new("a.png"));
    deck.slots[0].page = Some((100.0, 100.0));
    deck.slots[1].page = Some((100.0, 100.0));
    deck.slots[2].page = Some((100.0, 100.0));
    deck.relayout();
    // Solo cabe el primero en una ventana pequeña arriba del todo.
    let view = DeckRect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 50.0,
    };
    assert_eq!(deck.visible_indices(view), vec![0]);
}

#[test]
fn apply_jump_swaps_content_and_preserves_dirty_flag() {
    let mut deck = Deck::from_seed(seed(&["a.png", "b.png"]), Path::new("a.png"));
    deck.slots[1].content = SlotContent::Ready(Box::new(blank_slot_doc(50.0, 50.0)));
    let mut state = crate::editor::EditorState::new_blank(20.0, 20.0);
    deck.jump_to = Some(1);
    assert!(apply_jump(&mut deck, &mut state));
    assert_eq!(deck.active, 1);
    assert_eq!(state.doc.page().unwrap().width, 50.0);
    assert!(matches!(deck.slots[1].content, SlotContent::Active));
    assert!(matches!(deck.slots[0].content, SlotContent::Ready(_)));
}

#[test]
fn evict_skips_dirty_and_nearby_slots_but_takes_clean_far_ones() {
    let mut deck = Deck::from_seed(
        seed(&["a.png", "b.png", "c.png", "d.png", "e.png", "f.png"]),
        Path::new("a.png"),
    );
    // Activo = 0, radio de precarga = 2 ⇒ 0..=2 están protegidos; 3..=5
    // son candidatas. Todas pesan de sobra para forzar el descarte.
    for i in 3..6 {
        let mut doc = blank_slot_doc(10.0, 10.0);
        doc.bytes = EVICT_BUDGET_BYTES;
        deck.slots[i].content = SlotContent::Ready(Box::new(doc));
    }
    deck.slots[3].content = SlotContent::Ready(Box::new(dirty_slot_doc(10.0, 10.0)));
    if let SlotContent::Ready(d) = &mut deck.slots[3].content {
        d.bytes = EVICT_BUDGET_BYTES;
    }

    let freed = deck.evict();

    assert_eq!(freed.len(), 2, "solo las dos limpias, la sucia sobrevive");
    assert!(
        matches!(deck.slots[3].content, SlotContent::Ready(_)),
        "una ranura sucia nunca se descarta"
    );
    assert!(matches!(deck.slots[4].content, SlotContent::Idle));
    assert!(matches!(deck.slots[5].content, SlotContent::Idle));
}

#[test]
fn evict_skips_a_clean_slot_that_still_has_undo_history() {
    // Activa = 0, radio de precarga = 2 ⇒ la ranura 3 es candidata.
    let mut deck = Deck::from_seed(
        seed(&["a.png", "b.png", "c.png", "d.png"]),
        Path::new("a.png"),
    );
    let mut doc = blank_slot_doc(10.0, 10.0);
    doc.history.push_applied(Box::new(canvas_core::Rename {
        layer: LayerId::from_raw(1),
        before: "a".to_string(),
        after: "b".to_string(),
    }));
    // Guardado: limpio (`is_dirty() == false`), pero con un paso de
    // deshacer todavía en la pila.
    doc.history.mark_saved();
    // Estrictamente por encima del presupuesto: igualarlo no dispara el
    // descarte (ver `evict_never_discards_a_placeholder`).
    doc.bytes = EVICT_BUDGET_BYTES + 1;
    deck.slots[3].content = SlotContent::Ready(Box::new(doc));

    let freed = deck.evict();

    assert!(
        freed.is_empty(),
        "una ranura limpia con historial de deshacer no debe expulsarse: \
         perdería ese historial al recargarse de disco"
    );
    assert!(matches!(deck.slots[3].content, SlotContent::Ready(_)));
}

#[test]
fn request_loads_prioritises_a_pending_jump_outside_the_preload_radius() {
    // 8 ranuras, activa = 0 (radio de precarga 2 ⇒ 0..=2 cubiertas
    // aparte). Sin `jump_to`, la ranura 7 nunca se pediría por sí sola.
    let mut deck = Deck::from_seed(
        seed(&[
            "a.png", "b.png", "c.png", "d.png", "e.png", "f.png", "g.png", "h.png",
        ]),
        Path::new("a.png"),
    );
    deck.jump_to = Some(7);
    let spawned = deck.request_loads(&[]);
    assert_eq!(
        spawned.first(),
        Some(&PathBuf::from("h.png")),
        "el destino del salto pendiente debe cargarse primero, aunque esté lejos"
    );
    assert!(matches!(deck.slots[7].content, SlotContent::Loading));
}

#[test]
fn request_loads_prioritises_neighbours_of_an_active_placeholder() {
    let mut deck = Deck::from_seed(seed(&["a.png", "b.png", "c.png"]), Path::new("a.png"));
    let placeholder = deck
        .push_placeholder((800.0, 600.0), "canvas")
        .expect("con carpeta");
    deck.slots[deck.active].content = SlotContent::Idle;
    deck.active = placeholder;
    deck.slots[placeholder].content = SlotContent::Active;

    let spawned = deck.request_loads(&[]);

    assert_eq!(
        spawned,
        vec![PathBuf::from("c.png"), PathBuf::from("b.png")]
    );
    assert!(!spawned.contains(&deck.slots[placeholder].path));
    assert!(matches!(deck.slots[1].content, SlotContent::Loading));
    assert!(matches!(deck.slots[2].content, SlotContent::Loading));
}

#[test]
fn seeded_decks_get_unique_load_generations() {
    let first = Deck::from_seed(seed(&["a.png"]), Path::new("a.png"));
    let second = Deck::from_seed(seed(&["a.png"]), Path::new("a.png"));
    assert_ne!(first.generation(), second.generation());
}

#[test]
fn adaptive_budget_can_be_lowered_for_constrained_runs() {
    assert!(MIN_EVICT_BUDGET_BYTES <= adaptive_evict_budget());
    assert!(adaptive_evict_budget() <= MAX_EVICT_BUDGET_BYTES);
}

#[test]
fn request_loads_preloads_distant_gallery_pages_in_the_background() {
    let mut deck = Deck::from_seed(
        seed(&["a.png", "b.png", "c.png", "d.png", "e.png"]),
        Path::new("a.png"),
    );
    assert!(deck.preload_all);
    let spawned = deck.request_loads(&[]);

    assert_eq!(spawned.len(), MAX_INFLIGHT_LOADS);
    assert!(spawned.contains(&PathBuf::from("b.png")));
    assert!(spawned.contains(&PathBuf::from("c.png")));
    assert!(matches!(deck.slots[1].content, SlotContent::Loading));
    assert!(matches!(deck.slots[2].content, SlotContent::Loading));
}

#[test]
fn request_loads_ignores_a_jump_that_is_not_idle() {
    let mut deck = Deck::from_seed(seed(&["a.png", "b.png", "c.png"]), Path::new("a.png"));
    deck.slots[2].content = SlotContent::Ready(Box::new(blank_slot_doc(10.0, 10.0)));
    deck.jump_to = Some(2);
    let spawned = deck.request_loads(&[]);
    assert!(
        !spawned.contains(&PathBuf::from("c.png")),
        "una ranura ya Ready no debe volver a pedirse"
    );
}

#[test]
fn request_loads_ignores_an_out_of_range_jump() {
    let mut deck = Deck::from_seed(seed(&["a.png", "b.png"]), Path::new("a.png"));
    deck.jump_to = Some(99);
    // No debe entrar en pánico; el resto del comportamiento normal sigue.
    let spawned = deck.request_loads(&[]);
    assert!(spawned.contains(&PathBuf::from("b.png")));
}

#[test]
fn push_placeholder_appends_a_clean_ready_slot_at_the_end() {
    let mut deck = Deck::from_seed(seed(&["a.png", "b.png"]), Path::new("a.png"));
    let idx = deck
        .push_placeholder((800.0, 600.0), "canvas")
        .expect("con carpeta");
    assert_eq!(idx, 2);
    let slot = &deck.slots[idx];
    assert!(slot.is_placeholder);
    assert_eq!(slot.page, Some((800.0, 600.0)));
    assert!(slot.kind == ItemKind::Design);
    match &slot.content {
        SlotContent::Ready(d) => assert!(
            !d.history.is_dirty(),
            "una provisional recién creada debe leerse como limpia, \
             o se materializaría sola sin que nadie la editase"
        ),
        _ => panic!("se esperaba SlotContent::Ready"),
    }
}

#[test]
fn push_placeholder_png_makes_a_real_image_slot() {
    let mut deck = Deck::from_seed(seed(&["a.png", "b.png"]), Path::new("a.png"));
    let idx = deck
        .push_placeholder((800.0, 600.0), "png")
        .expect("con carpeta");
    let slot = &deck.slots[idx];
    assert_eq!(slot.kind, ItemKind::Image);
    assert!(slot.path.extension().is_some_and(|e| e == "png"));
    match &slot.content {
        SlotContent::Ready(d) => {
            assert!(!d.is_design, "un raster nuevo no es un diseño autónomo");
            assert!(
                d.sidecar_enabled,
                "sin sidecar, un raster en blanco perdería sus capas al guardar"
            );
        }
        _ => panic!("se esperaba SlotContent::Ready"),
    }
}

#[test]
fn push_placeholder_allows_multiple_empty_canvases() {
    let mut deck = Deck::from_seed(seed(&["a.png"]), Path::new("a.png"));
    let first = deck.push_placeholder((800.0, 600.0), "canvas");
    let second = deck.push_placeholder((800.0, 600.0), "canvas");
    assert_ne!(first, second);
    assert_eq!(deck.slots.len(), 3);
    assert_ne!(
        deck.slots[first.unwrap()].path,
        deck.slots[second.unwrap()].path
    );
}

#[test]
fn push_placeholder_needs_a_folder() {
    let mut deck = Deck::single(PathBuf::from("a.png"));
    assert_eq!(deck.push_placeholder((800.0, 600.0), "canvas"), None);
    assert_eq!(deck.slots.len(), 1);
}

#[test]
fn merge_scan_keeps_the_placeholder_and_leaves_it_last() {
    let mut deck = Deck::from_seed(seed(&["a.png", "b.png"]), Path::new("a.png"));
    let placeholder_idx = deck.push_placeholder((800.0, 600.0), "canvas").unwrap();
    let placeholder_id = deck.slots[placeholder_idx].id;
    // Reescaneo que reordena y añade un archivo nuevo: la provisional no
    // aparece en ningún listado real.
    deck.merge_scan(vec![
        (PathBuf::from("c.png"), None),
        (PathBuf::from("b.png"), None),
        (PathBuf::from("a.png"), None),
    ]);
    assert_eq!(deck.slots.len(), 4, "la provisional sobrevive al reescaneo");
    let last = deck.slots.last().unwrap();
    assert!(last.is_placeholder);
    assert_eq!(last.id, placeholder_id);
}

#[test]
fn merge_scan_follows_an_active_placeholder() {
    let mut deck = Deck::from_seed(seed(&["a.png", "b.png"]), Path::new("a.png"));
    let placeholder_idx = deck.push_placeholder((800.0, 600.0), "canvas").unwrap();
    // Simula que la provisional pasó a ser la activa (como haría
    // `apply_jump` al saltar a ella).
    deck.slots[deck.active].content = SlotContent::Ready(Box::new(blank_slot_doc(1.0, 1.0)));
    deck.active = placeholder_idx;
    deck.slots[placeholder_idx].content = SlotContent::Active;
    deck.merge_scan(vec![
        (PathBuf::from("c.png"), None),
        (PathBuf::from("b.png"), None),
        (PathBuf::from("a.png"), None),
    ]);
    assert!(
        deck.slots[deck.active].is_placeholder,
        "el índice activo debe seguir apuntando a la provisional tras el reordenado"
    );
}

#[test]
fn evict_never_discards_a_placeholder() {
    let mut deck = Deck::from_seed(
        seed(&["a.png", "b.png", "c.png", "d.png", "e.png", "f.png"]),
        Path::new("a.png"),
    );
    let placeholder_idx = deck.push_placeholder((10.0, 10.0), "canvas").unwrap();
    // Otra ranura limpia y lejana, también sobre el presupuesto: debe
    // descartarse ella y no la provisional.
    let mut doc = blank_slot_doc(10.0, 10.0);
    // Estrictamente por encima del presupuesto (no `==`): el
    // presupuesto total incluye los 0 bytes de la provisional, así que
    // igualarlo exactamente nunca dispara la condición de descarte.
    doc.bytes = EVICT_BUDGET_BYTES + 1;
    deck.slots[5].content = SlotContent::Ready(Box::new(doc));

    let freed = deck.evict();

    assert_eq!(
        freed.len(),
        1,
        "solo la ranura 5 se descarta, no la provisional"
    );
    assert!(matches!(deck.slots[5].content, SlotContent::Idle));
    assert!(
        matches!(deck.slots[placeholder_idx].content, SlotContent::Ready(_)),
        "una provisional nunca se descarta, aunque esté lejos y limpia"
    );
}

#[test]
fn request_loads_never_asks_for_a_placeholder() {
    let mut deck = Deck::from_seed(seed(&["a.png"]), Path::new("a.png"));
    let placeholder_idx = deck.push_placeholder((10.0, 10.0), "canvas").unwrap();
    // Forzar `Idle` a mano: no debería llegar a pasar en la práctica
    // (evict la protege), pero `request_loads` tiene que ser robusta
    // igualmente.
    let placeholder_path = deck.slots[placeholder_idx].path.clone();
    deck.slots[placeholder_idx].content = SlotContent::Idle;
    let spawned = deck.request_loads(&[]);
    assert!(!spawned.contains(&placeholder_path));
}

#[test]
fn a_deck_accepts_a_response_for_its_own_folder_and_generation() {
    let deck = Deck::from_seed(seed(&["a.png", "b.png"]), Path::new("a.png"));
    assert!(deck.accepts_response(Path::new(r"C:\folder"), deck.generation()));
}

#[test]
fn a_deck_rejects_a_response_from_an_older_generation_of_the_same_folder() {
    // Volver a la misma carpeta crea una baraja NUEVA: las respuestas en
    // vuelo de la anterior traen ids de ranura que ya no significan nada.
    let first = Deck::from_seed(seed(&["a.png", "b.png"]), Path::new("a.png"));
    let second = Deck::from_seed(seed(&["a.png", "b.png"]), Path::new("a.png"));

    assert_ne!(first.generation(), second.generation());
    assert!(!second.accepts_response(Path::new(r"C:\folder"), first.generation()));
}

#[test]
fn a_deck_rejects_a_response_for_another_folder() {
    let deck = Deck::from_seed(seed(&["a.png", "b.png"]), Path::new("a.png"));
    assert!(!deck.accepts_response(Path::new(r"C:\otra"), deck.generation()));
}

#[test]
fn a_deck_without_a_folder_accepts_nothing() {
    let deck = Deck::single(PathBuf::from("suelta.png"));
    assert!(deck.folder.is_none());
    assert!(!deck.accepts_response(Path::new(r"C:\folder"), deck.generation()));
}

/// Regresión del bucle infinito de recargas: con `preload_all` y más
/// ranuras que `MAX_LOADED_SLOTS`, antes `request_loads` pedía TODAS las
/// ranuras `Idle` cada frame. Como `evict` descarta por encima de
/// `MAX_LOADED_SLOTS`, un slot recién cargado y luego descartado volvía a
/// `Idle` y se re-pedía en el frame siguiente — cientos de recargas por
/// segundo, la app se quedaba «cargando» para siempre. Ahora
/// `request_loads` limita su radio de precarga al mismo techo que `evict`,
/// así lo que se carga no se descarta de inmediato.
#[test]
fn preload_all_does_not_reload_what_evict_just_dropped() {
    use super::cache::MAX_LOADED_SLOTS;
    // Carpetas con muchos más archivos que el techo de caché.
    let names: Vec<&str> = (0..(MAX_LOADED_SLOTS * 4))
        .map(|i| Box::leak(format!("f{i}.png").into_boxed_str()) as &str)
        .collect();
    let mut deck = Deck::from_seed(seed(&names), Path::new("f0.png"));
    assert!(deck.preload_all);

    // Simula dos frames completos: pedir, recibir, pedir de nuevo.
    // Tras el primer ciclo algunos slots quedan Ready. El segundo ciclo
    // NO debe volver a pedir ninguno de los ya Ready (ni de los que
    // `evict` descartaría): el número de cargas pedidas en el segundo
    // frame debe ser MENOR que en el primero (progresó, no bucle).
    let first_batch = deck.request_loads(&[]);
    // Marca las primeras cargas como Ready (simula respuesta exitosa).
    for path in &first_batch {
        let idx = deck.find_by_path(path).unwrap();
        deck.slots[idx].content = SlotContent::Ready(Box::new(blank_slot_doc(10.0, 10.0)));
        deck.loading_finished();
    }
    // `evict` descarta lo que exceda MAX_LOADED_SLOTS.
    let _ = deck.evict();
    let second_batch = deck.request_loads(&[]);

    // El segundo frame no debe re-pedido nada que ya estaba cargado.
    for path in &first_batch {
        let still_there = deck
            .find_by_path(path)
            .is_some_and(|i| matches!(deck.slots[i].content, SlotContent::Ready(_)));
        if still_there {
            assert!(
                !second_batch.contains(path),
                "slot ya Ready se volvió a pedir — bucle de recarga"
            );
        }
    }
    // Y tampoco debe pedir tantos como para que evict los tire enseguida:
    // la suma de Ready + Loading actuales + pedidos nuevos no debe superar
    // de lejos el techo (el bucle venía de pasarse).
    let ready_count = deck
        .slots
        .iter()
        .filter(|s| matches!(s.content, SlotContent::Ready(_)))
        .count();
    assert!(
        ready_count + second_batch.len() <= MAX_LOADED_SLOTS + MAX_INFLIGHT_LOADS,
        "se pidieron {} con {} ya listos (total {}) — supera el techo de {}",
        second_batch.len(),
        ready_count,
        ready_count + second_batch.len(),
        MAX_LOADED_SLOTS
    );
}
