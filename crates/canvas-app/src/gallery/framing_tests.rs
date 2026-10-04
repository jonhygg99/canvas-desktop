use super::*;

#[test]
fn late_thumbnail_does_not_replace_a_newly_saved_framing() {
    let ctx = egui::Context::default();
    let mut gallery = GalleryFramings::default();
    let path = PathBuf::from("clip.png");
    gallery.invalidate(&path);
    gallery
        .tx
        .send(ResultCard {
            path: path.clone(),
            version: 0,
            saved: Some(Framing::default()),
            error: None,
            source: None,
            background: None,
        })
        .unwrap();
    gallery.poll(&ctx);
    assert!(!gallery.cards.contains_key(&path));
    gallery
        .tx
        .send(ResultCard {
            path: path.clone(),
            version: 1,
            saved: Some(Framing {
                scale_pct: 80,
                ..Framing::default()
            }),
            error: None,
            source: None,
            background: None,
        })
        .unwrap();
    gallery.poll(&ctx);
    assert_eq!(gallery.cards[&path].saved.unwrap().scale_pct, 80);
}

#[test]
fn saved_filter_matches_only_valid_sidecars_and_keeps_pending_items() {
    let mut gallery = GalleryFramings::default();
    let saved = PathBuf::from("saved.png");
    let missing = PathBuf::from("missing.png");
    gallery.cards.insert(
        saved.clone(),
        Card {
            saved: Some(Framing::default()),
            ..Card::default()
        },
    );
    gallery.cards.insert(missing.clone(), Card::default());
    gallery.filter = StatusFilter::Saved;
    assert!(gallery.matches(&saved));
    assert!(!gallery.matches(&missing));
    assert!(gallery.matches(Path::new("pending.png")));
    gallery.filter = StatusFilter::Missing;
    assert!(!gallery.matches(&saved));
    assert!(gallery.matches(&missing));
}
