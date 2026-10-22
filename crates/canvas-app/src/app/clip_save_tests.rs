//! Tabla de «¿dónde se guarda un clip?»: el destino que comparten el `Ctrl+S`
//! y el autoguardado. Hermano de `clip_save.rs` (convención `*_tests.rs`).
use super::super::clip_fixture::{clip_canvas, clip_state, fake};
use super::{clip_save_target, is_clip_document, source_missing};

#[test]
fn a_new_writes_to_its_reserved_name_in_the_project() {
    let tmp = tempfile::tempdir().unwrap();
    let source = fake(&tmp.path().join("download/clip.mp4"));
    let project = tmp.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    let state = clip_state((1080.0, 1920.0), &source, &source, (2.0, Some(7.5)));
    let slot = project.join("clip-01.mp4");
    assert_eq!(
        clip_save_target(
            &state.doc,
            state.background_layer,
            &slot,
            true,
            Some(&project),
        ),
        Some(slot.clone()),
        "el primer guardado escribe el nombre reservado, no el temporal"
    );
    // Y el temporal sigue intacto: el mp4 de la descarga no se pisa.
    assert_eq!(std::fs::read(&source).unwrap(), b"x");
    assert_eq!(
        super::composition_of(&state.doc, state.background_layer)
            .map(|r| (r.start_s, r.end_s, r.page_w, r.page_h)),
        Some((2.0, Some(7.5), 1080, 1920))
    );
}

/// Sin carpeta de proyecto no hay destino que escribir sin preguntar: la
/// baraja sin guardar (o un mp4 suelto) sigue pidiendo «Save as…».
#[test]
fn a_clip_without_a_project_has_no_automatic_target() {
    let tmp = tempfile::tempdir().unwrap();
    let source = fake(&tmp.path().join("clip.mp4"));
    let state = clip_canvas(&source);
    let slot = tmp.path().join("clip.mp4");
    assert_eq!(
        clip_save_target(&state.doc, state.background_layer, &slot, true, None),
        None
    );
    // Ranura real, archivo propio del lienzo, pero sin proyecto: diálogo.
    assert_eq!(
        clip_save_target(&state.doc, state.background_layer, &source, false, None),
        None
    );
}

/// Un clip ya guardado dentro del proyecto se reescribe en su sitio.
#[test]
fn an_already_saved_project_writes_in_place() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let source = fake(&project.join("clip-01.mp4"));
    let state = clip_canvas(&source);
    assert_eq!(
        clip_save_target(
            &state.doc,
            state.background_layer,
            &source,
            false,
            Some(&project)
        ),
        Some(source.clone())
    );
    // La ranura y el origen discrepan (p. ej. tras renombrar el archivo):
    // no se inventa un destino, decide el diálogo.
    let other = project.join("otro.mp4");
    assert_eq!(
        clip_save_target(
            &state.doc,
            state.background_layer,
            &other,
            false,
            Some(&project)
        ),
        None
    );
}

/// Si se borró el temporal desde Download no hay composición que renderizar;
/// el póster se salva por «Save as…» y el autoguardado no toca nada.
#[test]
fn a_deleted_source_has_no_target_and_is_reported() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    let gone = tmp.path().join("download/clip.mp4");
    let state = clip_canvas(&gone);
    assert!(source_missing(&state.doc));
    assert_eq!(
        clip_save_target(
            &state.doc,
            state.background_layer,
            &project.join("clip-01.mp4"),
            true,
            Some(&project),
        ),
        None
    );
    // Con el archivo presente, la misma consulta sí tiene destino.
    fake(&gone);
    let state = clip_canvas(&gone);
    assert!(!source_missing(&state.doc));
    assert!(clip_save_target(
        &state.doc,
        state.background_layer,
        &project.join("clip-01.mp4"),
        true,
        Some(&project),
    )
    .is_some());
}

/// La comprobación de fuente mira lo que ffmpeg LEE (la capa), no el archivo
/// que escribe: con el mp4 del proyecto intacto pero el clip original
/// borrado no hay composición y el autoguardado no debe reintentar el fallo.
#[test]
fn an_already_saved_clip_stops_when_its_source_is_gone() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let saved = fake(&project.join("clip-01.mp4"));
    let gone = tmp.path().join("download/clip.mp4");
    let state = clip_state((1080.0, 1920.0), &saved, &gone, (0.0, None));
    assert_eq!(
        clip_save_target(
            &state.doc,
            state.background_layer,
            &saved,
            false,
            Some(&project)
        ),
        None,
        "sin fuente no hay nada que renderizar: reintentar solo repite el fallo"
    );
}

/// Al revés: si se borró el mp4 del lienzo desde Explorer, el autoguardado lo
/// vuelve a crear (la fuente sigue ahí). `replace_file` cae a `rename` cuando
/// el destino no existe, así que negarse dejaba el lienzo sin guardar.
#[test]
fn an_already_saved_writes_again_when_its_mp4_was_deleted() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let saved = project.join("clip-01.mp4");
    let source = fake(&project.join(".download/clip.mp4"));
    let state = clip_state((1080.0, 1920.0), &saved, &source, (0.0, None));
    assert!(!saved.is_file());
    assert_eq!(
        clip_save_target(
            &state.doc,
            state.background_layer,
            &saved,
            false,
            Some(&project)
        ),
        Some(saved.clone()),
        "el destino desaparecido se recrea"
    );
}

/// Lo que ffmpeg no reproduciría (capa extra, vídeo rotado, recorte,
/// transparencia) no se renderiza a medias: no hay destino automático.
#[test]
fn a_non_renderable_clip_has_no_target() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let source = fake(&project.join("clip.mp4"));
    let slot = project.join("clip-01.mp4");

    let mut extra = clip_canvas(&source);
    extra
        .doc
        .add_layer(
            "Título",
            canvas_core::Transform::new(0.0, 0.0, 300.0, 80.0),
            canvas_core::LayerContent::Shape(canvas_core::ShapeContent {
                kind: canvas_core::ShapeKind::Rect,
                fill: [255, 255, 255, 255],
                stroke: [0, 0, 0, 0],
                stroke_width: 0.0,
                corner_radius: 0.0,
            }),
        )
        .unwrap();
    assert_eq!(
        clip_save_target(
            &extra.doc,
            extra.background_layer,
            &slot,
            true,
            Some(&project)
        ),
        None
    );

    let mut rotated = clip_canvas(&source);
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
    assert_eq!(
        clip_save_target(
            &rotated.doc,
            rotated.background_layer,
            &slot,
            true,
            Some(&project)
        ),
        None
    );
}

/// Un lienzo raster con una capa de vídeo insertada NO es un clip: su guardado
/// sigue siendo el raster de siempre (hornea el póster), no un mp4.
#[test]
fn a_photo_canvas_with_a_video_layer_is_not_a_clip() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let photo = fake(&project.join("foto.png"));
    let source = fake(&project.join("clip.mp4"));
    let mut state = clip_canvas(&source);
    state.doc.source_path = Some(photo.clone());
    assert!(!is_clip_document(&state.doc));
    assert_eq!(
        clip_save_target(
            &state.doc,
            state.background_layer,
            &photo,
            false,
            Some(&project)
        ),
        None
    );
    assert!(is_clip_document(&clip_canvas(&source).doc));
}
