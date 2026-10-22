//! Miniaturas que faltan en la baraja (`DeckOps::claim_missing_thumbs`): una
//! miniatura que falló —o cuya petición nunca llegó— se recupera sola desde el
//! editor, con reloj e intentos acotados, sin depender de otro reescaneo.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui;

use crate::deck;

use super::DeckOps;

/// Baraja con tres archivos; los índices de `failed` nacen con la miniatura
/// fallida.
fn deck_with(failed: &[usize]) -> deck::Deck {
    let items: Vec<deck::SeedItem> = (0..3)
        .map(|i| {
            let name = format!("c-0{}.png", i + 1);
            deck::SeedItem {
                path: PathBuf::from("fotos").join(&name),
                name,
                kind: crate::gallery::ItemKind::Image,
                mtime: None,
                thumb: None,
                thumb_failed: false,
            }
        })
        .collect();
    let first = items[0].path.clone();
    let mut deck = deck::Deck::from_seed(
        deck::DeckSeed {
            folder: PathBuf::from("fotos"),
            sort: crate::settings::GallerySort::default(),
            items,
        },
        &first,
    );
    for (i, slot) in deck.slots.iter_mut().enumerate() {
        slot.thumb_failed = failed.contains(&i);
    }
    deck
}

fn path(n: usize) -> PathBuf {
    PathBuf::from(format!("fotos/c-0{n}.png"))
}

#[test]
fn a_failed_thumbnail_is_claimed_right_away() {
    let deck = deck_with(&[1]);
    let mut ops = DeckOps::default();
    let now = Instant::now();
    assert_eq!(
        ops.claim_missing_thumbs(&deck, now, 4),
        vec![path(2)],
        "la que falló se reintenta en la primera tanda; las que aún esperan, no"
    );
    assert!(
        ops.claim_missing_thumbs(&deck, now + Duration::from_millis(100), 4)
            .is_empty(),
        "no se reintenta en cada frame: hay un reloj entre tandas"
    );
}

/// Una petición que nunca llegó no puede dejar el spinner girando para
/// siempre: tras la espera, el editor pide la miniatura por su cuenta.
#[test]
fn a_thumbnail_that_never_arrived_is_requested_after_the_wait() {
    let deck = deck_with(&[]);
    let mut ops = DeckOps::default();
    let now = Instant::now();
    assert!(
        ops.claim_missing_thumbs(&deck, now, 4).is_empty(),
        "recién vista: el escaneo todavía la está generando"
    );
    assert_eq!(
        ops.claim_missing_thumbs(&deck, now + Duration::from_secs(7), 4)
            .len(),
        3,
        "pasada la espera se pide una para cada hueco"
    );
}

#[test]
fn retries_are_bounded_per_file() {
    let deck = deck_with(&[0]);
    let mut ops = DeckOps::default();
    let mut now = Instant::now();
    let mut attempts = 0;
    // Diez vueltas de reloj: el archivo que falla agota sus intentos y se deja
    // el aviso, en vez de lanzar un hilo cada dos segundos toda la sesión.
    for _ in 0..10 {
        attempts += ops
            .claim_missing_thumbs(&deck, now, 4)
            .iter()
            .filter(|claimed| **claimed == path(1))
            .count();
        now += Duration::from_secs(3);
    }
    assert_eq!(attempts, 3);
}

#[test]
fn a_thumbnail_that_arrived_is_forgotten() {
    let deck = deck_with(&[0]);
    let mut ops = DeckOps::default();
    let now = Instant::now();
    assert_eq!(ops.claim_missing_thumbs(&deck, now, 4).len(), 1);

    // Llegó la miniatura: la ranura deja de pedir y su contador se olvida, así
    // que un fallo posterior vuelve a tener sus tres intentos.
    let mut healed = deck_with(&[0]);
    let ctx = egui::Context::default();
    healed.slots[0].thumb = Some(ctx.load_texture(
        "miniatura",
        egui::ColorImage::example(),
        egui::TextureOptions::LINEAR,
    ));
    assert!(ops.claim_missing_thumbs(&healed, now, 4).is_empty());
    assert!(ops.thumb_retries.is_empty(), "contadores olvidados");
    healed.slots[0].thumb = None;
    healed.slots[0].thumb_failed = true;
    assert_eq!(
        ops.claim_missing_thumbs(&healed, now + Duration::from_secs(5), 4)
            .len(),
        1
    );
}

#[test]
fn the_batch_respects_the_per_frame_limit() {
    let deck = deck_with(&[0, 1, 2]);
    let mut ops = DeckOps::default();
    let mut now = Instant::now();
    let mut seen: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    for _ in 0..5 {
        let claimed = ops.claim_missing_thumbs(&deck, now, 2);
        assert!(claimed.len() <= 2, "tope por pasada");
        seen.extend(claimed);
        now += Duration::from_secs(3);
    }
    assert_eq!(seen.len(), 3, "las tres acaban recibiendo su intento");
}
