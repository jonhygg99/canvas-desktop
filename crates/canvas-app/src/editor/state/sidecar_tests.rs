//! Tests de integración del lado de restauración del sidecar (`from_restored`):
//! que al restaurar un documento hostil — con ids de capa que chocan con el
//! contador de ids del documento — y pegar después, el pegado NUNCA reutiliza
//! un id ya en uso. Regresión del choque encontrado al reproducir el guard
//! con una capa sin píxeles: la capa hostil llevaba id 4 y el contador del
//! documento restaurado también apuntaba a 4, así que el pegado pisaba a la
//! capa existente y su imagen «sanaba» sin querer el lado hostil.

use std::collections::HashSet;
use std::path::PathBuf;

use canvas_core::{Document, ImageContent, LayerContent, ShapeContent, Transform};
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
