//! La cola única de escritura (`write_queue.rs`): una baraja, una escritura
//! en vuelo, ids estables. Hermano de `write_queue.rs`.

use std::path::PathBuf;

use super::WriteQueue;

/// La cola es de UNA baraja y de UNA escritura a la vez: duplicados
/// ignorados, `next` nunca pisa la que está en vuelo, cambiar de baraja la
/// vacía y un `finished` con otro id no libera el vuelo.
#[test]
fn the_queue_keeps_one_write_in_flight_and_forgets_other_decks() {
    let mut queue = WriteQueue::default();
    queue.enqueue(7, [3, 4, 5, 4]);
    assert!(queue.busy(7));
    assert_eq!(queue.remaining(7), 3, "el duplicado no se encola dos veces");
    assert!(!queue.busy(8), "otra baraja no hereda la cola");
    assert_eq!(queue.remaining(8), 0);

    assert_eq!(queue.next(), Some(3));
    assert_eq!(queue.next(), None, "una en vuelo: no se arranca otra");
    queue.enqueue(7, [9]);
    assert_eq!(queue.next(), None);
    queue.finished(99);
    assert_eq!(
        queue.next(),
        None,
        "una escritura en vuelo no se suelta con un id ajeno"
    );
    queue.finished(3);
    assert_eq!(queue.next(), Some(4));
    queue.finished(4);
    assert_eq!(queue.next(), Some(5));
    queue.finished(5);
    assert_eq!(queue.next(), Some(9));

    // Cambiar de baraja olvida pendiente y vuelo.
    queue.sync(11);
    assert!(!queue.busy(11));
    assert_eq!(queue.next(), None);
}

/// Una ranura que desaparece (`find_by_id` falla) se descuenta igual: la cola
/// no se atasca por un id que ya no existe.
#[test]
fn the_queue_drops_an_id_that_no_longer_exists() {
    let mut queue = WriteQueue::default();
    queue.enqueue(1, [2]);
    assert_eq!(queue.next(), Some(2));
    queue.finished(2);
    assert!(!queue.busy(1));
    assert_eq!(queue.remaining(1), 0);
}

/// Una ranura en vuelo no se re-encola: si su escritura falla y sigue sucia,
/// un solo `next` la vuelve a tomar cuando corresponde, sin duplicados.
#[test]
fn an_in_flight_slot_is_never_queued_twice() {
    let mut queue = WriteQueue::default();
    queue.enqueue(1, [4]);
    assert_eq!(queue.next(), Some(4));
    queue.enqueue(1, [4]);
    assert_eq!(queue.remaining(1), 1, "solo cuenta el que está en vuelo");
    queue.finished(4);
    assert_eq!(queue.next(), None);
}

/// Baraja de proyecto con las ranuras dadas; la primera es la activa.
fn seeded(paths: &[&str], active: &str) -> crate::deck::Deck {
    use crate::deck::{Deck, DeckSeed, SeedItem};
    use crate::gallery::ItemKind;
    use crate::settings::GallerySort;
    let seed = DeckSeed {
        folder: PathBuf::from("proj"),
        sort: GallerySort::Name,
        items: paths
            .iter()
            .map(|path| SeedItem {
                path: PathBuf::from(path),
                name: (*path).to_owned(),
                kind: ItemKind::Image,
                mtime: None,
                thumb: None,
                thumb_failed: false,
            })
            .collect(),
    };
    Deck::from_seed(seed, std::path::Path::new(active))
}

/// `SlotDoc` raster con ese archivo como origen; `dirty` lo deja pendiente.
fn raster_slot(path: &str, dirty: bool) -> crate::deck::SlotDoc {
    let mut doc = crate::editor::EditorState::new_blank_image(100.0, 100.0).take_slot();
    doc.doc.source_path = Some(PathBuf::from(path));
    if dirty {
        doc.history.mark_unsaved();
    } else {
        doc.history.mark_saved();
    }
    doc
}

/// Lo editado y luego abandonado se escribe solo, también si está de fondo;
/// el activo y lo que ya está limpio no entran en la cola.
#[test]
fn only_dirty_background_slots_with_a_safe_destination_are_queued() {
    use crate::app::SaveFlow;
    let mut deck = seeded(&["a.png", "b.png", "c.png"], "a.png");
    deck.slots[1].content = crate::deck::SlotContent::Ready(Box::new(raster_slot("b.png", true)));
    deck.slots[2].content = crate::deck::SlotContent::Ready(Box::new(raster_slot("c.png", false)));
    let mut save = SaveFlow::default();
    save.mark_app_written(std::path::Path::new("b.png"));
    assert_eq!(
        super::dirty_background_slots(&deck, &save, false),
        vec![deck.slots[1].id],
        "solo la ranura de fondo sucia y escribible"
    );
}

/// El original del usuario no se escribe solo desde el fondo: necesita la
/// confirmación que solo puede pedir su propio lienzo («Save all» la pide).
#[test]
fn an_original_without_the_overwrite_answer_is_not_queued() {
    use crate::app::SaveFlow;
    let mut deck = seeded(&["a.png", "b.png"], "a.png");
    deck.slots[1].content = crate::deck::SlotContent::Ready(Box::new(raster_slot("b.png", true)));
    let save = SaveFlow::default();
    assert!(super::dirty_background_slots(&deck, &save, false).is_empty());

    let mut confirmed = SaveFlow::default();
    confirmed.confirm_overwrite(std::path::Path::new("b.png"));
    assert_eq!(
        super::dirty_background_slots(&deck, &confirmed, false),
        vec![deck.slots[1].id]
    );
}

/// Un fallo no se reintenta en bucle: la misma revisión queda bloqueada y solo
/// una edición nueva (otra revisión) vuelve a encolarla.
#[test]
fn a_failed_write_is_not_retried_until_the_next_edit() {
    use crate::app::SaveFlow;
    let mut deck = seeded(&["a.png", "b.png"], "a.png");
    deck.slots[1].content = crate::deck::SlotContent::Ready(Box::new(raster_slot("b.png", true)));
    let mut save = SaveFlow::default();
    save.mark_app_written(std::path::Path::new("b.png"));
    let slot_id = deck.slots[1].id;
    let revision = match &deck.slots[1].content {
        crate::deck::SlotContent::Ready(doc) => doc.history.revision(),
        _ => unreachable!(),
    };
    save.blocked_writes.insert(slot_id, revision);
    assert!(super::dirty_background_slots(&deck, &save, false).is_empty());

    if let crate::deck::SlotContent::Ready(doc) = &mut deck.slots[1].content {
        doc.history.push_applied(Box::new(canvas_core::Rename {
            layer: canvas_core::LayerId::from_raw(1),
            before: "a".into(),
            after: "b".into(),
        }));
    }
    assert_eq!(
        super::dirty_background_slots(&deck, &save, false),
        vec![slot_id],
        "una edición nueva vuelve a intentarlo"
    );
}

/// Un clip de fondo editado también se encola: su destino automático lo decide
/// `clip_save_target` (la misma política que el `Ctrl+S` del clip).
#[test]
fn an_edited_background_clip_is_queued_too() {
    use super::super::clip_fixture::{clip_state, fake};
    use crate::app::SaveFlow;
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let saved = fake(&project.join("clip-01.mp4"));
    let source = fake(&project.join(".download/clip.mp4"));
    let mut deck = seeded(&["a.png", "clip-01.mp4"], "a.png");
    deck.folder = Some(project.clone());
    let slot = clip_state((1080.0, 1920.0), &saved, &source, (2.0, Some(7.5))).take_slot();
    deck.slots[1].content = crate::deck::SlotContent::Ready(Box::new(slot));
    deck.slots[1].path = saved.clone();
    assert_eq!(
        super::dirty_background_slots(&deck, &SaveFlow::default(), false),
        vec![deck.slots[1].id]
    );
}
