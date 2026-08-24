//! Tests del borrado de capas. Sin GPU ni egui: `delete_selected` solo toca el
//! documento a traves de comandos.

use canvas_core::{LayerContent, LayerId, ShapeContent, ShapeKind, Transform};

use super::layer_ops::{delete_selected, has_deletable_selection};
use super::EditorState;

fn shape(state: &mut EditorState, name: &str) -> LayerId {
    state
        .doc
        .add_layer(
            name,
            Transform::new(0.0, 0.0, 10.0, 10.0),
            LayerContent::Shape(ShapeContent {
                kind: ShapeKind::Rect,
                fill: [255, 0, 0, 255],
                stroke: [0, 0, 0, 0],
                stroke_width: 0.0,
                corner_radius: 0.0,
            }),
        )
        .expect("un documento nuevo siempre tiene página")
}

fn group(state: &mut EditorState, name: &str) -> LayerId {
    state
        .doc
        .add_layer(
            name,
            Transform::new(0.0, 0.0, 10.0, 10.0),
            LayerContent::Group(canvas_core::GroupContent::default()),
        )
        .expect("un documento nuevo siempre tiene página")
}

fn lock(state: &mut EditorState, id: LayerId) {
    state.doc.layer_mut(id).expect("recién insertada").locked = true;
}

fn exists(state: &EditorState, id: LayerId) -> bool {
    state.doc.layer(id).is_ok()
}

fn select(state: &mut EditorState, ids: &[LayerId]) {
    state.selection = canvas_core::Selection::single(ids[0]);
    for &id in &ids[1..] {
        state.selection.toggle(id);
    }
}

#[test]
fn deleting_a_mixed_selection_skips_the_locked_layers() {
    // El bug que motiva este test: el botón «Delete» del panel de capas
    // borraba las bloqueadas, mientras que el menú y Ctrl+X las respetaban.
    let mut state = EditorState::new_blank(800.0, 600.0);
    let libre = shape(&mut state, "libre");
    let bloqueada = shape(&mut state, "bloqueada");
    lock(&mut state, bloqueada);
    select(&mut state, &[libre, bloqueada]);

    delete_selected(&mut state);

    assert!(!exists(&state, libre), "la capa libre debía borrarse");
    assert!(
        exists(&state, bloqueada),
        "la capa bloqueada NO debía borrarse"
    );
}

#[test]
fn a_layer_that_survives_the_delete_stays_selected() {
    // No ha desaparecido, así que deseleccionarla sería mentir sobre lo que
    // acaba de pasar. (El panel antes hacía `selection.clear()`.)
    let mut state = EditorState::new_blank(800.0, 600.0);
    let libre = shape(&mut state, "libre");
    let bloqueada = shape(&mut state, "bloqueada");
    lock(&mut state, bloqueada);
    select(&mut state, &[libre, bloqueada]);

    delete_selected(&mut state);

    assert_eq!(state.selection.ids(), &[bloqueada]);
}

#[test]
fn deleting_a_fully_locked_selection_does_nothing_at_all() {
    let mut state = EditorState::new_blank(800.0, 600.0);
    let bloqueada = shape(&mut state, "bloqueada");
    lock(&mut state, bloqueada);
    select(&mut state, &[bloqueada]);

    delete_selected(&mut state);

    assert!(exists(&state, bloqueada));
    assert!(
        !state.history.can_undo(),
        "un borrado que no borra nada no debe dejar un paso de deshacer vacío"
    );
}

#[test]
fn a_layer_locked_by_its_group_is_protected_too() {
    // `effective_locked`, no `locked`: bloquear el grupo protege a los hijos.
    let mut state = EditorState::new_blank(800.0, 600.0);
    let grupo = group(&mut state, "grupo");
    let hija = shape(&mut state, "hija");
    state
        .doc
        .page_mut()
        .expect("hay página")
        .move_subtree(hija, Some(grupo), 0)
        .expect("mover la hija dentro del grupo");
    lock(&mut state, grupo);
    select(&mut state, &[hija]);

    delete_selected(&mut state);

    assert!(
        exists(&state, hija),
        "una capa dentro de un grupo bloqueado no debe borrarse"
    );
}

#[test]
fn deleting_several_layers_is_a_single_undo_step() {
    let mut state = EditorState::new_blank(800.0, 600.0);
    let a = shape(&mut state, "a");
    let b = shape(&mut state, "b");
    let c = shape(&mut state, "c");
    select(&mut state, &[a, b, c]);

    delete_selected(&mut state);
    assert!(!exists(&state, a) && !exists(&state, b) && !exists(&state, c));

    state.undo();

    assert!(
        exists(&state, a) && exists(&state, b) && exists(&state, c),
        "un solo Ctrl+Z tiene que devolver las tres"
    );
    // El historial LOCAL del lienzo es el nivel correcto para comprobarlo:
    // `can_undo()` mira la pila global de la baraja, que es otra cosa.
    assert!(
        !state.history.can_undo(),
        "las tres capas dejaron más de un comando: no se agruparon en un Composite"
    );
}

#[test]
fn the_delete_button_is_disabled_when_everything_selected_is_locked() {
    // Si no, el botón se ve activo y al pulsarlo no pasa nada.
    let mut state = EditorState::new_blank(800.0, 600.0);
    let bloqueada = shape(&mut state, "bloqueada");
    lock(&mut state, bloqueada);
    select(&mut state, &[bloqueada]);

    assert!(!has_deletable_selection(&state));
}

#[test]
fn the_delete_button_is_enabled_when_something_can_go() {
    let mut state = EditorState::new_blank(800.0, 600.0);
    let libre = shape(&mut state, "libre");
    let bloqueada = shape(&mut state, "bloqueada");
    lock(&mut state, bloqueada);
    select(&mut state, &[libre, bloqueada]);

    assert!(has_deletable_selection(&state));
}

#[test]
fn an_empty_selection_has_nothing_to_delete() {
    let state = EditorState::new_blank(800.0, 600.0);
    assert!(!has_deletable_selection(&state));
}
