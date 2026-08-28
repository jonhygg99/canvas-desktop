//! Tests del borrado de capas. Sin GPU ni egui: `delete_selected` solo toca el
//! documento a traves de comandos.

use canvas_core::{LayerContent, LayerId, ShapeContent, ShapeKind, Transform};

use super::layer_ops::{
    apply_alignment, delete_selected, has_deletable_selection, reorder_layer, sibling_position,
    ZOrder,
};
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

fn ids(state: &EditorState) -> Vec<LayerId> {
    state
        .doc
        .page()
        .expect("hay página")
        .layers
        .iter()
        .map(|l| l.id)
        .collect()
}

#[test]
fn sibling_position_reports_parent_and_bounds() {
    let mut state = EditorState::new_blank(800.0, 600.0);
    let a = shape(&mut state, "a");
    let b = shape(&mut state, "b");
    let c = shape(&mut state, "c");
    assert_eq!(sibling_position(&state, a), Some((None, 0, 2)));
    assert_eq!(sibling_position(&state, b), Some((None, 1, 2)));
    assert_eq!(sibling_position(&state, c), Some((None, 2, 2)));
    assert_eq!(sibling_position(&state, LayerId::from_raw(9999)), None);
}

#[test]
fn reorder_layer_forward_and_backward_step_once() {
    let mut state = EditorState::new_blank(800.0, 600.0);
    let a = shape(&mut state, "a");
    let b = shape(&mut state, "b");
    let c = shape(&mut state, "c");

    reorder_layer(&mut state, a, ZOrder::Forward);
    assert_eq!(ids(&state), [b, a, c], "Forward sube un paso");

    reorder_layer(&mut state, c, ZOrder::Backward);
    assert_eq!(ids(&state), [b, c, a], "Backward baja un paso");
}

#[test]
fn reorder_layer_front_and_back_jump_to_the_ends() {
    let mut state = EditorState::new_blank(800.0, 600.0);
    let a = shape(&mut state, "a");
    let b = shape(&mut state, "b");
    let c = shape(&mut state, "c");

    reorder_layer(&mut state, a, ZOrder::Front);
    assert_eq!(ids(&state), [b, c, a], "Front lleva la capa al frente");

    reorder_layer(&mut state, a, ZOrder::Back);
    assert_eq!(ids(&state), [a, b, c], "Back la devuelve al fondo");
}

#[test]
fn reorder_layer_at_the_end_is_a_noop_without_an_undo_step() {
    let mut state = EditorState::new_blank(800.0, 600.0);
    let a = shape(&mut state, "a");
    let b = shape(&mut state, "b");
    let c = shape(&mut state, "c");

    reorder_layer(&mut state, c, ZOrder::Front); // ya está al frente
    assert_eq!(ids(&state), [a, b, c]);
    reorder_layer(&mut state, a, ZOrder::Back); // ya está al fondo
    assert_eq!(ids(&state), [a, b, c]);
    assert!(
        !state.history.can_undo(),
        "un reorden sin efecto no debe dejar un paso de deshacer"
    );
}

#[test]
fn reorder_layer_works_inside_a_group_and_is_undoable() {
    let mut state = EditorState::new_blank(800.0, 600.0);
    let g = group(&mut state, "g");
    let a = shape(&mut state, "a");
    let b = shape(&mut state, "b");
    let c = shape(&mut state, "c");
    let page = state.doc.page_mut().expect("hay página");
    page.move_subtree(a, Some(g), 0)
        .expect("a dentro del grupo");
    page.move_subtree(b, Some(g), 1)
        .expect("b dentro del grupo");
    page.move_subtree(c, Some(g), 2)
        .expect("c dentro del grupo");
    assert_eq!(state.doc.page().unwrap().children_of(Some(g)), [a, b, c]);
    assert_eq!(sibling_position(&state, a), Some((Some(g), 0, 2)));

    reorder_layer(&mut state, a, ZOrder::Forward);
    assert_eq!(state.doc.page().unwrap().children_of(Some(g)), [b, a, c]);

    state.undo();
    assert_eq!(
        state.doc.page().unwrap().children_of(Some(g)),
        [a, b, c],
        "un undo restaura el orden dentro del grupo"
    );
}

#[test]
fn apply_alignment_sets_the_transform_as_one_undo_step() {
    let mut state = EditorState::new_blank(800.0, 600.0);
    let a = shape(&mut state, "a"); // 10x10 en (0,0)
    let after = Transform::new(0.0, 0.0, 100.0, 100.0);

    apply_alignment(&mut state, a, after);
    assert_eq!(state.doc.layer(a).unwrap().transform, after);

    state.undo();
    assert_eq!(
        state.doc.layer(a).unwrap().transform,
        Transform::new(0.0, 0.0, 10.0, 10.0),
        "un undo restaura el transform original"
    );
}

#[test]
fn apply_alignment_with_the_same_transform_is_a_noop() {
    let mut state = EditorState::new_blank(800.0, 600.0);
    let a = shape(&mut state, "a");

    apply_alignment(&mut state, a, Transform::new(0.0, 0.0, 10.0, 10.0));

    assert!(
        !state.history.can_undo(),
        "alinear con el mismo transform no debe dejar paso de deshacer"
    );
}
