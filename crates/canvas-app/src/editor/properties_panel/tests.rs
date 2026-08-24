//! Tests del panel de propiedades. Extraídos de `mod.rs` para mantener el
//! archivo de código por debajo del objetivo de 400 líneas (mismo patrón que
//! `deck/tests.rs`). Cubren: commit de edición colgada al cambiar de
//! selección, pegado con/sin fondo desenfocado, reemplazo de imagen,
//! deshacer borrado, deshacer creación, y deshacer/rehacer global cruzado
//! entre diseños de la baraja.

use std::path::PathBuf;

use canvas_core::{ImageContent, LayerContent, ShapeContent, Transform};
use canvas_io::LoadedImage;

use super::super::{DeleteRecord, GlobalStep};
use super::*;

fn loaded_image(width: u32, height: u32) -> LoadedImage {
    LoadedImage {
        rgba: vec![0u8; (width * height * 4) as usize],
        width,
        height,
    }
}

#[test]
fn switching_selection_mid_edit_commits_the_pending_change_instead_of_leaking_it() {
    let mut state = EditorState::new_blank(500.0, 500.0);
    state.insert_layer_centered(
        "Text",
        200.0,
        80.0,
        LayerContent::Text(canvas_core::TextContent::default()),
    );
    let text_id = state.selection.primary().unwrap();
    let original = state.doc.layer(text_id).unwrap().content.clone();

    // Simula una edición de texto a medias: el control ya mutó el
    // documento en vivo (como hace `content_properties_ui` en cada
    // tecla), pero el usuario no soltó el foco del `TextEdit` todavía.
    state.content_edit = Some((text_id, original.clone()));
    if let LayerContent::Text(t) = &mut state.doc.layer_mut(text_id).unwrap().content {
        t.text = "edited mid-flight".to_owned();
    }

    // Selecciona otra capa SIN soltar el foco antes: en la UI real esto
    // hace que `content_properties_ui` deje de dibujarse para `text_id`
    // y `lost_focus()` nunca vuelva a disparar para ese control.
    state.insert_layer_centered(
        "Shape",
        50.0,
        50.0,
        LayerContent::Shape(canvas_core::ShapeContent::default()),
    );
    assert_ne!(state.selection.primary(), Some(text_id));
    assert!(
        state.content_edit.is_some(),
        "sigue colgado hasta que se reconcilie"
    );

    commit_stale_panel_edits(&mut state);

    assert!(
        state.content_edit.is_none(),
        "la edición colgada debe consolidarse, no seguir viva"
    );

    // El commit tardío debe haber quedado como su propio paso de
    // deshacer: un solo Ctrl+Z basta para devolver el texto a su valor
    // original, sin tocar la capa "Shape".
    state.undo();
    let restored = state.doc.layer(text_id).unwrap().content.clone();
    assert_eq!(restored, original);
    assert!(state.doc.layer(state.selection.primary().unwrap()).is_ok());
}

#[test]
fn pasting_into_an_empty_canvas_expands_and_adds_a_blurred_background() {
    let mut state = EditorState::new_blank(1080.0, 1080.0);
    state.add_image_layer("Pasted Image", None, loaded_image(540, 960));

    let page = state.doc.page().unwrap();
    assert_eq!(page.layers.len(), 2);

    // Fondo: al fondo de la pila, cubre la página entera y desenfocado.
    let bg = &page.layers[0];
    assert_eq!(bg.name, "Blurred background");
    assert_eq!(bg.effects.blur_radius, 50.0);
    assert!(bg.transform.width >= page.width - 1e-6);
    assert!(bg.transform.height >= page.height - 1e-6);
    assert_eq!(Some(bg.id), state.background_layer);

    // Imagen: se amplía hasta tocar el alto (9:16 en página 1:1), centrada.
    let fg = &page.layers[1];
    assert!((fg.transform.height - page.height).abs() < 1e-6);
    assert!(fg.transform.width < page.width);
    assert!((fg.transform.x - (page.width - fg.transform.width) / 2.0).abs() < 1e-6);
    assert_eq!(state.selection.primary(), Some(fg.id));

    // Un solo Ctrl+Z deja el lienzo vacío otra vez (imagen + fondo).
    state.undo();
    assert!(state.doc.page().unwrap().layers.is_empty());
    assert!(state.selection.primary().is_none());
}

#[test]
fn pasting_a_square_image_into_a_matching_square_canvas_skips_the_background() {
    let mut state = EditorState::new_blank(1000.0, 1000.0);
    state.add_image_layer("Pasted Image", None, loaded_image(500, 500));

    let page = state.doc.page().unwrap();
    assert_eq!(page.layers.len(), 1);
    assert_eq!(state.background_layer, None);
    let fg = &page.layers[0];
    assert_eq!((fg.transform.width, fg.transform.height), (1000.0, 1000.0));
}

#[test]
fn replacing_image_preserves_transform_and_undo_restores_the_old_layer() {
    let mut state = EditorState::new_blank(1000.0, 1000.0);
    state.add_image_layer("Original", None, loaded_image(500, 500));
    let original_layer = state.doc.page().unwrap().layers[0].clone();

    state
        .replace_image_layer(original_layer.id, None, loaded_image(250, 500))
        .unwrap();

    let page = state.doc.page().unwrap();
    assert_eq!(page.layers.len(), 1);
    let replaced = &page.layers[0];
    assert_ne!(replaced.id, original_layer.id);
    assert_eq!(replaced.name, original_layer.name);
    assert_eq!(replaced.transform, original_layer.transform);
    assert_eq!(state.selection.primary(), Some(replaced.id));
    match &replaced.content {
        LayerContent::Image(content) => {
            assert_eq!((content.natural_width, content.natural_height), (250, 500));
            assert_eq!(content.crop, None);
        }
        _ => panic!("replacement must stay an image layer"),
    }

    state.undo();

    let page = state.doc.page().unwrap();
    assert_eq!(page.layers.len(), 1);
    let restored = &page.layers[0];
    assert_eq!(restored.id, original_layer.id);
    assert_eq!(restored.transform, original_layer.transform);
    match &restored.content {
        LayerContent::Image(content) => {
            assert_eq!((content.natural_width, content.natural_height), (500, 500));
        }
        _ => panic!("undo must restore the original image layer"),
    }
    assert!(state.images.contains_key(&original_layer.id));
}

/// Deshacer un borrado de archivo real (`GlobalStep::Delete`, apilado
/// vía `record_delete` tras un «Delete» del usuario) no pertenece a
/// ninguna ranura: se resuelve de inmediato — dejando la restauración
/// pedida en `pending_restore` — sin esperar ningún salto de baraja, y
/// sin dejar rastro en `global_redo` (no se "rehace" volver a borrar).
#[test]
fn undoing_a_delete_step_requests_a_restore_without_touching_redo() {
    let mut state = EditorState::new_blank(10.0, 10.0);
    state.pending_creation = false; // no es lo que se está probando aquí
    let record = DeleteRecord {
        path: PathBuf::from("C:/photos/cat.png"),
        sidecar: Some(PathBuf::from("C:/photos/.canvas/cat.png.canvas")),
    };
    state.record_delete(record.clone());
    assert!(state.can_undo());

    state.undo();

    assert_eq!(state.pending_restore, Some(record));
    assert!(!state.can_undo(), "el paso de borrado ya se resolvió");
    assert!(
        !state.can_redo(),
        "deshacer un borrado no deja nada que rehacer"
    );
}

/// El borrado que dispara `finish_pending_global_undo` al deshacer una
/// creación (`GlobalStep::Create`) queda marcado como "no venía de un
/// clic del usuario" (`pending_delete_from_undo`) — es la señal que lee
/// `main.rs` para NO apilarle a su vez un `GlobalStep::Delete`: si lo
/// hiciera, un `Ctrl+Z` más adelante podría "deshacer el deshacer" y
/// restaurar un lienzo que el propio usuario decidió descartar.
#[test]
fn finishing_a_pending_create_undo_marks_the_delete_as_not_user_initiated() {
    let mut state = EditorState::new_blank(10.0, 10.0);
    state.active_slot_id = 1;
    state.pending_creation = false;
    state.pending_global_undo = Some(GlobalStep::Create(1));

    assert!(!state.pending_delete_from_undo);
    state.finish_pending_global_undo();

    assert!(state.delete_requested);
    assert!(state.pending_delete_from_undo);
}

/// Un lienzo recién creado (`new_blank`) nace con `pending_creation`
/// activo y SIN nada que deshacer todavía: hasta que no se edita de
/// verdad, "crear" no es un paso — evita que un relleno automático de la
/// baraja que nadie llega a tocar se cuele como un paso de deshacer
/// fantasma (la causa del bug: registrar la creación en cada sitio
/// donde podía aparecer una ranura, en vez de en su primera edición
/// real, dejaba huecos y duplicados según la carrera entre clics del
/// usuario y el relleno asíncrono).
#[test]
fn a_freshly_created_canvas_has_nothing_to_undo_until_its_first_edit() {
    let state = EditorState::new_blank(10.0, 10.0);
    assert!(state.pending_creation);
    assert!(!state.can_undo());
}

/// La primera edición real de un lienzo recién creado antepone su
/// `GlobalStep::Create` en la pila global: dos `Ctrl+Z` deshacen esa
/// UNA edición y LUEGO piden borrar la ranura entera — no las dos
/// cosas de golpe con un solo `Ctrl+Z` (el bug reportado).
#[test]
fn first_edit_of_a_freshly_created_canvas_records_its_creation_too() {
    let mut state = EditorState::new_blank(10.0, 10.0);
    let id = state
        .doc
        .add_layer(
            "a",
            Transform::new(0.0, 0.0, 1.0, 1.0),
            LayerContent::Image(ImageContent {
                source_path: None,
                natural_width: 1,
                natural_height: 1,
                crop: None,
            }),
        )
        .unwrap();
    state.active_slot_id = 2;

    state.doc.layer_mut(id).unwrap().name = "b".to_string();
    state.push_undo_step(Box::new(canvas_core::Rename {
        layer: id,
        before: "a".to_string(),
        after: "b".to_string(),
    }));
    assert!(
        !state.pending_creation,
        "se consume en la primera edición real"
    );

    // Ctrl+Z #1: deshace SOLO la edición; la ranura sigue viva.
    state.undo();
    assert!(state.pending_global_undo.is_none());
    assert_eq!(state.doc.layer(id).unwrap().name, "a");
    assert!(!state.delete_requested);
    assert!(
        state.can_undo(),
        "todavía queda el paso de creación por deshacer"
    );

    // Ctrl+Z #2: ahora sí, pide borrar la ranura entera.
    state.undo();
    assert_eq!(state.pending_global_undo, Some(GlobalStep::Create(2)));
    state.finish_pending_global_undo();
    assert!(
        state.delete_requested,
        "debe pedir borrar la ranura recién creada"
    );
    assert!(
        !state.can_undo(),
        "crear no deja nada más que deshacer detrás"
    );
}

/// Simula "editar el diseño 1, luego el 3, luego el 1 otra vez" con
/// `active_slot_id` (una sola `EditorState`/`Document` de sobra para
/// probar el ORDEN cruzado — el salto real de baraja lo cubre
/// `deck::apply_jump` por separado). Comprueba que deshacer tres veces
/// reproduce el orden cronológico real (1, 3, 1) pidiendo el salto
/// correspondiente cada vez que le toca a un diseño que no es el
/// activo, y que rehacer reconstruye el mismo cruce en sentido inverso.
#[test]
fn global_undo_and_redo_replay_steps_in_true_chronological_order_across_designs() {
    let mut state = EditorState::new_blank(10.0, 10.0);
    let id = state
        .doc
        .add_layer(
            "a",
            Transform::new(0.0, 0.0, 1.0, 1.0),
            LayerContent::Image(ImageContent {
                source_path: None,
                natural_width: 1,
                natural_height: 1,
                crop: None,
            }),
        )
        .unwrap();
    // Este test es sobre el orden entre `Edit`, no sobre `Create` (ya
    // cubierto aparte) — se apaga para no mezclar ambos.
    state.pending_creation = false;
    let rename = |state: &mut EditorState, before: &str, after: &str| {
        state.doc.layer_mut(id).unwrap().name = after.to_string();
        state.push_undo_step(Box::new(canvas_core::Rename {
            layer: id,
            before: before.to_string(),
            after: after.to_string(),
        }));
    };
    let name = |state: &EditorState| state.doc.layer(id).unwrap().name.clone();

    state.active_slot_id = 1;
    rename(&mut state, "a", "b"); // diseño 1
    state.active_slot_id = 3;
    rename(&mut state, "b", "c"); // diseño 3
    state.active_slot_id = 1;
    rename(&mut state, "c", "d"); // diseño 1 otra vez

    // El paso más reciente es del diseño activo (1): deshace en el sitio.
    state.undo();
    assert!(state.pending_global_undo.is_none());
    assert_eq!(name(&state), "c");

    // El siguiente le toca al diseño 3: pide el salto SIN tocar el
    // documento todavía.
    state.undo();
    assert_eq!(state.pending_global_undo, Some(GlobalStep::Edit(3)));
    assert_eq!(name(&state), "c");

    // `main.rs` completó el salto de baraja al diseño 3.
    state.active_slot_id = 3;
    state.finish_pending_global_undo();
    assert_eq!(name(&state), "b");

    // Queda el primer paso, del diseño 1: pide saltar de vuelta.
    state.undo();
    assert_eq!(state.pending_global_undo, Some(GlobalStep::Edit(1)));
    state.active_slot_id = 1;
    state.finish_pending_global_undo();
    assert_eq!(name(&state), "a");
    assert!(!state.can_undo());

    // Rehacer reproduce el mismo cruce de diseños en sentido inverso.
    state.redo();
    assert_eq!(name(&state), "b");
    state.redo();
    assert_eq!(state.pending_global_redo, Some(GlobalStep::Edit(3)));
    state.active_slot_id = 3;
    state.finish_pending_global_redo();
    assert_eq!(name(&state), "c");
    state.redo();
    assert_eq!(state.pending_global_redo, Some(GlobalStep::Edit(1)));
    state.active_slot_id = 1;
    state.finish_pending_global_redo();
    assert_eq!(name(&state), "d");
    assert!(!state.can_redo());
}

#[test]
fn pasting_into_a_non_empty_canvas_keeps_the_old_contain_behavior() {
    let mut state = EditorState::new_blank(1080.0, 1080.0);
    // Deja el lienzo no vacío con una capa que no es una imagen.
    state.insert_layer_centered(
        "Rect",
        100.0,
        100.0,
        LayerContent::Shape(ShapeContent::default()),
    );

    state.add_image_layer("Pasted Image", None, loaded_image(540, 960));

    let page = state.doc.page().unwrap();
    assert_eq!(page.layers.len(), 2);
    assert_eq!(state.background_layer, None);

    // Nunca se amplía sobre un lienzo no vacío: 960 < 1080, cabe sin
    // escalar, y "contain" no se aplica (comportamiento de siempre).
    let fg = &page.layers[1];
    assert_eq!((fg.transform.width, fg.transform.height), (540.0, 960.0));
}
