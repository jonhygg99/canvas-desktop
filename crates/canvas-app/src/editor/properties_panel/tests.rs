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

#[test]
fn dropping_an_image_places_it_centered_at_the_drop_point() {
    let mut state = EditorState::new_blank(1000.0, 1000.0);
    // Arrastre desde Unsplash: sin fondo automático, centrada en el punto.
    state.add_image_layer_at("Unsplash · foto", (700.0, 300.0), loaded_image(500, 500));

    let page = state.doc.page().unwrap();
    assert_eq!(page.layers.len(), 1, "el arrastre nunca añade fondo");
    assert_eq!(state.background_layer, None);

    let fg = &page.layers[0];
    assert_eq!((fg.transform.width, fg.transform.height), (500.0, 500.0));
    // Centrada en (700, 300): la esquina queda a media imagen de distancia.
    assert_eq!(fg.transform.x, 700.0 - 250.0);
    assert_eq!(fg.transform.y, 300.0 - 250.0);
    assert_eq!(state.selection.primary(), Some(fg.id));
}

#[test]
fn dropping_an_image_larger_than_the_page_scales_it_down_but_keeps_the_center() {
    let mut state = EditorState::new_blank(1000.0, 1000.0);
    // 2:1 sobre página 1:1 → escala 0.5 (1000x500), centrada en (500, 500).
    state.add_image_layer_at("Unsplash · ancha", (500.0, 500.0), loaded_image(2000, 1000));

    let fg = &state.doc.page().unwrap().layers[0];
    assert_eq!((fg.transform.width, fg.transform.height), (1000.0, 500.0));
    assert_eq!(fg.transform.x, 0.0);
    assert_eq!(fg.transform.y, 250.0);
}

// ---------------------------------------------------------------------------
// Edición de propiedades → comandos deshacibles. Conducen los MISMOS widgets
// que la app (DragValue/Slider/checkbox) a través de frames de egui reales
// (CPU, sin backend), como hace `opacity_tests`, así que prueban que la
// edición de tamaño/opacidad/efectos acaba en un paso de deshacer y que un
// solo Ctrl+Z devuelve el valor original.
// ---------------------------------------------------------------------------

use super::effects::{blur_control, color_adjustments_ui, shadow_ui};

/// Un rectángulo de 200×100 en (50, 50) como capa seleccionada.
fn selected_rect(state: &mut EditorState) -> canvas_core::LayerId {
    let id = state
        .doc
        .add_layer(
            "capa",
            Transform::new(50.0, 50.0, 200.0, 100.0),
            LayerContent::Shape(ShapeContent {
                kind: canvas_core::ShapeKind::Rect,
                fill: [255, 0, 0, 255],
                stroke: [0, 0, 0, 0],
                stroke_width: 0.0,
                corner_radius: 0.0,
            }),
        )
        .unwrap();
    state.selection = canvas_core::Selection::single(id);
    id
}

/// Un frame headless de egui que corre `fx(state, ui, id)` a la vez.
fn run_one(
    ctx: &egui::Context,
    state: &mut EditorState,
    id: canvas_core::LayerId,
    fx: fn(&mut EditorState, &mut egui::Ui, canvas_core::LayerId),
    events: Vec<egui::Event>,
) {
    let _ = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 400.0),
            )),
            events,
            ..Default::default()
        },
        |ui| fx(state, ui, id),
    );
}

/// Un clic de egui real (mover el puntero, pulsar y soltar), como en la app.
fn click_one(
    ctx: &egui::Context,
    state: &mut EditorState,
    id: canvas_core::LayerId,
    fx: fn(&mut EditorState, &mut egui::Ui, canvas_core::LayerId),
    pos: egui::Pos2,
) {
    run_one(ctx, state, id, fx, vec![egui::Event::PointerMoved(pos)]);
    run_one(
        ctx,
        state,
        id,
        fx,
        vec![egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    run_one(
        ctx,
        state,
        id,
        fx,
        vec![egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
}

/// Un arrastre de egui real sobre el widget en `(x0, y)` hacia `(x1, y)`.
fn drag_one(
    ctx: &egui::Context,
    state: &mut EditorState,
    id: canvas_core::LayerId,
    fx: fn(&mut EditorState, &mut egui::Ui, canvas_core::LayerId),
    y: f32,
    x0: f32,
    x1: f32,
) {
    run_one(
        ctx,
        state,
        id,
        fx,
        vec![egui::Event::PointerMoved(egui::pos2(x0, y))],
    );
    run_one(
        ctx,
        state,
        id,
        fx,
        vec![egui::Event::PointerButton {
            pos: egui::pos2(x0, y),
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    run_one(
        ctx,
        state,
        id,
        fx,
        vec![egui::Event::PointerMoved(egui::pos2(x1, y))],
    );
    run_one(
        ctx,
        state,
        id,
        fx,
        vec![egui::Event::PointerButton {
            pos: egui::pos2(x1, y),
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
}

/// Editar el ancho (campo W de la sección «Size») en un frame headless
/// termina en un `SetTransform` deshacible: un Ctrl+Z restaura el transform
/// original, tamaño y posición incluidos (escalado alrededor del centro).
#[test]
fn editing_the_size_in_the_panel_commits_an_undoable_transform() {
    let ctx = egui::Context::default();
    ctx.set_fonts(egui::FontDefinitions::empty());
    let mut state = EditorState::new_blank(400.0, 1200.0);
    let id = selected_rect(&mut state);
    let original = state.doc.layer(id).unwrap().transform;

    // Renderiza el panel y arrastra el campo W (fila superior de «Size»).
    let wrap = |state: &mut EditorState, events: Vec<egui::Event>| {
        let _ = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
            properties_ui(state, ui);
        });
    };
    wrap(&mut state, vec![]);
    wrap(&mut state, vec![egui::Event::PointerMoved(egui::pos2(40.0, 110.0))]);
    wrap(
        &mut state,
        vec![egui::Event::PointerButton {
            pos: egui::pos2(40.0, 110.0),
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    wrap(&mut state, vec![egui::Event::PointerMoved(egui::pos2(110.0, 110.0))]);
    wrap(
        &mut state,
        vec![egui::Event::PointerButton {
            pos: egui::pos2(110.0, 110.0),
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );

    let edited = state.doc.layer(id).unwrap().transform;
    assert!(
        edited.width > original.width,
        "el arrastre debe agrandar la capa en vivo"
    );
    assert!(state.history.can_undo(), "la edición debe ser un paso de deshacer");

    state.undo();
    let restored = state.doc.layer(id).unwrap().transform;
    assert_eq!(
        restored, original,
        "un undo debe devolver el transform original completo"
    );
}

/// Arrastrar el slider de desenfoque termina en un `SetBlur` deshacible.
#[test]
fn editing_blur_commits_an_undoable_set_blur() {
    let ctx = egui::Context::default();
    ctx.set_fonts(egui::FontDefinitions::empty());
    let mut state = EditorState::new_blank(400.0, 400.0);
    let id = selected_rect(&mut state);
    let before = state.doc.layer(id).unwrap().effects.blur_radius;

    drag_one(&ctx, &mut state, id, blur_control, 0.0, 200.0, 320.0);

    let edited = state.doc.layer(id).unwrap().effects.blur_radius;
    assert!(edited > before, "el arrastre debe añadir desenfoque");
    assert!(state.history.can_undo());

    state.undo();
    assert_eq!(state.doc.layer(id).unwrap().effects.blur_radius, before);
}

/// Ajustar la saturación/brillo (cualquier slider de color) termina en un
/// `SetEffects` deshacible que restaura el ajuste neutro original.
#[test]
fn editing_color_commits_an_undoable_set_effects() {
    let ctx = egui::Context::default();
    ctx.set_fonts(egui::FontDefinitions::empty());
    let mut state = EditorState::new_blank(400.0, 400.0);
    let id = selected_rect(&mut state);
    let original = state.doc.layer(id).unwrap().effects;

    // El primer slider de color es «Brightness»: arrastrarlo lo desvía.
    drag_one(
        &ctx,
        &mut state,
        id,
        color_adjustments_ui,
        0.0,
        200.0,
        320.0,
    );

    let edited = state.doc.layer(id).unwrap().effects;
    assert!(
        (edited.brightness - original.brightness).abs() > f32::EPSILON,
        "el arrastre debe desviar el brillo (efectos de color)"
    );
    assert!(state.history.can_undo());

    state.undo();
    assert_eq!(state.doc.layer(id).unwrap().effects, original);
}

/// Marcar la casilla de sombra termina en un `SetShadow` deshacible (on), y
/// un segundo Ctrl+Z la restablecería; aquí verificamos el primer paso.
#[test]
fn enabling_the_shadow_commits_an_undoable_set_shadow() {
    let ctx = egui::Context::default();
    ctx.set_fonts(egui::FontDefinitions::empty());
    let mut state = EditorState::new_blank(400.0, 400.0);
    let id = selected_rect(&mut state);
    assert!(state.doc.layer(id).unwrap().effects.shadow.is_none());

    click_one(&ctx, &mut state, id, shadow_ui, egui::pos2(12.0, 0.0));

    let shadow = state.doc.layer(id).unwrap().effects.shadow;
    assert!(
        shadow.is_some(),
        "marcar la casilla debe añadir una sombra por defecto"
    );
    assert!(state.history.can_undo());

    state.undo();
    assert!(
        state.doc.layer(id).unwrap().effects.shadow.is_none(),
        "un undo debe quitar la sombra recién añadida"
    );
}

/// Con el candado de proporción (`aspect_lock`) activado, arrastrar el campo
/// W ajusta también el alto para conservar la relación de la capa, y todo el
/// arrastre acaba en un único `SetTransform` deshacible.
#[test]
fn dragging_width_with_aspect_lock_keeps_ratio_and_is_a_single_undo_step() {
    let ctx = egui::Context::default();
    ctx.set_fonts(egui::FontDefinitions::empty());
    let mut state = EditorState::new_blank(400.0, 1200.0);
    let id = selected_rect(&mut state);
    state.aspect_lock = true;

    let original = state.doc.layer(id).unwrap().transform;
    let ratio = original.aspect_ratio();

    // Renderiza el panel y arrastra el campo W (fila «W/H» de la sección
    // «Size») con el candado ya activado.
    let wrap = |state: &mut EditorState, events: Vec<egui::Event>| {
        let _ = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
            properties_ui(state, ui);
        });
    };
    wrap(&mut state, vec![]);
    wrap(&mut state, vec![egui::Event::PointerMoved(egui::pos2(40.0, 112.0))]);
    wrap(
        &mut state,
        vec![egui::Event::PointerButton {
            pos: egui::pos2(40.0, 112.0),
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    wrap(&mut state, vec![egui::Event::PointerMoved(egui::pos2(340.0, 112.0))]);
    wrap(
        &mut state,
        vec![egui::Event::PointerButton {
            pos: egui::pos2(340.0, 112.0),
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );

    let edited = state.doc.layer(id).unwrap().transform;
    assert!(
        edited.width > original.width,
        "el arrastre debe agrandar el ancho en vivo"
    );
    assert_eq!(
        edited.height,
        (edited.width / ratio).max(1.0),
        "con aspect_lock el alto sigue a la proporción original (ratio {ratio})"
    );
    assert!(
        (edited.aspect_ratio() - ratio).abs() < 1e-3,
        "la relación anchura/altura debe conservarse al escalar"
    );

    // Todo el arrastre es UN solo paso de deshacer: un único undo deja el
    // panel sin cambios pendientes y restaura el transform original.
    assert!(state.history.can_undo());
    state.undo();
    let restored = state.doc.layer(id).unwrap().transform;
    assert_eq!(
        restored, original,
        "un único undo debe devolver el transform original completo"
    );
}

#[test]
fn probe_lock_toggle() {
    for y in (30u32..=240).step_by(3) {
        let y = y as f32;
        let ctx = egui::Context::default(); // real fonts
        let mut state = EditorState::new_blank(400.0, 1200.0);
        let _id = selected_rect(&mut state);
        assert!(state.aspect_lock, "el default debe ser true");
        let wrap = |state: &mut EditorState, events: Vec<egui::Event>| {
            let _ = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
                properties_ui(state, ui);
            });
        };
        wrap(&mut state, vec![]);
        wrap(&mut state, vec![egui::Event::PointerMoved(egui::pos2(12.0, y))]);
        wrap(&mut state, vec![egui::Event::PointerButton { pos: egui::pos2(12.0, y), button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE }]);
        wrap(&mut state, vec![egui::Event::PointerButton { pos: egui::pos2(12.0, y), button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::NONE }]);
        if !state.aspect_lock {
            eprintln!("PROBE_LOCK_HIT toggled true->false at y={y}");
        }
    }
}

/// Una capa de imagen 400×200 a la que se le ha recortado la esquina inferior
/// derecha; devuelve su `LayerId` con la selección apuntando a ella.
fn cropped_image(state: &mut EditorState) -> canvas_core::LayerId {
    let (cropped_t, crop) = canvas_core::trim_crop_from_corner(
        &canvas_core::Transform::new(100.0, 100.0, 400.0, 200.0),
        canvas_core::CropRect::full(),
        canvas_core::Corner::BottomRight,
        -40.0,
        -30.0,
    );
    let id = state
        .doc
        .add_layer(
            "foto",
            cropped_t,
            LayerContent::Image(ImageContent {
                source_path: None,
                natural_width: 400,
                natural_height: 200,
                crop: Some(crop),
            }),
        )
        .unwrap();
    state.selection = canvas_core::Selection::single(id);
    id
}

/// Un clic en un punto concreto dentro del panel, usando fuentes reales
/// (los botones de texto necesitan anchos de glifo reales para pegarles).
fn panel_click(
    ctx: &egui::Context,
    state: &mut EditorState,
    pos: egui::Pos2,
) {
    let r = |st: &mut EditorState, events: Vec<egui::Event>| {
        let _ = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
            properties_ui(st, ui);
        });
    };
    r(state, vec![]);
    r(state, vec![egui::Event::PointerMoved(pos)]);
    r(
        state,
        vec![egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    r(
        state,
        vec![egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
}

/// Posiciones (con fuentes reales) de la fila «Crop»: el botón Crop/Done a la
/// izquierda y Reset junto a él.
const CROP_TOGGLE_POS: egui::Pos2 = egui::Pos2::new(40.0, 516.0);
const CROP_RESET_POS: egui::Pos2 = egui::Pos2::new(100.0, 515.0);

/// El botón «Crop» de una capa de imagen entra en modo recorte, y al pulsarlo
/// de nuevo («Done») lo deja.
#[test]
fn clicking_the_crop_button_enters_and_exits_crop_mode() {
    let ctx = egui::Context::default();
    let mut state = EditorState::new_blank(400.0, 1200.0);
    let _id = cropped_image(&mut state);
    assert!(!state.crop_mode);

    panel_click(&ctx, &mut state, CROP_TOGGLE_POS);
    assert!(
        state.crop_mode,
        "pulsar «Crop» debe activar el modo recorte"
    );

    panel_click(&ctx, &mut state, CROP_TOGGLE_POS);
    assert!(
        !state.crop_mode,
        "pulsar «Done» debe salir del modo recorte"
    );
}

/// Pulsar «Reset» en una capa con recorte limpia el crop, ensancha el
/// transform hasta el contenido completo (`uncrop_transform`) y lo consolida
/// en UN `Composite` deshacible que un solo Ctrl+Z revierte por completo.
#[test]
fn resetting_a_crop_commits_an_undoable_composite() {
    let ctx = egui::Context::default();
    let mut state = EditorState::new_blank(400.0, 1200.0);
    let id = cropped_image(&mut state);
    let before = match &state.doc.layer(id).unwrap().content {
        LayerContent::Image(c) => (c.crop.unwrap(), state.doc.layer(id).unwrap().transform),
        _ => unreachable!(),
    };
    let restored_expected =
        canvas_core::uncrop_transform(&before.1, before.0);
    let (orig_crop, orig_transform) = before;

    panel_click(&ctx, &mut state, CROP_RESET_POS);

    let after = state.doc.layer(id).unwrap();
    assert!(
        matches!(&after.content, LayerContent::Image(c) if c.crop.is_none()),
        "Reset debe quitar el recorte no destructivo"
    );
    // El transform se expande hasta mostrar la imagen completa, centrada.
    assert!(
        (after.transform.width - restored_expected.width).abs() < 1e-6
            && (after.transform.height - restored_expected.height).abs() < 1e-6,
        "Reset debe expandir el transform al contenido completo, no solo borrar el crop"
    );
    assert!(
        state.can_undo(),
        "el Reset debe registrar un paso deshacible"
    );

    state.undo();
    let undone = state.doc.layer(id).unwrap();
    assert_eq!(
        undone.transform, orig_transform,
        "el undo debe devolver el transform recortado original"
    );
    assert_eq!(
        match &undone.content {
            LayerContent::Image(c) => c.crop,
            _ => None,
        },
        Some(orig_crop),
        "el undo debe restaurar el crop original en un solo paso"
    );
}

/// Arrastrar una esquina en modo recorte (gesto sobre el lienzo) muta el
/// crop/transform en vivo y, al soltar, lo consolida en UN `Composite`
/// deshacible que un solo Ctrl+Z revierte.
#[test]
fn dragging_a_crop_corner_commits_an_undoable_composite() {
    use super::super::interaction::layer_interaction;
    use super::super::viewport::layer_corners_screen;

    let ctx = egui::Context::default();
    let mut state = EditorState::new_blank(400.0, 1200.0);
    let id = cropped_image(&mut state);
    state.crop_mode = true;
    state.viewport.zoom = 1.0;
    state.viewport.pan = egui::Vec2::ZERO;

    // Área de lienzo con origen en (0,0); con zoom 1 y pan 0, coordenadas de
    // pantalla == coordenadas de página.
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
    let t0 = state.doc.layer(id).unwrap().transform;
    let crop0 = match &state.doc.layer(id).unwrap().content {
        LayerContent::Image(c) => c.crop,
        _ => None,
    };

    // La esquina inferior derecha de la capa, en coordenadas de pantalla.
    let br = layer_corners_screen(&state.viewport, rect, &t0)[3];

    let run = |state: &mut EditorState, events: Vec<egui::Event>| {
        let _ = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(rect),
                events,
                ..Default::default()
            },
            |ui| {
                let response = ui.allocate_response(rect.size(), egui::Sense::drag());
                layer_interaction(state, ui, &response, response.rect);
            },
        );
    };

    // 1) Hover, 2) pulsar sobre la esquina → inicia `Gesture::Crop`, 3)
    // arrastrar hacia dentro (encoger), 4) soltar → consolida el comanddo.
    run(&mut state, vec![egui::Event::PointerMoved(br)]);
    run(
        &mut state,
        vec![egui::Event::PointerButton {
            pos: br,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    let target = br + egui::vec2(-30.0, -20.0);
    run(&mut state, vec![egui::Event::PointerMoved(target)]);
    run(
        &mut state,
        vec![egui::Event::PointerButton {
            pos: target,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );

    let edited = state.doc.layer(id).unwrap();
    let edited_crop = match &edited.content {
        LayerContent::Image(c) => c.crop,
        _ => None,
    };
    assert!(
        edited_crop != crop0 || edited.transform != t0,
        "arrastrar la esquina debe cambiar el crop/transform"
    );
    assert!(
        edited_crop.is_some() && edited.transform.width < t0.width,
        "encoger la esquina inferior derecha debe reducir la ventana visible"
    );
    assert!(
        state.can_undo(),
        "el gesto de recorte debe ser un paso deshacible"
    );

    state.undo();
    let undone = state.doc.layer(id).unwrap();
    assert_eq!(
        undone.transform, t0,
        "el undo debe devolver el transform original"
    );
    assert_eq!(
        match &undone.content {
            LayerContent::Image(c) => c.crop,
            _ => None,
        },
        crop0,
        "el undo debe restaurar el crop original en un solo paso"
    );
}

