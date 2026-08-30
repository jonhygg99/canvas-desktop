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
    let (json, _) = container::split_container(&with_hash, Path::new("test.canvas")).unwrap();
    let file: SidecarFile = serde_json::from_slice(json).unwrap();
    assert!(file.preview_png.is_none());

    // Diseño autónomo (`image_hash: None`): miniatura embebida.
    let standalone = encode_payload(Path::new("test.canvas"), None, &payload).unwrap();
    let (json, _) = container::split_container(&standalone, Path::new("test.canvas")).unwrap();
    let file: SidecarFile = serde_json::from_slice(json).unwrap();
    assert!(file.preview_png.is_some());
}

#[test]
fn v5_container_keeps_pixel_blobs_out_of_the_json_header() {
    let (doc, images) = sample_doc();
    let payload = sample_payload(&doc, &images, None);
    let bytes = encode_payload(
        Path::new("test.canvas"),
        Some("deadbeef".to_owned()),
        &payload,
    )
    .expect("encode payload");

    // Es un contenedor (mágica) y el JSON de cabecera ya no lleva los
    // píxeles en base64: solo índices de blob.
    assert!(container::is_container(&bytes), "debe empezar por CANVAS5");
    let (json, blobs) =
        container::split_container(&bytes, Path::new("test.canvas")).expect("contenedor válido");
    let file: SidecarFile = serde_json::from_slice(json).unwrap();
    assert_eq!(file.images.len(), images.len());
    assert!(json
        .windows(b"png_base64".len())
        .all(|w| w != b"png_base64"));
    assert_eq!(blobs.len(), images.len());

    // Los blobs son PNG decodificables de vuelta a los píxeles originales.
    for (blob, (_, rgba, w, h)) in blobs.iter().zip(&images) {
        let (decoded, dw, dh) =
            crate::png_codec::decode_png_bytes(blob, Path::new("test")).unwrap();
        assert_eq!((dw, dh), (*w, *h));
        assert_eq!(decoded, *rgba);
    }

    // Y el roundtrip completo restaura el documento con sus píxeles.
    let dir = tempfile::tempdir().expect("tempdir");
    let image_path = dir.path().join("foto.png");
    std::fs::write(&image_path, b"x").unwrap();
    write_sidecar(&image_path, b"x", &payload).expect("escribir sidecar v5");
    let restored = read_sidecar(&image_path)
        .expect("leer")
        .expect("hay sidecar");
    assert!(restored.hash_matches);
    assert_eq!(restored.images.len(), images.len());
}

#[test]
fn v5_container_rejects_truncated_blobs() {
    let (doc, images) = sample_doc();
    let payload = sample_payload(&doc, &images, None);
    let bytes = encode_payload(
        Path::new("test.canvas"),
        Some("deadbeef".to_owned()),
        &payload,
    )
    .expect("encode payload");

    // Cortado a mitad de un blob: el split falla, no pánico ni basura.
    let truncated = &bytes[..bytes.len() - 3];
    assert!(container::split_container(truncated, Path::new("test.canvas")).is_err());
}

/// Escribe `bytes` como diseño autónomo y exige que la lectura falle con
/// `IoError::Decode` — un `.canvas` corrompido en disco es un error limpio,
/// nunca un pánico, un cuelgue ni una lectura "exitosa" con basura.
fn write_and_expect_decode_error(bytes: &[u8]) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("corrupt.canvas");
    std::fs::write(&path, bytes).unwrap();
    match read_design(&path) {
        Err(crate::IoError::Decode { .. }) => {}
        Err(_) => panic!("error de otro tipo; se esperaba IoError::Decode"),
        Ok(_) => panic!("un contenedor corrupto no debe leerse con éxito"),
    }
}

#[test]
fn corrupted_v5_containers_fail_as_clean_decode_errors() {
    let (doc, images) = sample_doc();
    let payload = sample_payload(&doc, &images, None);
    let good = encode_payload(Path::new("test.canvas"), None, &payload).expect("encode");
    // Diseño autónomo => la cabecera lleva la miniatura; los blobs empiezan
    // tras mágica (7) + json_len (8) + json.
    let mut len_bytes = [0u8; 8];
    len_bytes.copy_from_slice(&good[7..15]);
    let json_len = u64::from_le_bytes(len_bytes) as usize;
    let blob_start = 7 + 8 + json_len;

    // a) `json_len` miente: apunta más allá del final del archivo.
    let mut bytes = good.clone();
    bytes[7..15].copy_from_slice(&u64::MAX.to_le_bytes());
    write_and_expect_decode_error(&bytes);

    // b) `blob_len` miente: el blob promete más bytes de los que hay.
    let mut bytes = good.clone();
    bytes[blob_start..blob_start + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    write_and_expect_decode_error(&bytes);

    // c) píxeles triturados: el PNG del blob ya no decodifica (CRC/zlib).
    let mut bytes = good.clone();
    for b in bytes[blob_start + 4..].iter_mut() {
        *b = 0xFF;
    }
    write_and_expect_decode_error(&bytes);

    // d) mágica correcta pero cabecera JSON podrida.
    let mut bytes = good.clone();
    bytes[15..23].copy_from_slice(b"not json");
    write_and_expect_decode_error(&bytes);

    // e) contenedor truncado a mitad del último blob.
    write_and_expect_decode_error(&good[..good.len() - 5]);
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

// ---- Corpus adversarial del parser de .canvas ----

/// PRNG mínimo y determinista para los corpus (misma idea que `XorShift` en
/// canvas-core): suficiente para mutar bytes, reproducible por semilla y sin
/// dependencias nuevas.
struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Self {
        Self(seed | 1)
    }
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }
}

/// El `.canvas` es el ÚNICO formato que la app abre por doble clic sin que
/// nadie le haya dicho nada sobre su contenido, así que su parser tiene que
/// sobrevivir a bytes hostiles. Tomamos un contenedor v5 válido y le
/// aplicamos mutaciones pseudoaleatorias (bits volcados, truncados,
/// longitudes de cabecera/blob corrompidas): el contrato es `Err` limpio
/// (IoError::Decode) o lectura correcta — JAMÁS un pánico ni un alloc
/// desbocado. Cargo-fuzz daría cobertura de ramas mayor; este corpus
/// determinista cubre el grueso sin nightly ni dependencias.
#[test]
fn mutated_containers_never_panic_and_fail_as_clean_errors() {
    let (doc, images) = sample_doc();
    let payload = sample_payload(&doc, &images, None);
    let good = encode_payload(Path::new("fuzz.canvas"), None, &payload).expect("encode");
    assert!(container::is_container(&good));

    let mut rng = Lcg::new(0xF00D);
    for _ in 0..2000 {
        let mut bytes = good.clone();
        for _ in 0..1 + rng.below(12) {
            match rng.below(4) {
                0 if !bytes.is_empty() => {
                    let i = rng.below(bytes.len());
                    bytes[i] ^= 1 << rng.below(8);
                }
                1 => {
                    let cut = rng.below(bytes.len() + 1);
                    bytes.truncate(cut);
                }
                2 if bytes.len() > 15 => {
                    // Corromper el json_len de la cabecera fija (bytes 7..15).
                    let i = 7 + rng.below(8);
                    bytes[i] ^= 0xFF;
                }
                _ if bytes.len() > 20 => {
                    // Corromper un blob_len o bytes de blob en la sección binaria.
                    let i = 15 + rng.below(bytes.len() - 15);
                    bytes[i] ^= 0xFF;
                }
                _ => {}
            }
            if bytes.is_empty() {
                break;
            }
            // Ok parseable o Err limpio — nunca pánico.
            let _ = container::split_container(&bytes, Path::new("fuzz.canvas"));
        }
    }
}

/// Igual que el corpus de arriba pero por el camino COMPLETO de lectura
/// (`read_design`: contenedor → JSON de cabecera → decodificado de blobs
/// PNG bajo `image::Limits`), escribiendo en disco para ejercitar también
/// la I/O. Menos iteraciones: toca disco en cada una.
#[test]
fn mutated_design_files_never_panic_when_read_back() {
    let (doc, images) = sample_doc();
    let payload = sample_payload(&doc, &images, None);
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("fuzz.canvas");
    let mut rng = Lcg::new(0x0BAD_C0DE);
    for _ in 0..64 {
        let mut bytes = encode_payload(&path, None, &payload).expect("encode");
        for _ in 0..1 + rng.below(16) {
            let i = rng.below(bytes.len());
            bytes[i] ^= 1 << rng.below(8);
        }
        std::fs::write(&path, &bytes).expect("escribir");
        let _ = read_design(&path);
    }
}
