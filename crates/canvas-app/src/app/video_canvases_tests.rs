use super::*;
use crate::gallery::ItemKind;
use std::path::PathBuf;

#[test]
fn clip_canvases_are_video_slots_named_after_the_clip() {
    // Regresión: las ranuras nuevas salían como imagen (`N.png`) en vez de
    // como el vídeo del que vienen.
    let dir = tempfile::tempdir().unwrap();
    let poster = dir.path().join("poster.png");
    image::RgbaImage::from_pixel(16, 9, image::Rgba([50, 90, 140, 255]))
        .save(&poster)
        .unwrap();
    let single = crate::ytdlp::edit::default_accept(
        dir.path().join("clip-Ana.mp4"),
        "clip Ana".to_owned(),
        (1920.0, 1080.0),
    );
    assert_eq!(clip_slot_name(&single, false), "clip-Ana.mp4");
    // Con varios recortes manda el título, que ya los numera.
    let mut many = single.clone();
    many.title = "clip Ana — Trim 2".to_owned();
    assert_eq!(clip_slot_name(&many, true), "clip Ana — Trim 2");
    // Un clip sin nombre de archivo cae al título en vez de a una ranura muda.
    let mut nameless = single.clone();
    nameless.path = PathBuf::new();
    assert_eq!(clip_slot_name(&nameless, false), "clip Ana");

    let states = std::iter::once(single.clone())
        .map(|mut accept| {
            accept.poster = poster.clone();
            build_video_canvas(&accept)
        })
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let (_, deck) = assemble_canvases(states, std::slice::from_ref(&single));
    assert_eq!(deck.slots.len(), 1);
    assert!(deck.slots[0].kind == ItemKind::Video);
    assert_eq!(deck.slots[0].name, "clip-Ana.mp4");
}

#[test]
fn appended_clip_canvases_are_video_slots() {
    let mut deck = deck::Deck::new_design((1920.0, 1080.0));
    let mut current = editor::EditorState::new_blank(1920.0, 1080.0);
    current.history.mark_unsaved();
    let outgoing = current.take_slot();
    let mut new_state = editor::EditorState::new_blank_image(1080.0, 1920.0);
    new_state.history.mark_unsaved();
    let new_doc = new_state.take_slot();
    let first_new = super::stash_and_push(
        &mut deck,
        outgoing,
        vec![(new_doc, "clip-Ana.mp4".to_owned(), (1080.0, 1920.0))],
    )
    .expect("la sesión sin guardar admite hermanos");
    assert!(deck.slots[first_new].kind == ItemKind::Video);
}

#[test]
fn every_trim_becomes_a_dirty_canvas_and_survives_navigation() {
    let dir = tempfile::tempdir().unwrap();
    let poster = dir.path().join("poster.png");
    image::RgbaImage::from_pixel(16, 9, image::Rgba([50, 90, 140, 255]))
        .save(&poster)
        .unwrap();
    let accepts: Vec<_> = [(0.0, 7.0), (10.0, 15.0), (20.0, 25.0)]
        .into_iter()
        .enumerate()
        .map(|(index, (start, end))| {
            let mut accept = crate::ytdlp::edit::default_accept(
                dir.path().join("clip.mp4"),
                format!("Clip — Trim {}", index + 1),
                (1080.0, 1920.0),
            );
            accept.poster = poster.clone();
            accept.video_size = Some((16.0, 9.0));
            accept.trim_start = start;
            accept.trim_end = Some(end);
            accept
        })
        .collect();
    let states = accepts
        .iter()
        .map(build_video_canvas)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let (mut state, mut deck) = assemble_canvases(states, &accepts);
    assert_eq!(deck.slots.len(), 3);
    assert!(deck.unsaved_session);
    assert!(deck.can_add_canvas());
    for index in [0, 1, 2, 0] {
        if index != deck.active {
            deck.jump_to = Some(index);
            assert!(matches!(
                deck::apply_jump(&mut deck, &mut state),
                deck::JumpOutcome::Applied
            ));
        }
        let link = state
            .doc
            .page()
            .unwrap()
            .layers
            .iter()
            .find_map(|layer| {
                if let canvas_core::LayerContent::Video(link) = &layer.content {
                    Some(link)
                } else {
                    None
                }
            })
            .unwrap();
        assert_eq!(
            (link.trim_start, link.trim_end),
            (accepts[index].trim_start, accepts[index].trim_end)
        );
        assert_eq!(state.doc.page().unwrap().layers.len(), 2);
        assert!(state.history.is_dirty());
        assert!(state.doc.source_path.is_none());
        assert_eq!(deck.slots[index].name, accepts[index].title);
        assert!(deck.slots[index].kind == ItemKind::Video);
    }
}

#[test]
fn create_canvas_keeps_current_project_instead_of_replacing_it() {
    // Regresión: `Create canvas` añadía un proyecto nuevo y tiraba el lienzo
    // en curso; ahora los nuevos van a la misma baraja y el actual se guarda
    // en su ranura.
    let mut deck = deck::Deck::new_design((1920.0, 1080.0));
    let mut current = editor::EditorState::new_blank_image(1920.0, 1080.0);
    current
        .doc
        .add_layer(
            "actual",
            canvas_core::Transform::new(10.0, 10.0, 100.0, 100.0),
            canvas_core::LayerContent::Shape(canvas_core::ShapeContent {
                kind: canvas_core::ShapeKind::Rect,
                fill: [10, 20, 30, 255],
                stroke: [0, 0, 0, 0],
                stroke_width: 2.0,
                corner_radius: 0.0,
            }),
        )
        .unwrap();
    current.history.mark_unsaved();
    let outgoing = current.take_slot();
    assert!(outgoing.history.is_dirty());

    let mut new_state = editor::EditorState::new_blank_image(1080.0, 1920.0);
    new_state.history.mark_unsaved();
    let new_doc = new_state.take_slot();
    let first_new = super::stash_and_push(
        &mut deck,
        outgoing,
        vec![(new_doc, "Clip".to_owned(), (1080.0, 1920.0))],
    )
    .expect("la sesión sin guardar admite hermanos");
    assert_eq!(first_new, 1);
    assert_eq!(deck.slots.len(), 2);
    // El lienzo actual sigue en su ranura, sucio y sin perder su capa.
    let deck::SlotContent::Ready(old) = &deck.slots[0].content else {
        panic!("el lienzo actual debe quedar guardado");
    };
    assert!(old.history.is_dirty());
    assert!(old
        .doc
        .page()
        .unwrap()
        .layers
        .iter()
        .any(|l| l.name == "actual"));
    // El nuevo ocupa su ranura con su nombre y tamaño.
    assert_eq!(deck.slots[first_new].name, "Clip");
    assert_eq!(deck.slots[first_new].page, Some((1080.0, 1920.0)));
    assert!(matches!(
        deck.slots[first_new].content,
        deck::SlotContent::Ready(_)
    ));
}
