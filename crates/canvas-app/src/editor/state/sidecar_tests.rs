//! Tests de integración del lado de restauración del sidecar (`from_restored`):
//! que al restaurar un documento hostil — con ids de capa que chocan con el
//! contador de ids del documento — y pegar después, el pegado NUNCA reutiliza
//! un id ya en uso. Regresión del choque encontrado al reproducir el guard
//! con una capa sin píxeles: la capa hostil llevaba id 4 y el contador del
//! documento restaurado también apuntaba a 4, así que el pegado pisaba a la
//! capa existente y su imagen «sanaba» sin querer el lado hostil.

use std::collections::HashSet;
use std::path::PathBuf;

use canvas_core::{Document, ImageContent, LayerContent, LayerId, ShapeContent, Transform};
use canvas_io::{LoadedImage, RestoredDocument};
use serde_json::json;

use super::EditorState;

fn loaded_image(width: u32, height: u32) -> LoadedImage {
    LoadedImage {
        rgba: vec![0u8; (width * height * 4) as usize],
        width,
        height,
    }
}

/// Documento con tres capas (`ids` 1,2,3) cuyo `next_layer_id` se fuerza a 1
/// vía serde JSON, como haría un sidecar hostil o un documento ajeno cuyo
/// contador quedó desalineado con sus ids.
fn hostile_document() -> Document {
    let mut src = Document::new(800.0, 600.0);
    let t = |x: f64| Transform::new(x, 0.0, 10.0, 10.0);
    let shape = || LayerContent::Shape(ShapeContent::default());
    src.add_layer("a", t(0.0), shape()).unwrap();
    src.add_layer("b", t(10.0), shape()).unwrap();
    src.add_layer("c", t(20.0), shape()).unwrap();
    let mut value = serde_json::to_value(&src).unwrap();
    value["next_layer_id"] = json!(1);
    serde_json::from_value(value).unwrap()
}

/// Restaurar un documento hostil y pegar una imagen encima no debe
/// reutilizar un id de capa existente: antes del fix, allocate_layer_id
/// devolvía 1 (o el primer id libre del contador), que ya era el de «a».
#[test]
fn pasting_over_a_restored_hostile_document_keeps_all_layer_ids_unique() {
    let restored = RestoredDocument {
        document: hostile_document(),
        images: Vec::new(),
        background_layer: None,
        hash_matches: true,
        standalone: false,
    };
    let mut state = EditorState::from_restored(PathBuf::from("/tmp/hostil.png"), restored);

    // El pegado (add_image_layer simula Ctrl+V) reserva un id NUEVO.
    state.add_image_layer("Pasted Image", None, loaded_image(300, 400));

    // Todos los ids del documento son distintos entre sí y del pegado.
    let ids: Vec<u64> = state
        .doc
        .page()
        .unwrap()
        .layers
        .iter()
        .map(|l| l.id.raw())
        .collect();
    let mut seen = HashSet::new();
    for id in &ids {
        assert!(
            seen.insert(*id),
            "id {id} duplicado en {ids:?}: pegar sobre un doc hostil pisó una capa existente"
        );
    }
    assert_eq!(
        ids.len(),
        4,
        "el documento restaurado (3 capas) + el pegado (1) deben convivir"
    );

    // La capa nueva es seleccionable por su id y está conectada a sus píxeles:
    // no comparte id con ninguna anterior (si compartiera, la selección o el
    // guardado del lado hostil lo pisarían).
    let pasted = state.selection.primary().unwrap();
    assert!(state.images.contains_key(&pasted));
    assert_eq!(state.doc.layer(pasted).unwrap().name, "Pasted Image");
}

/// Un documento con una sola capa de imagen (la foto), listo para editar.
fn state_with_photo() -> EditorState {
    let mut doc = Document::new(800.0, 600.0);
    let id = doc
        .add_layer(
            "foto",
            Transform::new(0.0, 0.0, 800.0, 600.0),
            LayerContent::Image(ImageContent {
                source_path: None,
                natural_width: 800,
                natural_height: 600,
                crop: None,
            }),
        )
        .unwrap();
    let restored = RestoredDocument {
        document: doc,
        images: vec![(id.raw(), loaded_image(800, 600))],
        background_layer: None,
        hash_matches: true,
        standalone: false,
    };
    EditorState::from_restored(PathBuf::from("/tmp/foto.png"), restored)
}

/// Reproduce el flujo real del incidente `1.png` paso a paso y verifica el
/// misterio de fondo: **abrir una foto de galería** (`from_image`: la foto es
/// una capa fuente normal), **activar el `Blurred background`** (se cuela
/// debajo de la foto), **pegar n capas por encima** y **guardar**
/// (`sidecar_payload`).
///
/// Contratos que este flujo debe mantener, y que el incidente violó (la foto
/// acabó solo en `images` sin entrada en `layers`): la capa fuente sigue viva
/// en `doc.layers` tras pegar y guardar y su blob se embebe, y ningún blob
/// del sidecar queda huérfano (id sin capa).
///
/// Si este test pasa, el fallo del `1.png` no vino del flujo limpio
/// abrir→blur→pegar→guardar, sino de alguna acción adicional (deshacer,
/// cortar, reemplazar, alternar el fondo). Aun sin confirmar el flujo limpio,
/// este test lo convierte en contrato.
#[test]
fn open_blur_paste_save_keeps_source_photo_alive() {
    let mut state = state_with_photo(); // from_image: la foto es capa 1 (fuente)
    let photo_raw = state.doc.page().unwrap().layers[0].id.raw();

    // Blurred background: capa cover con blur 50 insertada en el fondo.
    state.set_blurred_background(true);
    assert!(
        state.doc.page().unwrap().layers.len() == 2,
        "abrir foto + blur = 2 capas, había {}",
        state.doc.page().unwrap().layers.len()
    );

    // Pegar n capas por encima (Ctrl+V de imágenes del sistema).
    for i in 0..5 {
        state.add_image_layer(format!("Pasted Image {i}"), None, loaded_image(40, 30));
    }

    let payload = state.sidecar_payload(); // el guardado
    let layers: Vec<canvas_core::Layer> = payload.document.page().unwrap().layers.clone();
    let live: HashSet<u64> = layers.iter().map(|l| l.id.raw()).collect();
    let image_ids: Vec<u64> = payload.images.iter().map(|(id, ..)| *id).collect();

    assert_eq!(
        layers.len(),
        7,
        "foto + blur + 5 pegadas = 7 capas, había {}",
        layers.len()
    );
    assert!(
        layers.iter().any(|l| l.id.raw() == photo_raw),
        "la capa fuente de la foto (id {photo_raw}) desapareció de doc.layers tras pegar y guardar"
    );
    assert!(
        image_ids.contains(&photo_raw),
        "el blob de la foto no está en el sidecar"
    );
    // Ningún blob huérfano: cada imagen del sidecar tiene su capa.
    for id in &image_ids {
        assert!(
            live.contains(id),
            "blob huérfano en el sidecar (id {id} sin capa en el documento)"
        );
    }
    assert_eq!(
        image_ids.len(),
        live.len(),
        "el sidecar debe contener exactamente las {} capas vivas ({} imágenes)",
        live.len(),
        image_ids.len()
    );
}

/// La vía destructiva que SÍ reproduce la pérdida del `1.png`: con el fondo
/// desenfocado activo, borrar la capa fuente (seleccionar la foto + Delete,
/// `delete_selected`) la retira de `doc.layers` pero deja su blob en `images`
/// para deshacer dentro de la MISMA sesión. Guardar en ese estado no debe
/// persistir el huérfano: el sidecar solo embebe las capas vivas (el blur,
/// no la foto borrada). Confirma las dos mitades del misterio: el borrado es
/// donde se pierde la capa, y el guardado ya no congela el blob huérfano.
#[test]
fn delete_source_photo_between_blur_and_save_does_not_persist_orphan() {
    let mut state = state_with_photo();
    state.set_blurred_background(true);
    let photo_raw = state.selection.primary().unwrap().raw(); // la foto queda seleccionada
    let blur_raw = state.doc.page().unwrap().layers[0].id.raw(); // el blur al fondo

    // Acción destructiva: seleccionar la foto + Delete.
    crate::editor::delete_selected(&mut state);

    let layer_ids: Vec<u64> = state
        .doc
        .page()
        .unwrap()
        .layers
        .iter()
        .map(|l| l.id.raw())
        .collect();
    assert!(
        !layer_ids.contains(&photo_raw),
        "la foto debe quedar fuera de doc.layers tras el borrado (es la pérdida del 1.png)"
    );
    assert!(
        layer_ids.contains(&blur_raw),
        "el blur de fondo debe seguir vivo tras borrar la foto"
    );

    let payload = state.sidecar_payload();
    let live: HashSet<u64> = payload
        .document
        .page()
        .unwrap()
        .layers
        .iter()
        .map(|l| l.id.raw())
        .collect();
    let image_ids: Vec<u64> = payload.images.iter().map(|(id, ..)| *id).collect();

    // Guardado limpio: el blob de la foto borrada NO se persiste (sería un
    // huérfano), y ningún blob del sidecar queda sin su capa viva.
    assert!(
        !image_ids.contains(&photo_raw),
        "el sidecar no debe persistir el blob de la capa borrada (id {photo_raw})"
    );
    for id in &image_ids {
        assert!(live.contains(id), "blob huérfano persistido (id {id})");
    }
    assert_eq!(
        image_ids.len(),
        live.len(),
        "el sidecar debe contener exactamente las capas vivas"
    );
    assert!(image_ids.contains(&blur_raw), "el blur debe embeberse");
}

/// End-to-end a través de la serialización REAL de disco: borrar la foto
/// (`delete_selected`), guardar (payload → `write_sidecar`) y recargar
/// (`open_document` → `from_restored`) deben producir un diseño COHERENTE:
/// la foto borrada no reaparece (ni como capa ni como blob fantasma), el
/// fondo desenfocado sobrevive, y toda capa viva conserva sus píxeles. Es el
/// round-trip que el fix del serializador garantiza — antes, el blob huérfano
/// de la foto borrada se escribía y recargaba como una imagen sin capa.
#[test]
fn reload_after_deleting_photo_is_coherent() {
    let mut state = state_with_photo();
    state.set_blurred_background(true);
    let photo_raw = state.selection.primary().unwrap().raw();
    let blur_raw = state.doc.page().unwrap().layers[0].id.raw();

    // Acción destructiva: borrar la foto dejando su blob en `images` (para
    // deshacer en esta misma sesión).
    crate::editor::delete_selected(&mut state);

    // Guardar DE VERDAD: PNG base + sidecar v5 en disco, con el hash del PNG
    // exactamente como hace `spawn_save`.
    let dir = std::env::temp_dir().join(format!("canvas_sidecar_reload_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let png = dir.join("foto.png");
    let png_bytes = canvas_io::save_rgba(&png, vec![255u8; 4 * 4 * 4], 4, 4, 100, None).unwrap();
    let payload = state.sidecar_payload();
    canvas_io::write_sidecar(&png, &png_bytes, &payload).unwrap();

    // Recargar por el cargador real (no por el mapa en memoria).
    let canvas_io::OpenOutcome::Restored(restored) = canvas_io::open_document(&png, true).unwrap()
    else {
        panic!("se esperaba Restored para un sidecar válido");
    };
    let reopened = EditorState::from_restored(png.clone(), restored);

    let layers: Vec<canvas_core::Layer> = reopened.doc.page().unwrap().layers.clone();
    // Sin capas perdidas ni fantasmas: solo el blur, nada de la foto borrada.
    assert_eq!(
        layers.len(),
        1,
        "debe quedar solo el blur, había {}",
        layers.len()
    );
    assert_eq!(
        layers[0].id.raw(),
        blur_raw,
        "la única capa viva debe ser el fondo desenfocado"
    );
    assert!(
        layers.iter().all(|l| l.id.raw() != photo_raw),
        "la foto borrada no debe reaparecer como capa tras recargar"
    );
    assert_eq!(
        reopened.background_layer,
        Some(LayerId::from_raw(blur_raw)),
        "el fondo desenfocado debe seguir marcado"
    );
    // Sin blob fantasma y con píxeles para cada capa viva.
    for l in &layers {
        assert!(
            reopened.images.contains_key(&l.id),
            "la capa viva {:?} perdió sus píxeles al recargar",
            l.id
        );
    }
    assert_eq!(
        reopened.images.len(),
        layers.len(),
        "no debe haber imágenes fantasma (sin capa) ni capas sin imagen"
    );
    assert!(
        !reopened.images.contains_key(&LayerId::from_raw(photo_raw)),
        "el blob de la foto borrada no debe sobrevivir como imagen fantasma"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// El sidecar NO debe embeder imágenes de capas que ya no existen en el
/// documento. Apagar el `Blurred background` retira la capa pero conserva
/// sus píxeles en `images` por si se deshace DENTRO de la misma sesión;
/// guardar en ese estado serializaba un blob huérfano (imagen en `images`
/// sin entrada en `layers`) — la inconsistencia detrás de los diseños
/// `14.png`/`1.png` que obligaron a crear `recover_design`. Sin historial
/// persistido, ese blob no sirve de nada tras recargar.
#[test]
fn sidecar_payload_excludes_orphaned_layer_images() {
    let mut state = state_with_photo();

    // Fondo desenfocado activo: capa viva + su blob; el payload embebe ambos.
    state.set_blurred_background(true);
    let on = state.sidecar_payload();
    assert_eq!(on.images.len(), 2, "foto + blur deben embeberse");
    assert!(on
        .document
        .page()
        .unwrap()
        .layers
        .iter()
        .any(|l| l.name == "Blurred background"));

    // Desactivar el fondo retira la capa del documento pero DEJA sus píxeles
    // en `images` (deshacer en sesión). El sidecar no debe serializarlos.
    state.set_blurred_background(false);
    let off = state.sidecar_payload();
    let live: HashSet<u64> = off
        .document
        .page()
        .unwrap()
        .layers
        .iter()
        .map(|l| l.id.raw())
        .collect();
    assert_eq!(live.len(), 1, "la foto sigue siendo la única capa viva");
    for (img_id, ..) in &off.images {
        assert!(
            live.contains(img_id),
            "sidecar con imagen huérfana (id {img_id} sin capa en el documento)"
        );
    }
    assert_eq!(
        off.images.len(),
        live.len(),
        "el sidecar debe contener exactamente las capas vivas"
    );
}
