//! Tests del sidecar: roundtrip de imagen, detección de modificación
//! externa, grupos, compatibilidad v3, diseño autónomo, miniatura,
//! papelera local y migración legacy. Extraídos de `mod.rs` para
//! mantenerlo por debajo del objetivo de 400 líneas.

use super::*;
use canvas_core::{ImageContent, LayerContent, Transform};
use payload::encode_payload;

fn sample_doc() -> (Document, Vec<LayerPixels>) {
    let mut doc = Document::new(200.0, 100.0);
    let id = doc
        .add_layer(
            "img",
            Transform::new(25.0, 10.0, 50.0, 40.0),
            LayerContent::Image(ImageContent {
                source_path: None,
                natural_width: 4,
                natural_height: 2,
                crop: None,
            }),
        )
        .unwrap();
    doc.layer_mut(id).unwrap().effects.blur_radius = 7.0;
    let rgba: Vec<u8> = (0..4 * 2 * 4).map(|i| (i * 7 % 256) as u8).collect();
    (doc, vec![(id.raw(), rgba, 4, 2)])
}

fn sample_payload(
    document: &Document,
    images: &[LayerPixels],
    background_layer: Option<u64>,
) -> CanvasPayload {
    CanvasPayload {
        document: document.clone(),
        images: images.to_vec(),
        background_layer,
        preview: None,
    }
}

#[test]
fn roundtrip_restores_document_and_pixels() {
    let dir = tempfile::tempdir().expect("tempdir");
    let image_path = dir.path().join("foto.png");
    let fake_image = b"bytes de la imagen guardada";
    std::fs::write(&image_path, fake_image).unwrap();

    let (doc, images) = sample_doc();
    let payload = sample_payload(&doc, &images, None);
    write_sidecar(&image_path, fake_image, &payload).expect("escribir");
    assert!(sidecar_path(&image_path).exists());

    let restored = read_sidecar(&image_path)
        .expect("leer")
        .expect("hay sidecar");
    assert!(restored.hash_matches);
    assert!(!restored.standalone);
    assert_eq!(restored.document, doc);
    assert_eq!(restored.images.len(), 1);
    let (layer, pixels) = &restored.images[0];
    assert_eq!(*layer, images[0].0);
    assert_eq!((pixels.width, pixels.height), (4, 2));
    assert_eq!(pixels.rgba, images[0].1);
}

#[test]
fn detects_externally_modified_image() {
    let dir = tempfile::tempdir().expect("tempdir");
    let image_path = dir.path().join("foto.png");
    std::fs::write(&image_path, b"original").unwrap();

    let (doc, images) = sample_doc();
    let payload = sample_payload(&doc, &images, None);
    write_sidecar(&image_path, b"original", &payload).expect("escribir");

    // Alguien edita la imagen por fuera.
    std::fs::write(&image_path, b"modificada por otro programa").unwrap();
    let restored = read_sidecar(&image_path)
        .expect("leer")
        .expect("hay sidecar");
    assert!(!restored.hash_matches);
}

#[test]
fn groups_survive_a_sidecar_roundtrip() {
    use canvas_core::Layer;

    let dir = tempfile::tempdir().expect("tempdir");
    let image_path = dir.path().join("foto.png");
    let fake_image = b"bytes de la imagen guardada";
    std::fs::write(&image_path, fake_image).unwrap();

    let (mut doc, images) = sample_doc();
    let child = doc
        .layer(canvas_core::LayerId::from_raw(images[0].0))
        .unwrap()
        .id;
    let group_id = doc.allocate_layer_id();
    {
        let page = doc.page_mut().unwrap();
        page.insert_child(Layer::group(group_id, "Group"), None, 1);
        page.move_subtree(child, Some(group_id), 0).unwrap();
    }

    let payload = sample_payload(&doc, &images, None);
    write_sidecar(&image_path, fake_image, &payload).expect("escribir");
    let restored = read_sidecar(&image_path)
        .expect("leer")
        .expect("hay sidecar");
    assert_eq!(restored.document, doc);
    assert_eq!(
        restored.document.layer(child).unwrap().parent_id,
        Some(group_id)
    );
    assert!(restored.document.page().unwrap().is_group(group_id));
}

#[test]
fn version_probe_rejects_a_newer_sidecar_before_parsing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let image_path = dir.path().join("foto.png");
    std::fs::write(&image_path, b"x").unwrap();

    // Documento con un campo de version futura y contenido que este
    // build no sabria deserializar (para probar que el rechazo ocurre
    // ANTES de intentar parsear `SidecarFile`).
    let fake_future = serde_json::json!({
        "version": SIDECAR_VERSION + 1,
        "algo_que_no_existe_todavia": { "esto": "no es un Document" },
    });
    let path = sidecar_path(&image_path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, serde_json::to_vec(&fake_future).unwrap()).unwrap();

    match read_sidecar(&image_path) {
        Err(crate::IoError::Decode { .. }) => {}
        Err(other) => panic!("se esperaba IoError::Decode, se obtuvo otro error: {other}"),
        Ok(_) => panic!("un sidecar de version futura no deberia leerse con exito"),
    }
}

#[test]
fn missing_sidecar_is_none_and_delete_is_quiet() {
    let dir = tempfile::tempdir().expect("tempdir");
    let image_path = dir.path().join("foto.png");
    std::fs::write(&image_path, b"x").unwrap();
    assert!(read_sidecar(&image_path).expect("leer").is_none());
    delete_sidecar(&image_path); // no explota sin sidecar

    let (doc, images) = sample_doc();
    let payload = sample_payload(&doc, &images, Some(7));
    write_sidecar(&image_path, b"x", &payload).expect("escribir");
    let restored = read_sidecar(&image_path).unwrap().unwrap();
    assert_eq!(restored.background_layer, Some(7));
    delete_sidecar(&image_path);
    assert!(!sidecar_path(&image_path).exists());
}

#[test]
fn design_roundtrips_without_any_image() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("Untitled.canvas");

    let (doc, images) = sample_doc();
    let payload = sample_payload(&doc, &images, None);
    write_design(&path, &payload).expect("escribir diseño");

    let restored = read_design(&path).expect("leer diseño");
    assert!(restored.standalone);
    assert!(restored.hash_matches);
    assert_eq!(restored.document, doc);
    assert_eq!(restored.images.len(), 1);
    assert_eq!(restored.images[0].1.rgba, images[0].1);
}

#[test]
fn design_preview_roundtrips() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("Untitled.canvas");

    let (doc, images) = sample_doc();
    let mut payload = sample_payload(&doc, &images, None);
    let preview_rgba: Vec<u8> = (0..8 * 4 * 4).map(|i| (i * 3 % 256) as u8).collect();
    payload.preview = Some(crate::LoadedImage {
        rgba: preview_rgba.clone(),
        width: 8,
        height: 4,
    });
    write_design(&path, &payload).expect("escribir diseño");

    let preview = read_preview(&path)
        .expect("leer preview")
        .expect("hay preview");
    assert_eq!((preview.width, preview.height), (8, 4));
    assert_eq!(preview.rgba, preview_rgba);

    // Sin miniatura embebida: `Ok(None)`, no error.
    let no_preview_path = dir.path().join("SinPreview.canvas");
    let mut payload_sin = sample_payload(&doc, &images, None);
    payload_sin.preview = None;
    write_design(&no_preview_path, &payload_sin).expect("escribir diseño sin preview");
    assert!(read_preview(&no_preview_path)
        .expect("leer preview")
        .is_none());
}

/// Red de compatibilidad hacia atrás: un sidecar v3 (sin `preview_png`,
/// `image_hash` obligatorio) escrito a mano se sigue leyendo tal cual.
#[test]
fn v3_sidecar_still_restores() {
    let dir = tempfile::tempdir().expect("tempdir");
    let image_path = dir.path().join("foto.png");
    let fake_image = b"bytes de la imagen guardada";
    std::fs::write(&image_path, fake_image).unwrap();

    let (doc, images) = sample_doc();
    let encoded_images: Vec<_> = images
        .iter()
        .map(|(layer, rgba, w, h)| {
            serde_json::json!({
                "layer": layer,
                "png_base64": crate::png_codec::encode_layer_png(
                    rgba,
                    *w,
                    *h,
                    Path::new("test"),
                )
                .unwrap(),
            })
        })
        .collect();
    let v3_json = serde_json::json!({
        "version": 3,
        "image_hash": format!("{:016x}", fnv1a64(fake_image)),
        "background_layer": null,
        "document": doc,
        "images": encoded_images,
    });
    let path = sidecar_path(&image_path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, serde_json::to_vec(&v3_json).unwrap()).unwrap();

    let restored = read_sidecar(&image_path)
        .expect("leer sidecar v3")
        .expect("hay sidecar");
    assert!(!restored.standalone);
    assert!(restored.hash_matches);
    assert_eq!(restored.document, doc);
}

#[test]
fn sidecar_survives_a_missing_companion_image() {
    let dir = tempfile::tempdir().expect("tempdir");
    let image_path = dir.path().join("foto.png");
    let fake_image = b"bytes de la imagen guardada";
    std::fs::write(&image_path, fake_image).unwrap();

    let (doc, images) = sample_doc();
    let payload = sample_payload(&doc, &images, None);
    write_sidecar(&image_path, fake_image, &payload).expect("escribir");

    // Alguien borra la imagen original; el sidecar sigue ahí.
    std::fs::remove_file(&image_path).unwrap();
    let restored = read_sidecar(&image_path)
        .expect("leer")
        .expect("hay sidecar aunque falte la imagen");
    assert!(restored.hash_matches);
    assert!(!restored.standalone);
}

#[test]
fn an_absent_hash_marks_the_file_standalone() {
    let dir = tempfile::tempdir().expect("tempdir");
    let image_path = dir.path().join("foto.png");
    std::fs::write(&image_path, b"imagen real").unwrap();

    // Un `.canvas` con el nombre de sidecar de `foto.png`, pero escrito
    // como diseño autónomo (sin `image_hash`).
    let (doc, images) = sample_doc();
    let payload = sample_payload(&doc, &images, None);
    let path = sidecar_path(&image_path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    write_design(&path, &payload).expect("escribir diseño");

    let restored = read_sidecar(&image_path)
        .expect("leer")
        .expect("hay archivo en la ruta del sidecar");
    assert!(restored.standalone);
    assert!(restored.hash_matches);
}

#[test]
fn write_sidecar_hides_the_dot_canvas_folder_and_never_leaves_it_next_to_the_image() {
    let dir = tempfile::tempdir().expect("tempdir");
    let image_path = dir.path().join("foto.png");
    let fake_image = b"bytes de la imagen guardada";
    std::fs::write(&image_path, fake_image).unwrap();

    let (doc, images) = sample_doc();
    let payload = sample_payload(&doc, &images, None);
    write_sidecar(&image_path, fake_image, &payload).expect("escribir");

    assert!(sidecar_path(&image_path).exists());
    assert_eq!(
        sidecar_path(&image_path).parent().unwrap(),
        sidecar_dir(dir.path())
    );
    assert!(!legacy_sidecar_path(&image_path).exists());
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
        let attrs = std::fs::metadata(sidecar_dir(dir.path()))
            .unwrap()
            .file_attributes();
        assert!(
            attrs & FILE_ATTRIBUTE_HIDDEN != 0,
            "la carpeta debe quedar oculta"
        );
    }
}

#[test]
fn find_sidecar_falls_back_to_the_legacy_sibling() {
    let dir = tempfile::tempdir().expect("tempdir");
    let image_path = dir.path().join("foto.png");
    std::fs::write(&image_path, b"x").unwrap();

    // Sidecar escrito a la manera antigua (hermano directo), sin pasar
    // por `write_sidecar`: simula una carpeta de antes de este cambio.
    let (doc, images) = sample_doc();
    let payload = sample_payload(&doc, &images, None);
    let legacy = legacy_sidecar_path(&image_path);
    let hash = format!("{:016x}", fnv1a64(b"x"));
    let json = encode_payload(&legacy, Some(hash), &payload).unwrap();
    std::fs::write(&legacy, json).unwrap();

    assert_eq!(find_sidecar(&image_path), Some(legacy.clone()));
    let restored = read_sidecar(&image_path)
        .expect("leer")
        .expect("se encuentra el sidecar legacy");
    assert_eq!(restored.document, doc);

    // Guardar de nuevo migra: el legacy desaparece, el nuevo existe.
    write_sidecar(&image_path, b"x", &payload).expect("escribir");
    assert!(
        !legacy.exists(),
        "el sidecar legacy debe borrarse al migrar"
    );
    assert!(sidecar_path(&image_path).exists());
    assert_eq!(find_sidecar(&image_path), Some(sidecar_path(&image_path)));
}

#[test]
fn delete_sidecar_removes_both_locations() {
    let dir = tempfile::tempdir().expect("tempdir");
    let image_path = dir.path().join("foto.png");
    std::fs::write(&image_path, b"x").unwrap();

    std::fs::create_dir_all(sidecar_dir(dir.path())).unwrap();
    std::fs::write(sidecar_path(&image_path), b"{}").unwrap();
    std::fs::write(legacy_sidecar_path(&image_path), b"{}").unwrap();

    delete_sidecar(&image_path);
    assert!(!sidecar_path(&image_path).exists());
    assert!(!legacy_sidecar_path(&image_path).exists());
}

#[test]
fn preview_png_is_only_embedded_for_a_standalone_design() {
    let (doc, images) = sample_doc();
    let mut payload = sample_payload(&doc, &images, None);
    payload.preview = Some(crate::LoadedImage {
        rgba: vec![255u8; 4 * 2 * 4],
        width: 4,
        height: 2,
    });

    // Sidecar de imagen (`image_hash: Some(..)`): sin miniatura embebida.
    let with_hash = encode_payload(
        Path::new("test.canvas"),
        Some("deadbeef".to_owned()),
        &payload,
    )
    .unwrap();
    let file: SidecarFile = serde_json::from_slice(&with_hash).unwrap();
    assert!(file.preview_png.is_none());

    // Diseño autónomo (`image_hash: None`): miniatura embebida.
    let standalone = encode_payload(Path::new("test.canvas"), None, &payload).unwrap();
    let file: SidecarFile = serde_json::from_slice(&standalone).unwrap();
    assert!(file.preview_png.is_some());
}

#[test]
fn write_blank_canvas_png_produces_a_real_image_and_its_sidecar() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("Untitled.png");
    write_blank_canvas(&path, 40.0, 20.0, 92).expect("crear lienzo en blanco");

    let decoded = image::open(&path).expect("el PNG debe ser decodificable");
    assert_eq!((decoded.width(), decoded.height()), (40, 20));

    let restored = read_sidecar(&path)
        .expect("leer sidecar")
        .expect("hay sidecar");
    assert!(!restored.standalone);
    assert!(restored.hash_matches);
    assert_eq!(restored.document.page().unwrap().layers.len(), 0);
}

#[test]
fn write_blank_canvas_dot_canvas_still_makes_a_standalone_design() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("Untitled.canvas");
    write_blank_canvas(&path, 40.0, 20.0, 92).expect("crear diseño en blanco");

    let restored = read_design(&path).expect("leer diseño");
    assert!(restored.standalone);
    assert!(
        !path.with_extension("").is_file(),
        "no debe crear ninguna imagen"
    );
}

#[test]
fn moving_to_local_trash_and_restoring_round_trips_the_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("foto.png");
    std::fs::write(&path, b"pixels").unwrap();

    let staged = move_to_local_trash(&path).expect("mover a la papelera");
    assert!(!path.exists(), "ya no debe estar en su sitio original");
    assert!(staged.exists());
    assert_eq!(staged, trash_dir(dir.path()).join("foto.png"));
    assert_eq!(staged, local_trash_path(&path));

    restore_from_local_trash(&staged, &path).expect("restaurar");
    assert!(path.exists());
    assert!(!staged.exists());
    assert_eq!(std::fs::read(&path).unwrap(), b"pixels");
}

#[test]
fn restoring_refuses_to_overwrite_a_file_that_reappeared_at_the_original_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("foto.png");
    std::fs::write(&path, b"original").unwrap();
    let staged = move_to_local_trash(&path).expect("mover a la papelera");

    // Algo (o alguien) creó un archivo nuevo con el mismo nombre
    // mientras tanto.
    std::fs::write(&path, b"nuevo").unwrap();

    let err = restore_from_local_trash(&staged, &path);
    assert!(err.is_err(), "no debe sobrescribir en silencio");
    assert_eq!(std::fs::read(&path).unwrap(), b"nuevo");
    assert!(staged.exists(), "lo movido sigue a salvo en la papelera");
}

#[test]
fn purge_removes_everything_left_in_the_trash_but_not_the_folder_itself() {
    let dir = tempfile::tempdir().expect("tempdir");
    let a = dir.path().join("a.png");
    let b = dir.path().join("b.png");
    std::fs::write(&a, b"a").unwrap();
    std::fs::write(&b, b"b").unwrap();
    move_to_local_trash(&a).expect("mover a");
    move_to_local_trash(&b).expect("mover b");

    purge_local_trash(dir.path());

    assert!(!local_trash_path(&a).exists());
    assert!(!local_trash_path(&b).exists());
}

#[test]
fn purge_of_a_folder_with_no_trash_yet_does_not_panic() {
    let dir = tempfile::tempdir().expect("tempdir");
    purge_local_trash(dir.path());
}
