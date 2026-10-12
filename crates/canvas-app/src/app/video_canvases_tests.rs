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
            build_video_canvas(&accept, NewCanvasFormat::Png)
        })
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    // Regresión: el documento apuntaba al provisional (`Untitled N.png`)
    // y el vídeo se guardaba/mostraba como PNG en vez de como su mp4.
    assert_eq!(
        states[0].doc.source_path.as_deref(),
        Some(dir.path().join("clip-Ana.mp4").as_path()),
        "el documento del lienzo es el clip, no el póster ni el provisional"
    );
    let (_, deck) = assemble_canvases(states, std::slice::from_ref(&single), NewCanvasFormat::Png);
    assert_eq!(deck.slots.len(), 1);
    assert!(deck.slots[0].kind == ItemKind::Video);
    assert_eq!(deck.slots[0].name, "clip-Ana.mp4");
}

#[test]
fn clip_canvases_use_the_new_canvas_format_from_settings() {
    // Regresión: el formato venía incrustado como «png», así que un clip
    // acababa en PNG incluso con «Canvas design» elegido en Ajustes.
    let dir = tempfile::tempdir().unwrap();
    let poster = dir.path().join("poster.png");
    image::RgbaImage::from_pixel(16, 9, image::Rgba([50, 90, 140, 255]))
        .save(&poster)
        .unwrap();
    let build = |format| {
        let mut accept = crate::ytdlp::edit::default_accept(
            dir.path().join("clip.mp4"),
            "clip".to_owned(),
            (1920.0, 1080.0),
        );
        accept.poster = poster.clone();
        build_video_canvas(&accept, format)
    };
    let mut design = build(NewCanvasFormat::Canvas).unwrap();
    assert!(design.is_design);
    assert!(!build(NewCanvasFormat::WebP).unwrap().is_design);

    // Con formato Canvas la ranura es un diseño; con un formato raster la
    // ranura es el vídeo (mp4), nunca el PNG del ajuste: eso es lo que decide
    // el archivo que se escribe al guardar.
    let accept = crate::ytdlp::edit::default_accept(
        dir.path().join("clip.mp4"),
        "clip".to_owned(),
        (1920.0, 1080.0),
    );
    let mut deck = deck::Deck::new_design((1920.0, 1080.0));
    let mut current = editor::EditorState::new_blank(1920.0, 1080.0);
    current.history.mark_unsaved();
    let outgoing = current.take_slot();
    let doc = design.take_slot();
    let first_new = super::stash_and_push(
        &mut deck,
        outgoing,
        vec![(doc, "clip.mp4".to_owned(), (1920.0, 1080.0))],
        std::slice::from_ref(&accept),
        NewCanvasFormat::Canvas,
    )
    .expect("la sesión sin guardar admite hermanos");
    assert_eq!(
        deck.slots[first_new]
            .path
            .extension()
            .and_then(|e| e.to_str()),
        Some(canvas_io::CANVAS_EXTENSION)
    );

    // Regresión del informe: con PNG en Ajustes el clip acababa en `.png`.
    let mut raster = build(NewCanvasFormat::Png).unwrap();
    let mut deck = deck::Deck::new_design((1920.0, 1080.0));
    let mut current = editor::EditorState::new_blank(1920.0, 1080.0);
    current.history.mark_unsaved();
    let first_new = super::stash_and_push(
        &mut deck,
        current.take_slot(),
        vec![(raster.take_slot(), "clip.mp4".to_owned(), (1920.0, 1080.0))],
        std::slice::from_ref(&accept),
        NewCanvasFormat::Png,
    )
    .expect("la sesión sin guardar admite hermanos");
    assert_eq!(
        deck.slots[first_new]
            .path
            .extension()
            .and_then(|e| e.to_str()),
        Some("mp4"),
        "la ranura de un clip es vídeo aunque el ajuste sea PNG"
    );
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
    let accept = crate::ytdlp::edit::default_accept(
        PathBuf::from("clip-Ana.mp4"),
        "clip Ana".to_owned(),
        (1080.0, 1920.0),
    );
    let first_new = super::stash_and_push(
        &mut deck,
        outgoing,
        vec![(new_doc, "clip-Ana.mp4".to_owned(), (1080.0, 1920.0))],
        std::slice::from_ref(&accept),
        NewCanvasFormat::Png,
    )
    .expect("la sesión sin guardar admite hermanos");
    assert!(deck.slots[first_new].kind == ItemKind::Video);
    assert_eq!(
        deck.slots[first_new]
            .path
            .extension()
            .and_then(|e| e.to_str()),
        Some("mp4")
    );
}

#[test]
fn every_trim_becomes_a_dirty_canvas_and_survives_navigation() {
    let dir = tempfile::tempdir().unwrap();
    let poster = dir.path().join("poster.png");
    image::RgbaImage::from_pixel(16, 9, image::Rgba([50, 90, 140, 255]))
        .save(&poster)
        .unwrap();
    // Cada recorte viene de SU clip: con la misma ruta en todos, un cruce de
    // enlaces (el lienzo 2 apuntando al mp4 del 1) pasaba inadvertido.
    let accepts: Vec<_> = [(0.0, 7.0), (10.0, 15.0), (20.0, 25.0)]
        .into_iter()
        .enumerate()
        .map(|(index, (start, end))| {
            let mut accept = crate::ytdlp::edit::default_accept(
                dir.path().join(format!("clip-{}.mp4", index + 1)),
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
        .map(|accept| build_video_canvas(accept, NewCanvasFormat::Png))
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let (mut state, mut deck) = assemble_canvases(states, &accepts, NewCanvasFormat::Png);
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
        // Cada lienzo es SU clip (mp4), no un provisional .png ni el clip de
        // otro recorte: documento y capa enlazan el mismo archivo.
        assert_eq!(
            state.doc.source_path.as_deref(),
            Some(accepts[index].path.as_path())
        );
        assert_eq!(
            link.source_path.as_deref(),
            Some(accepts[index].path.as_path()),
            "el recorte {index} no debe apuntar al clip de otro"
        );
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
    let accept = crate::ytdlp::edit::default_accept(
        PathBuf::from("clip.mp4"),
        "Clip".to_owned(),
        (1080.0, 1920.0),
    );
    let first_new = super::stash_and_push(
        &mut deck,
        outgoing,
        vec![(new_doc, "Clip".to_owned(), (1080.0, 1920.0))],
        std::slice::from_ref(&accept),
        NewCanvasFormat::Png,
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

/// Baraja de carpeta con una foto, para los tests con copia a disco.
fn folder_deck(folder: &std::path::Path) -> deck::Deck {
    let seed = deck::DeckSeed {
        folder: folder.to_owned(),
        sort: crate::settings::GallerySort::Name,
        items: vec![deck::SeedItem {
            path: folder.join("a.png"),
            name: "a.png".to_owned(),
            kind: ItemKind::Image,
            mtime: None,
            thumb: None,
            thumb_failed: false,
        }],
    };
    deck::Deck::from_seed(seed, &folder.join("a.png"))
}

#[test]
fn created_canvas_links_the_temp_and_only_the_render_lands_in_the_project() {
    // Lo aprobado: el descargado NO entra al proyecto — el lienzo enlaza el
    // temporal y la ranura solo reserva el nombre (`1.mp4`…). Solo el primer
    // `Ctrl+S` (el render editado) escribe en el proyecto y lo adopta.
    let staging = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let clip = staging.path().join("clip.mp4");
    std::fs::write(&clip, b"fake-mp4-bytes").unwrap();
    let poster = staging.path().join("poster.png");
    image::RgbaImage::from_pixel(16, 9, image::Rgba([50, 90, 140, 255]))
        .save(&poster)
        .unwrap();
    let mut accept =
        crate::ytdlp::edit::default_accept(clip.clone(), "clip".to_owned(), (1920.0, 1080.0));
    accept.poster = poster;
    let mut built = build_video_canvas(&accept, NewCanvasFormat::Png).unwrap();

    let mut deck = folder_deck(project.path());
    let mut current = editor::EditorState::new_blank_image(64.0, 64.0);
    current.history.mark_unsaved();
    let first_new = super::stash_and_push(
        &mut deck,
        current.take_slot(),
        vec![(built.take_slot(), "clip.mp4".to_owned(), (1920.0, 1080.0))],
        std::slice::from_ref(&accept),
        NewCanvasFormat::Png,
    )
    .expect("la reserva del clip no debe fallar");
    // La reserva no toca disco: el proyecto sigue vacío, la ranura es
    // provisional con el NOMBRE DEL CLIP (no `1.mp4`: la galería mostraba el
    // número y parecía un renombre) y el documento enlaza el temporal.
    assert!(deck.slots[first_new].is_placeholder);
    assert!(!deck.slots[first_new].path.is_file());
    let dst = deck.slots[first_new].path.clone();
    assert_eq!(
        dst.file_name().and_then(|n| n.to_str()),
        Some("clip.mp4"),
        "la reserva lleva el nombre del clip, no el número de la ranura"
    );
    assert_eq!(dst.parent(), Some(project.path()));
    assert_eq!(deck.slots[first_new].name, "clip.mp4");
    assert!(
        std::fs::read_dir(project.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .all(|e| e.path().extension().and_then(|x| x.to_str()) != Some("mp4")),
        "el descargado no se copia al proyecto: solo el render editado lo hará"
    );
    let deck::SlotContent::Ready(doc) = &deck.slots[first_new].content else {
        panic!("ranura lista");
    };
    assert_eq!(doc.doc.source_path.as_deref(), Some(clip.as_path()));
    // El primer render (aquí, un archivo a mano) se adopta: documento + capa
    // apuntan al editado, la provisional se vuelve real y el temporal sigue.
    std::fs::write(&dst, b"edited-render").unwrap();
    let slot_id = deck.slots[first_new].id;
    let mut doc = match std::mem::replace(
        &mut deck.slots[first_new].content,
        deck::SlotContent::Active,
    ) {
        deck::SlotContent::Ready(doc) => *doc,
        _ => panic!("ranura lista"),
    };
    assert!(super::adopt_rendered_video(
        &mut deck,
        &mut doc,
        slot_id,
        &clip,
        &dst,
        5.0,
        (1920, 1080)
    ));
    deck.slots[first_new].content = deck::SlotContent::Ready(Box::new(doc));
    let slot = &deck.slots[first_new];
    assert!(!slot.is_placeholder);
    assert_eq!(
        slot.name,
        slot.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap(),
        "el nombre visible es el render (1.mp4, 2.mp4…), no el temporal"
    );
    let deck::SlotContent::Ready(doc) = &slot.content else {
        panic!("ranura lista");
    };
    assert_eq!(doc.doc.source_path.as_deref(), Some(slot.path.as_path()));
    let link = doc
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
        .expect("capa de vídeo");
    assert_eq!(link.source_path.as_deref(), Some(slot.path.as_path()));
    assert_eq!((link.trim_start, link.trim_end), (0.0, None));
    assert_eq!(
        (link.natural_width, link.natural_height),
        (1920, 1080),
        "adoptar re-ancla al render aunque el temporal no dé tamaño"
    );
    assert!(
        clip.is_file(),
        "el temporal sigue en Download hasta borrarlo"
    );
}

#[test]
fn missing_clip_stashes_and_stale_adopts_are_ignored() {
    // Si el temporal ya no tiene el mp4 (se borró desde Download), el stash
    // no falla ni deja ranuras a medias: el lienzo saliente sigue en la suya
    // y la provisional queda enlazando el temporal (existe el documento,
    // funciona el póster); el guardado avisará y ofrecerá el póster.
    let project = tempfile::tempdir().unwrap();
    let mut deck = folder_deck(project.path());
    let mut current = editor::EditorState::new_blank_image(64.0, 64.0);
    current.history.mark_unsaved();
    let accept = crate::ytdlp::edit::default_accept(
        project.path().join("gone.mp4"),
        "gone".to_owned(),
        (1920.0, 1080.0),
    );
    let mut new_state = editor::EditorState::new_blank_image(64.0, 64.0);
    new_state.history.mark_unsaved();
    let first_new = super::stash_and_push(
        &mut deck,
        current.take_slot(),
        vec![(new_state.take_slot(), "gone.mp4".to_owned(), (64.0, 64.0))],
        std::slice::from_ref(&accept),
        NewCanvasFormat::Png,
    )
    .expect("la reserva no toca disco");
    assert_eq!(deck.slots.len(), 2, "sin ranuras a medias");
    assert!(
        matches!(deck.slots[0].content, deck::SlotContent::Ready(_)),
        "el lienzo saliente sigue guardado en su ranura"
    );
    assert!(deck.slots[first_new].is_placeholder);
    // Una adopción que llega tarde (otra ranura u otra ruta: la baraja cambió
    // por el medio) no toca nada.
    let dst = deck.slots[first_new].path.clone();
    let slot_id = deck.slots[first_new].id;
    let mut doc = match std::mem::replace(
        &mut deck.slots[first_new].content,
        deck::SlotContent::Active,
    ) {
        deck::SlotContent::Ready(doc) => *doc,
        _ => panic!("ranura lista"),
    };
    assert!(!super::adopt_rendered_video(
        &mut deck,
        &mut doc,
        9999,
        &accept.path,
        &dst,
        5.0,
        (1920, 1080)
    ));
    let other = project.path().join("other.mp4");
    assert!(!super::adopt_rendered_video(
        &mut deck,
        &mut doc,
        slot_id,
        &accept.path,
        &other,
        5.0,
        (1920, 1080)
    ));
    deck.slots[first_new].content = deck::SlotContent::Ready(Box::new(doc));
    assert!(deck.slots[first_new].is_placeholder);
}

#[test]
fn clip_slot_reservation_keeps_the_clip_name_and_dedups() {
    // El informe: el clip `clip (8).mp4` acababa en la galería como `38.mp4`
    // (el número de la ranura). Ahora la reserva usa el stem del clip y
    // deduplica contra el disco y contra la misma tanda.
    let project = tempfile::tempdir().unwrap();
    std::fs::write(project.path().join("clip (8).mp4"), b"ya existe").unwrap();
    let mut deck = folder_deck(project.path());
    let reserved = super::reserve_clip_slot_path(&deck, project.path(), "clip (8)", "mp4");
    assert_eq!(
        reserved.file_name().and_then(|n| n.to_str()),
        Some("clip (8) 2.mp4")
    );
    // Sin colisión en disco, el nombre del clip pasa tal cual.
    let free = super::reserve_clip_slot_path(&deck, project.path(), "otro", "mp4");
    assert_eq!(free.file_name().and_then(|n| n.to_str()), Some("otro.mp4"));
    // Dos recortes del mismo clip en una tanda no comparten ruta aunque
    // ningún archivo exista aún: la segunda ve la ranura de la primera.
    let staging = tempfile::tempdir().unwrap();
    let clip = staging.path().join("clip.mp4");
    std::fs::write(&clip, b"x").unwrap();
    let accept = crate::ytdlp::edit::default_accept(clip, "clip".to_owned(), (64.0, 64.0));
    let mut current = editor::EditorState::new_blank_image(64.0, 64.0);
    current.history.mark_unsaved();
    let mk = || {
        let mut s = editor::EditorState::new_blank_image(64.0, 64.0);
        s.history.mark_unsaved();
        (s.take_slot(), "clip.mp4".to_owned(), (64.0, 64.0))
    };
    let first_new = super::stash_and_push(
        &mut deck,
        current.take_slot(),
        vec![mk(), mk()],
        &[accept.clone(), accept],
        NewCanvasFormat::Png,
    )
    .expect("dos recortes del mismo clip");
    let a = deck.slots[first_new].path.clone();
    let b = deck.slots[first_new + 1].path.clone();
    assert_ne!(a, b);
    assert!(a.parent() == Some(project.path()) && b.parent() == Some(project.path()));
}

#[test]
fn canvas_format_clip_points_at_the_project_design() {
    // Con formato «Canvas» el documento apunta desde ya al `.canvas`
    // reservado con el nombre del clip: un diseño se escribe tal cual y sin
    // esto el primer guardado machacaría el temporal con el contenedor.
    let staging = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let clip = staging.path().join("clip.mp4");
    std::fs::write(&clip, b"x").unwrap();
    let accept = crate::ytdlp::edit::default_accept(clip, "clip".to_owned(), (64.0, 64.0));
    let mut deck = folder_deck(project.path());
    let mut current = editor::EditorState::new_blank_image(64.0, 64.0);
    current.history.mark_unsaved();
    let mut built = editor::EditorState::new_blank(64.0, 64.0);
    built.history.mark_unsaved();
    let first_new = super::stash_and_push(
        &mut deck,
        current.take_slot(),
        vec![(built.take_slot(), "clip".to_owned(), (64.0, 64.0))],
        std::slice::from_ref(&accept),
        NewCanvasFormat::Canvas,
    )
    .expect("reserva del diseño");
    let slot = &deck.slots[first_new];
    assert_eq!(
        slot.path.file_name().and_then(|n| n.to_str()),
        Some("clip.canvas")
    );
    let deck::SlotContent::Ready(doc) = &slot.content else {
        panic!("ranura lista");
    };
    assert_eq!(doc.doc.source_path.as_deref(), Some(slot.path.as_path()));
}

#[test]
fn first_save_keeps_the_live_geometry_intact() {
    // El informe: tras `Ctrl+S` el clip de enfrente se veía distorsionado en
    // el lienzo (fondo con blur intacto) y al volver de la galería se veía
    // bien. El render sí sale bien (la recarga lo demuestra), así que la
    // adopción no debe reencuadrar nada visual: ni transform, ni página, ni
    // píxeles en memoria — solo enlaces, trim, duración y el re-anclaje al
    // archivo adoptado (el natural pasa a ser el render).
    let Some(ffmpeg) = canvas_io::ffmpeg_path() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let clip = dir.path().join("clip.mp4");
    let make = canvas_io::media_command(&ffmpeg)
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=720x1280:rate=10",
            "-t",
            "8",
            "-pix_fmt",
            "yuv420p",
        ])
        .arg(&clip)
        .output()
        .unwrap();
    assert!(make.status.success(), "fixture 9:16");
    let project = tempfile::tempdir().unwrap();
    let mut accept =
        crate::ytdlp::edit::default_accept(clip.clone(), "clip".to_owned(), (1080.0, 1920.0));
    accept.trim_start = 2.0;
    accept.trim_end = Some(7.5);
    accept.blur_radius = 12.0;
    let mut built = build_video_canvas(&accept, NewCanvasFormat::Png).unwrap();
    // Composición renderizable a página completa antes de guardar.
    let request = super::video_composition_request(&built).expect("clip 9:16 estándar");
    assert_eq!((request.page_w, request.page_h), (1080, 1920));
    assert_eq!((request.fg_w, request.fg_h), (1080, 1920));
    let snap = |state: &editor::EditorState| {
        let page = state.doc.page().unwrap();
        let fg = page
            .layers
            .iter()
            .find(|l| matches!(l.content, canvas_core::LayerContent::Video(_)))
            .unwrap();
        let canvas_core::LayerContent::Video(_) = &fg.content else {
            unreachable!()
        };
        (
            (page.width, page.height),
            (
                fg.transform.x,
                fg.transform.y,
                fg.transform.width,
                fg.transform.height,
            ),
        )
    };
    let before = snap(&built);
    assert_eq!(before.1 .2, 1080.0);
    assert_eq!(before.1 .3, 1920.0);
    let mut deck = folder_deck(project.path());
    let mut current = editor::EditorState::new_blank_image(64.0, 64.0);
    current.history.mark_unsaved();
    let first_new = super::stash_and_push(
        &mut deck,
        current.take_slot(),
        vec![(built.take_slot(), "clip.mp4".to_owned(), (1080.0, 1920.0))],
        std::slice::from_ref(&accept),
        NewCanvasFormat::Png,
    )
    .expect("reserva");
    let dst = deck.slots[first_new].path.clone();
    std::fs::write(&dst, b"edited-render").unwrap();
    let slot_id = deck.slots[first_new].id;
    let mut doc = match std::mem::replace(
        &mut deck.slots[first_new].content,
        deck::SlotContent::Active,
    ) {
        deck::SlotContent::Ready(doc) => *doc,
        _ => panic!("ranura lista"),
    };
    let mut images_before: Vec<_> = doc.images.keys().copied().collect();
    images_before.sort();
    assert!(super::adopt_rendered_video(
        &mut deck,
        &mut doc,
        slot_id,
        &clip,
        &dst,
        5.5,
        (1080, 1920)
    ));
    // Transform y página, intactos; el natural se re-ancla al render (sus
    // píxeles ya son la página, no el clip original).
    let page = doc.doc.page().unwrap();
    let fg = page
        .layers
        .iter()
        .find(|l| matches!(l.content, canvas_core::LayerContent::Video(_)))
        .unwrap();
    let canvas_core::LayerContent::Video(link) = &fg.content else {
        unreachable!()
    };
    let after = (
        (page.width, page.height),
        (
            fg.transform.x,
            fg.transform.y,
            fg.transform.width,
            fg.transform.height,
        ),
    );
    assert_eq!(
        before, after,
        "adoptar el render no reencuadra el lienzo vivo"
    );
    assert_eq!(
        (link.natural_width, link.natural_height),
        (1080, 1920),
        "la capa describe el archivo adoptado, no el clip original"
    );
    assert_eq!((link.trim_start, link.trim_end), (0.0, None));
    assert_eq!(link.duration_secs, Some(5.5));
    let mut keys: Vec<_> = doc.images.keys().copied().collect();
    keys.sort();
    assert_eq!(keys, images_before, "los píxeles en memoria no se tocan");
    deck.slots[first_new].content = deck::SlotContent::Ready(Box::new(doc));
}

fn video_state(
    page: (f64, f64),
    fg: (f64, f64, f64, f64),
    blur: f32,
    trim: (f64, Option<f64>),
) -> editor::EditorState {
    use canvas_core::{ImageContent, LayerContent, Transform, VideoContent};
    let mut state = editor::EditorState::new_blank_image(page.0, page.1);
    state.doc.source_path = Some(PathBuf::from("clip.mp4"));
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
    state.doc.layer_mut(bg).unwrap().effects.blur_radius = blur;
    state.background_layer = Some(bg);
    state
        .doc
        .add_layer(
            "Clip",
            Transform::new(fg.0, fg.1, fg.2, fg.3),
            LayerContent::Video(VideoContent {
                source_path: Some(PathBuf::from("clip.mp4")),
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
    state
}

#[test]
fn render_resolves_the_page_layout_not_the_source() {
    // Lo que `Ctrl+S` renderiza sale de aquí: la página + el rectángulo de
    // la capa + el blur del fondo + el trim. El caso del informe: fuente
    // 9:16 en página 1920×1080 — la salida es la página, no la fuente.
    let state = video_state(
        (1920.0, 1080.0),
        (656.0, 0.0, 608.0, 1080.0),
        12.0,
        (2.0, Some(7.5)),
    );
    let request = super::video_composition_request(&state).expect("lienzo de vídeo estándar");
    assert_eq!(request.src, PathBuf::from("clip.mp4"));
    assert_eq!((request.start_s, request.end_s), (2.0, Some(7.5)));
    assert_eq!((request.page_w, request.page_h), (1920, 1080));
    assert_eq!(
        (request.fg_x, request.fg_y, request.fg_w, request.fg_h),
        (656, 0, 608, 1080)
    );
    assert_eq!(request.bg_blur, 12.0);
    // Sin capa de vídeo no hay composición: el guardado sigue su camino
    // raster de siempre en vez de inventar un mp4.
    let blank = editor::EditorState::new_blank_image(64.0, 64.0);
    assert!(super::video_composition_request(&blank).is_none());
}

#[test]
fn render_evens_odd_dimensions_and_rejects_what_ffmpeg_cannot_reproduce() {
    // x264/yuv420p exigen pares: 607 baja a 606 (1 px frente al editor).
    let state = video_state(
        (1920.0, 1080.0),
        (656.0, 0.0, 607.0, 1080.0),
        0.0,
        (0.0, None),
    );
    let request = super::video_composition_request(&state).expect("pares por redondeo");
    assert_eq!((request.fg_w, request.fg_h), (606, 1080));
    assert_eq!(request.bg_blur, 0.0);
    // Vídeo rotado: ffmpeg no lo reproduciría → sin mp4 a medias.
    let mut rotated = video_state(
        (1920.0, 1080.0),
        (656.0, 0.0, 608.0, 1080.0),
        0.0,
        (0.0, None),
    );
    let id = rotated
        .doc
        .page()
        .unwrap()
        .layers
        .iter()
        .find(|l| matches!(l.content, canvas_core::LayerContent::Video(_)))
        .unwrap()
        .id;
    rotated.doc.layer_mut(id).unwrap().transform.rotation = 90.0;
    assert!(super::video_composition_request(&rotated).is_none());
    // Capa visible extra (una forma encima del clip): tampoco se renderiza.
    let mut busy = video_state(
        (1920.0, 1080.0),
        (656.0, 0.0, 608.0, 1080.0),
        0.0,
        (0.0, None),
    );
    busy.doc
        .add_layer(
            "Título",
            canvas_core::Transform::new(0.0, 0.0, 100.0, 40.0),
            canvas_core::LayerContent::Shape(canvas_core::ShapeContent {
                kind: canvas_core::ShapeKind::Rect,
                fill: [255, 255, 255, 255],
                stroke: [0, 0, 0, 0],
                stroke_width: 0.0,
                corner_radius: 0.0,
            }),
        )
        .unwrap();
    assert!(super::video_composition_request(&busy).is_none());
}

#[test]
fn in_place_save_resets_the_trim_to_the_whole_file() {
    // Tras guardar en el sitio, el archivo ES la composición: el trim pasa al
    // archivo entero (si no, el próximo guardado recortaría dos veces). Solo
    // las capas de ese archivo; con otro destino («Guardar como…») no se toca.
    fn link_of(slot: &deck::SlotDoc) -> &canvas_core::VideoContent {
        slot.doc
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
            .unwrap()
    }
    let mut slot = video_state(
        (1920.0, 1080.0),
        (656.0, 0.0, 608.0, 1080.0),
        0.0,
        (2.0, Some(7.5)),
    )
    .take_slot();
    super::reset_rendered_trim(
        &mut slot,
        std::path::Path::new("clip.mp4"),
        5.5,
        (1920, 1080),
    );
    assert_eq!(
        (
            link_of(&slot).trim_start,
            link_of(&slot).trim_end,
            link_of(&slot).poster_time
        ),
        (0.0, None, 0.0)
    );
    assert_eq!(link_of(&slot).duration_secs, Some(5.5));
    // Re-anclaje: los píxeles ya son la página — natural de página y rect a
    // pantalla completa, para que la reproducción no re-escale el archivo al
    // tamaño del clip original (eso distorsionaba el clip hasta recargar).
    assert_eq!(
        (link_of(&slot).natural_width, link_of(&slot).natural_height),
        (1920, 1080)
    );
    let fg = slot
        .doc
        .page()
        .unwrap()
        .layers
        .iter()
        .find(|l| matches!(l.content, canvas_core::LayerContent::Video(_)))
        .unwrap();
    assert_eq!(
        (
            fg.transform.x,
            fg.transform.y,
            fg.transform.width,
            fg.transform.height
        ),
        (0.0, 0.0, 1920.0, 1080.0)
    );
    let mut other = video_state(
        (1920.0, 1080.0),
        (656.0, 0.0, 608.0, 1080.0),
        0.0,
        (2.0, Some(7.5)),
    )
    .take_slot();
    super::reset_rendered_trim(
        &mut other,
        std::path::Path::new("otro.mp4"),
        5.5,
        (1920, 1080),
    );
    assert_eq!(
        (link_of(&other).trim_start, link_of(&other).trim_end),
        (2.0, Some(7.5))
    );
    assert_eq!(
        (
            link_of(&other).natural_width,
            link_of(&other).natural_height
        ),
        (540, 960),
        "el archivo ajeno no se re-ancla"
    );
}

#[test]
fn adopting_a_mismatched_render_goes_full_bleed_with_page_natural() {
    // El informe exacto: clip 9:16 con fondo con blur en página 16:9 — tras
    // el primer Ctrl+S el clip de enfrente se veía distorsionado (mismo
    // tamaño, fondo intacto) y al volver de la galería se veía bien. El
    // archivo adoptado ES la página, así que la capa pasa a pantalla completa
    // con natural de página: la reproducción decodifica 1:1 sin re-escalar
    // al tamaño del clip original y muestra la composición tal cual salió.
    let project = tempfile::tempdir().unwrap();
    let dst = project.path().join("clip.mp4");
    std::fs::write(&dst, b"edited-render").unwrap();
    let mut deck = folder_deck(project.path());
    deck.slots[0].path = dst.clone();
    deck.slots[0].content = deck::SlotContent::Ready(Box::new(
        video_state(
            (1920.0, 1080.0),
            (656.0, 0.0, 608.0, 1080.0),
            12.0,
            (2.0, Some(7.5)),
        )
        .take_slot(),
    ));
    let slot_id = deck.slots[0].id;
    let mut doc = match std::mem::replace(&mut deck.slots[0].content, deck::SlotContent::Active) {
        deck::SlotContent::Ready(doc) => *doc,
        _ => panic!("ranura lista"),
    };
    assert!(super::adopt_rendered_video(
        &mut deck,
        &mut doc,
        slot_id,
        std::path::Path::new("clip.mp4"),
        &dst,
        5.5,
        (1920, 1080),
    ));
    let page = doc.doc.page().unwrap();
    let fg = page
        .layers
        .iter()
        .find(|l| matches!(l.content, canvas_core::LayerContent::Video(_)))
        .unwrap();
    let canvas_core::LayerContent::Video(link) = &fg.content else {
        unreachable!()
    };
    assert_eq!(link.source_path.as_deref(), Some(dst.as_path()));
    assert_eq!((link.trim_start, link.trim_end), (0.0, None));
    assert_eq!(
        (link.natural_width, link.natural_height),
        (1920, 1080),
        "decodificar el render a su tamaño real no lo aplasta"
    );
    assert_eq!(
        (
            fg.transform.x,
            fg.transform.y,
            fg.transform.width,
            fg.transform.height
        ),
        (0.0, 0.0, 1920.0, 1080.0),
        "los píxeles ya son la composición: se muestra 1:1"
    );
    deck.slots[0].content = deck::SlotContent::Ready(Box::new(doc));
    assert!(!deck.slots[0].is_placeholder);
}
