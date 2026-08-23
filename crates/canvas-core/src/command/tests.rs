//! Tests del modulo `command`. Se mantienen juntos a proposito: muchos
//! cruzan varias familias de comando (agrupar y luego deshacer, rehacer sobre
//! un `Composite`), asi que repartirlos por archivo los haria menos legibles.

use super::*;
use crate::document::Document;
use crate::error::CoreError;
use crate::layer::{ImageContent, LayerContent, LayerId, Transform};

fn doc_with_layer() -> (Document, LayerId) {
    let mut doc = Document::new(800.0, 600.0);
    let id = doc
        .add_layer(
            "img",
            Transform::new(10.0, 20.0, 100.0, 50.0),
            LayerContent::Image(ImageContent {
                source_path: None,
                natural_width: 100,
                natural_height: 50,
                crop: None,
            }),
        )
        .expect("documento recién creado tiene página");
    (doc, id)
}

fn move_cmd(layer: LayerId, before: Transform, x: f64, y: f64) -> Box<dyn Command> {
    Box::new(SetTransform {
        layer,
        before,
        after: Transform { x, y, ..before },
    })
}

fn image_content() -> LayerContent {
    LayerContent::Image(ImageContent {
        source_path: None,
        natural_width: 10,
        natural_height: 10,
        crop: None,
    })
}

/// Documento con dos capas de imagen raíz: `a` (más abajo), `b` (más
/// arriba). Sin agrupar todavía: cada test construye el árbol que
/// necesita con los propios comandos que está probando.
fn doc_with_two_layers() -> (Document, LayerId, LayerId) {
    let mut doc = Document::new(800.0, 600.0);
    let a = doc
        .add_layer("a", Transform::new(0.0, 0.0, 10.0, 10.0), image_content())
        .unwrap();
    let b = doc
        .add_layer("b", Transform::new(20.0, 20.0, 10.0, 10.0), image_content())
        .unwrap();
    (doc, a, b)
}

#[test]
fn apply_undo_redo_roundtrip() {
    let (mut doc, id) = doc_with_layer();
    let before = doc.layer(id).unwrap().transform;
    let mut history = History::default();

    history
        .apply(&mut doc, move_cmd(id, before, 200.0, 300.0))
        .unwrap();
    assert_eq!(doc.layer(id).unwrap().transform.x, 200.0);

    assert!(history.undo(&mut doc).unwrap());
    assert_eq!(doc.layer(id).unwrap().transform, before);

    assert!(history.redo(&mut doc).unwrap());
    assert_eq!(doc.layer(id).unwrap().transform.x, 200.0);
    assert_eq!(doc.layer(id).unwrap().transform.y, 300.0);
}

#[test]
fn undo_on_empty_history_is_noop() {
    let (mut doc, _) = doc_with_layer();
    let mut history = History::default();
    assert!(!history.undo(&mut doc).unwrap());
    assert!(!history.redo(&mut doc).unwrap());
}

/// Comando de prueba cuyo `revert`/`apply` fallan siempre — simula un
/// comando cuyo destino (p. ej. una capa) ya no existe en el documento.
#[derive(Debug)]
struct AlwaysFails;

impl Command for AlwaysFails {
    fn label(&self) -> &str {
        "siempre falla"
    }

    fn apply(&mut self, _doc: &mut Document) -> Result<(), CoreError> {
        Err(CoreError::NoPages)
    }

    fn revert(&mut self, _doc: &mut Document) -> Result<(), CoreError> {
        Err(CoreError::NoPages)
    }
}

#[test]
fn undo_failure_discards_the_command_instead_of_wedging_history() {
    let (mut doc, id) = doc_with_layer();
    let before = doc.layer(id).unwrap().transform;
    let mut history = History::default();

    history
        .apply(&mut doc, move_cmd(id, before, 200.0, 300.0))
        .unwrap();
    history.push_applied(Box::new(AlwaysFails));

    // El revert que falla se propaga...
    assert!(history.undo(&mut doc).is_err());
    // ...pero no deja el comando atascado arriba de la pila: el paso
    // anterior sigue siendo deshacible.
    assert!(history.undo(&mut doc).unwrap());
    assert_eq!(doc.layer(id).unwrap().transform, before);
    assert!(!history.can_undo());
}

#[test]
fn redo_failure_discards_the_command_instead_of_wedging_history() {
    let (mut doc, id) = doc_with_layer();
    let before = doc.layer(id).unwrap().transform;
    let mut history = History::default();

    history
        .apply(&mut doc, move_cmd(id, before, 200.0, 300.0))
        .unwrap();
    history.undo(&mut doc).unwrap();
    // Un `AlwaysFails` deshecho a mano, empujado directamente a la pila
    // de redo (sin pasar por `push_applied`, que la vaciaría).
    history.redo.push(Box::new(AlwaysFails));

    assert!(history.redo(&mut doc).is_err());
    // El siguiente redo es el paso real, que sí debe funcionar.
    assert!(history.redo(&mut doc).unwrap());
    assert_eq!(doc.layer(id).unwrap().transform.x, 200.0);
    assert!(!history.can_redo());
}

#[test]
fn new_command_clears_redo() {
    let (mut doc, id) = doc_with_layer();
    let before = doc.layer(id).unwrap().transform;
    let mut history = History::default();

    history
        .apply(&mut doc, move_cmd(id, before, 200.0, 300.0))
        .unwrap();
    history.undo(&mut doc).unwrap();
    assert!(history.can_redo());

    history
        .apply(&mut doc, move_cmd(id, before, 50.0, 60.0))
        .unwrap();
    assert!(!history.can_redo());
    assert_eq!(doc.layer(id).unwrap().transform.x, 50.0);
}

#[test]
fn drag_coalesces_into_single_undo_step() {
    let (mut doc, id) = doc_with_layer();
    let start = doc.layer(id).unwrap().transform;
    let mut history = History::default();

    // Simula un arrastre de 200 frames: mutación directa, sin comandos.
    for i in 1..=200 {
        doc.layer_mut(id).unwrap().transform.x = start.x + f64::from(i);
    }
    let end = doc.layer(id).unwrap().transform;
    history.push_applied(Box::new(SetTransform {
        layer: id,
        before: start,
        after: end,
    }));

    // UN solo paso de deshacer devuelve al estado inicial.
    assert!(history.undo(&mut doc).unwrap());
    assert_eq!(doc.layer(id).unwrap().transform, start);
    assert!(!history.can_undo());
}

#[test]
fn dirty_tracks_saved_position() {
    let (mut doc, id) = doc_with_layer();
    let before = doc.layer(id).unwrap().transform;
    let mut history = History::default();
    assert!(
        !history.is_dirty(),
        "documento recién abierto no está sucio"
    );

    history
        .apply(&mut doc, move_cmd(id, before, 1.0, 1.0))
        .unwrap();
    assert!(history.is_dirty());

    history.undo(&mut doc).unwrap();
    assert!(
        !history.is_dirty(),
        "deshacer hasta el estado guardado limpia el sucio"
    );

    history.redo(&mut doc).unwrap();
    assert!(history.is_dirty());

    history.mark_saved();
    assert!(!history.is_dirty());

    history.undo(&mut doc).unwrap();
    assert!(
        history.is_dirty(),
        "deshacer por detrás del guardado ensucia"
    );
}

#[test]
fn saved_state_unreachable_after_diverging() {
    let (mut doc, id) = doc_with_layer();
    let before = doc.layer(id).unwrap().transform;
    let mut history = History::default();

    history
        .apply(&mut doc, move_cmd(id, before, 1.0, 1.0))
        .unwrap();
    history.mark_saved();
    history.undo(&mut doc).unwrap();
    // Nueva rama: el estado guardado ya no es alcanzable.
    history
        .apply(&mut doc, move_cmd(id, before, 2.0, 2.0))
        .unwrap();
    assert!(history.is_dirty());
    history.undo(&mut doc).unwrap();
    assert!(
        history.is_dirty(),
        "ni siquiera igualando la longitud de pila"
    );
}

#[test]
fn composite_applies_in_order_and_reverts_in_reverse() {
    let (mut doc, id) = doc_with_layer();
    let start = doc.layer(id).unwrap().transform;
    let mut history = History::default();

    // Dos pasos encadenados: el segundo parte del resultado del primero.
    let step1 = Transform { x: 100.0, ..start };
    let step2 = Transform { y: 200.0, ..step1 };
    history
        .apply(
            &mut doc,
            Box::new(Composite::new(
                "mover dos veces",
                vec![
                    Box::new(SetTransform {
                        layer: id,
                        before: start,
                        after: step1,
                    }),
                    Box::new(SetTransform {
                        layer: id,
                        before: step1,
                        after: step2,
                    }),
                ],
            )),
        )
        .unwrap();
    assert_eq!(doc.layer(id).unwrap().transform, step2);

    // UN solo deshacer revierte todo el grupo, en orden inverso.
    history.undo(&mut doc).unwrap();
    assert_eq!(doc.layer(id).unwrap().transform, start);
    assert!(!history.can_undo());

    history.redo(&mut doc).unwrap();
    assert_eq!(doc.layer(id).unwrap().transform, step2);
}

#[test]
fn set_shadow_roundtrips() {
    let (mut doc, id) = doc_with_layer();
    let mut history = History::default();
    let shadow = crate::Shadow::default();

    history
        .apply(
            &mut doc,
            Box::new(SetShadow {
                layer: id,
                before: None,
                after: Some(shadow),
            }),
        )
        .unwrap();
    assert_eq!(doc.layer(id).unwrap().effects.shadow, Some(shadow));

    history.undo(&mut doc).unwrap();
    assert_eq!(doc.layer(id).unwrap().effects.shadow, None);
}

#[test]
fn set_page_size_roundtrips() {
    let (mut doc, _) = doc_with_layer();
    let mut history = History::default();
    history
        .apply(
            &mut doc,
            Box::new(SetPageSize {
                before: (800.0, 600.0),
                after: (1920.0, 1080.0),
            }),
        )
        .unwrap();
    let page = doc.page().unwrap();
    assert_eq!((page.width, page.height), (1920.0, 1080.0));

    history.undo(&mut doc).unwrap();
    let page = doc.page().unwrap();
    assert_eq!((page.width, page.height), (800.0, 600.0));
}

#[test]
fn insert_and_remove_layer_undo_redo() {
    let (mut doc, existing) = doc_with_layer();
    let mut history = History::default();

    // Inserta una capa nueva en el fondo (índice 0).
    let id = doc.allocate_layer_id();
    let layer = crate::Layer::new(
        id,
        "fondo",
        Transform::new(0.0, 0.0, 10.0, 10.0),
        LayerContent::Image(ImageContent {
            source_path: None,
            natural_width: 10,
            natural_height: 10,
            crop: None,
        }),
    );
    history
        .apply(&mut doc, Box::new(InsertLayer { index: 0, layer }))
        .unwrap();
    assert_eq!(doc.page().unwrap().layers[0].id, id, "insertada al fondo");
    assert_eq!(doc.page().unwrap().layers.len(), 2);

    history.undo(&mut doc).unwrap();
    assert_eq!(doc.page().unwrap().layers.len(), 1);
    assert_eq!(doc.page().unwrap().layers[0].id, existing);

    history.redo(&mut doc).unwrap();
    assert_eq!(doc.page().unwrap().layers[0].id, id);

    // Y ahora quitarla, con deshacer que la devuelve a su sitio.
    history
        .apply(&mut doc, Box::new(RemoveLayer::new(id)))
        .unwrap();
    assert!(doc.layer(id).is_err());
    history.undo(&mut doc).unwrap();
    assert_eq!(doc.page().unwrap().layers[0].id, id, "vuelve al índice 0");
    history.redo(&mut doc).unwrap();
    assert!(doc.layer(id).is_err());
}

#[test]
fn history_limit_drops_oldest() {
    let (mut doc, id) = doc_with_layer();
    let before = doc.layer(id).unwrap().transform;
    let mut history = History::with_limit(5);

    for i in 0..8 {
        history
            .apply(&mut doc, move_cmd(id, before, f64::from(i), 0.0))
            .unwrap();
    }
    let mut undone = 0;
    while history.undo(&mut doc).unwrap() {
        undone += 1;
    }
    assert_eq!(undone, 5);
    assert!(
        history.is_dirty(),
        "el estado inicial se perdió del historial"
    );
}

#[test]
fn reorder_moves_a_layer_and_undo_puts_it_back() {
    let (mut doc, a, b) = doc_with_two_layers();
    let mut history = History::default();
    assert_eq!(doc.page().unwrap().children_of(None), vec![a, b]);

    history
        .apply(&mut doc, Box::new(Reorder::new(a, None, 1)))
        .unwrap();
    assert_eq!(doc.page().unwrap().children_of(None), vec![b, a]);

    history.undo(&mut doc).unwrap();
    assert_eq!(doc.page().unwrap().children_of(None), vec![a, b]);
}

#[test]
fn reorder_reparents_a_whole_subtree() {
    let (mut doc, a, _b) = doc_with_two_layers();
    let mut history = History::default();
    let group_id = doc.allocate_layer_id();
    history
        .apply(&mut doc, Box::new(Group::new(vec![a], group_id, "G")))
        .unwrap();
    let c = doc
        .add_layer("c", Transform::new(40.0, 40.0, 10.0, 10.0), image_content())
        .unwrap();
    let outer_id = doc.allocate_layer_id();
    history
        .apply(&mut doc, Box::new(Group::new(vec![c], outer_id, "Outer")))
        .unwrap();

    history
        .apply(
            &mut doc,
            Box::new(Reorder::new(group_id, Some(outer_id), 0)),
        )
        .unwrap();
    let page = doc.page().unwrap();
    assert_eq!(page.layer(a).unwrap().parent_id, Some(group_id));
    assert_eq!(page.layer(group_id).unwrap().parent_id, Some(outer_id));
    assert!(page.is_ancestor(outer_id, a));

    history.undo(&mut doc).unwrap();
    let page = doc.page().unwrap();
    assert_eq!(page.layer(group_id).unwrap().parent_id, None);
    assert!(!page.is_ancestor(outer_id, a));
}

#[test]
fn group_wraps_the_selection_and_undo_restores_the_order() {
    let (mut doc, a, b) = doc_with_two_layers();
    let group_id = doc.allocate_layer_id();
    let mut history = History::default();
    history
        .apply(
            &mut doc,
            Box::new(Group::new(vec![a, b], group_id, "Group")),
        )
        .unwrap();

    let page = doc.page().unwrap();
    assert_eq!(page.children_of(None), vec![group_id]);
    assert_eq!(page.children_of(Some(group_id)), vec![a, b]);

    history.undo(&mut doc).unwrap();
    let page = doc.page().unwrap();
    assert_eq!(page.children_of(None), vec![a, b]);
    assert!(
        page.layer(group_id).is_none(),
        "el grupo desaparece al deshacer"
    );
}

#[test]
fn group_ignores_children_whose_parent_is_also_selected() {
    let (mut doc, a, b) = doc_with_two_layers();
    let inner_id = doc.allocate_layer_id();
    let mut history = History::default();
    history
        .apply(&mut doc, Box::new(Group::new(vec![a], inner_id, "Inner")))
        .unwrap();

    // Selecciona el grupo interno Y su hijo "a": "a" debe descartarse
    // porque ya es descendiente de "inner_id".
    let outer_id = doc.allocate_layer_id();
    history
        .apply(
            &mut doc,
            Box::new(Group::new(vec![inner_id, a, b], outer_id, "Outer")),
        )
        .unwrap();

    let page = doc.page().unwrap();
    assert_eq!(page.children_of(Some(outer_id)), vec![inner_id, b]);
    assert_eq!(page.layer(a).unwrap().parent_id, Some(inner_id));
}

#[test]
fn ungroup_dissolves_the_group_in_place() {
    let (mut doc, a, b) = doc_with_two_layers();
    let group_id = doc.allocate_layer_id();
    let mut history = History::default();
    history
        .apply(
            &mut doc,
            Box::new(Group::new(vec![a, b], group_id, "Group")),
        )
        .unwrap();

    history
        .apply(&mut doc, Box::new(Ungroup::new(group_id)))
        .unwrap();
    let page = doc.page().unwrap();
    assert_eq!(page.children_of(None), vec![a, b]);
    assert!(page.layer(group_id).is_none());
}

#[test]
fn group_then_ungroup_is_the_identity() {
    let (mut doc, a, b) = doc_with_two_layers();
    let group_id = doc.allocate_layer_id();
    let mut history = History::default();

    history
        .apply(
            &mut doc,
            Box::new(Group::new(vec![a, b], group_id, "Group")),
        )
        .unwrap();
    history
        .apply(&mut doc, Box::new(Ungroup::new(group_id)))
        .unwrap();

    let page = doc.page().unwrap();
    assert_eq!(page.children_of(None), vec![a, b]);
    assert_eq!(page.layer(a).unwrap().parent_id, None);
    assert_eq!(page.layer(b).unwrap().parent_id, None);

    // Y el propio deshacer/rehacer de los dos pasos también cierra bien.
    history.undo(&mut doc).unwrap(); // deshace Ungroup
    assert!(doc.page().unwrap().is_group(group_id));
    history.undo(&mut doc).unwrap(); // deshace Group
    assert_eq!(doc.page().unwrap().children_of(None), vec![a, b]);
    assert!(doc.page().unwrap().layer(group_id).is_none());
}

#[test]
fn rename_roundtrips() {
    let (mut doc, id) = doc_with_layer();
    let mut history = History::default();
    history
        .apply(
            &mut doc,
            Box::new(Rename {
                layer: id,
                before: "img".to_owned(),
                after: "Renamed".to_owned(),
            }),
        )
        .unwrap();
    assert_eq!(doc.layer(id).unwrap().name, "Renamed");
    history.undo(&mut doc).unwrap();
    assert_eq!(doc.layer(id).unwrap().name, "img");
}

#[test]
fn set_visible_locked_and_opacity_roundtrip() {
    let (mut doc, id) = doc_with_layer();
    let mut history = History::default();
    history
        .apply(
            &mut doc,
            Box::new(SetVisible {
                layer: id,
                before: true,
                after: false,
            }),
        )
        .unwrap();
    assert!(!doc.layer(id).unwrap().visible);
    history
        .apply(
            &mut doc,
            Box::new(SetLocked {
                layer: id,
                before: false,
                after: true,
            }),
        )
        .unwrap();
    assert!(doc.layer(id).unwrap().locked);
    history
        .apply(
            &mut doc,
            Box::new(SetOpacity {
                layer: id,
                before: 1.0,
                after: 0.4,
            }),
        )
        .unwrap();
    assert!((doc.layer(id).unwrap().opacity - 0.4).abs() < 1e-6);

    history.undo(&mut doc).unwrap();
    assert!((doc.layer(id).unwrap().opacity - 1.0).abs() < 1e-6);
    history.undo(&mut doc).unwrap();
    assert!(!doc.layer(id).unwrap().locked);
    history.undo(&mut doc).unwrap();
    assert!(doc.layer(id).unwrap().visible);
}

#[test]
fn remove_layer_takes_the_whole_subtree_with_it() {
    let (mut doc, a, b) = doc_with_two_layers();
    let group_id = doc.allocate_layer_id();
    let mut history = History::default();
    history
        .apply(
            &mut doc,
            Box::new(Group::new(vec![a, b], group_id, "Group")),
        )
        .unwrap();

    history
        .apply(&mut doc, Box::new(RemoveLayer::new(group_id)))
        .unwrap();
    assert!(doc.page().unwrap().layers.is_empty());

    history.undo(&mut doc).unwrap();
    let page = doc.page().unwrap();
    assert_eq!(page.children_of(None), vec![group_id]);
    assert_eq!(page.children_of(Some(group_id)), vec![a, b]);
}

#[test]
fn nested_groups_survive_undo_redo() {
    let (mut doc, a, b) = doc_with_two_layers();
    let mut history = History::default();
    let inner = doc.allocate_layer_id();
    history
        .apply(&mut doc, Box::new(Group::new(vec![a], inner, "Inner")))
        .unwrap();
    let middle = doc.allocate_layer_id();
    history
        .apply(
            &mut doc,
            Box::new(Group::new(vec![inner], middle, "Middle")),
        )
        .unwrap();
    let outer = doc.allocate_layer_id();
    history
        .apply(&mut doc, Box::new(Group::new(vec![middle], outer, "Outer")))
        .unwrap();

    let page = doc.page().unwrap();
    assert_eq!(page.depth(a), 3, "a está dentro de outer > middle > inner");
    assert!(page.is_ancestor(outer, a));

    for _ in 0..3 {
        history.undo(&mut doc).unwrap();
    }
    assert_eq!(doc.page().unwrap().children_of(None), vec![a, b]);

    for _ in 0..3 {
        history.redo(&mut doc).unwrap();
    }
    let page = doc.page().unwrap();
    assert!(page.is_ancestor(outer, a));
    assert_eq!(page.depth(a), 3);
}
