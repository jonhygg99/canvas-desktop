//! Tests del panel lateral (Page / Layers / Insert): fila de capas,
//! reordenado y creación por tile. Movidos del `mod.rs` para respetar
//! la convención de un `tests.rs` por carpeta.

use super::*;
use crate::settings::LayersTabOrder;
use canvas_core::{LayerId, Selection, ShapeKind};
use eframe::egui;

/// Lo que debe crear `insert_item` para cada etiqueta del panel Insert:
/// nombre de la capa, tamaño y tipo de contenido. Espejo de `insert_item`
/// para detectar cualquier desvío entre lo que ofrece la cuadrícula y lo
/// que realmente se inserta.
struct InsertCase {
    label: &'static str,
    name: &'static str,
    w: f64,
    h: f64,
    kind: LayerKind,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum LayerKind {
    Text,
    Shape(ShapeKind),
}

const INSERT_CASES: [InsertCase; 12] = [
    InsertCase {
        label: "Text",
        name: "Text",
        w: 500.0,
        h: 120.0,
        kind: LayerKind::Text,
    },
    InsertCase {
        label: "Rect",
        name: "Rectangle",
        w: 320.0,
        h: 220.0,
        kind: LayerKind::Shape(ShapeKind::Rect),
    },
    InsertCase {
        label: "Ellipse",
        name: "Ellipse",
        w: 280.0,
        h: 280.0,
        kind: LayerKind::Shape(ShapeKind::Ellipse),
    },
    InsertCase {
        label: "Line",
        name: "Line",
        w: 400.0,
        h: 48.0,
        kind: LayerKind::Shape(ShapeKind::Line),
    },
    InsertCase {
        label: "Triangle",
        name: "Triangle",
        w: 320.0,
        h: 280.0,
        kind: LayerKind::Shape(ShapeKind::Triangle),
    },
    InsertCase {
        label: "Star",
        name: "Star",
        w: 320.0,
        h: 300.0,
        kind: LayerKind::Shape(ShapeKind::Star),
    },
    InsertCase {
        label: "Arrow",
        name: "Arrow",
        w: 400.0,
        h: 200.0,
        kind: LayerKind::Shape(ShapeKind::Arrow),
    },
    InsertCase {
        label: "Pentagon",
        name: "Pentagon",
        w: 320.0,
        h: 300.0,
        kind: LayerKind::Shape(ShapeKind::Pentagon),
    },
    InsertCase {
        label: "Hexagon",
        name: "Hexagon",
        w: 320.0,
        h: 280.0,
        kind: LayerKind::Shape(ShapeKind::Hexagon),
    },
    InsertCase {
        label: "Diamond",
        name: "Diamond",
        w: 280.0,
        h: 280.0,
        kind: LayerKind::Shape(ShapeKind::Diamond),
    },
    InsertCase {
        label: "Cross",
        name: "Cross",
        w: 300.0,
        h: 300.0,
        kind: LayerKind::Shape(ShapeKind::Cross),
    },
    InsertCase {
        label: "Heart",
        name: "Heart",
        w: 300.0,
        h: 280.0,
        kind: LayerKind::Shape(ShapeKind::Heart),
    },
];

/// La tabla de casos es un espejo exacto de la cuadrícula: ni etiquetas
/// del panel sin caso, ni casos huérfanos.
#[test]
fn insert_cases_match_the_panel_tiles() {
    assert_eq!(INSERT_CASES.len(), INSERT_ITEMS.len());
    for item in &INSERT_ITEMS {
        assert!(
            INSERT_CASES.iter().any(|c| c.label == item.label),
            "la etiqueta '{}' del panel no tiene caso esperado",
            item.label
        );
    }
    for case in &INSERT_CASES {
        assert!(
            INSERT_ITEMS.iter().any(|i| i.label == case.label),
            "el caso '{}' no corresponde a ninguna etiqueta del panel",
            case.label
        );
    }
}

#[test]
fn insert_item_creates_each_tile_centered_with_expected_content() {
    for case in &INSERT_CASES {
        let mut state = EditorState::new_blank(800.0, 600.0);
        insert_item(&mut state, case.label);
        let page = state
            .doc
            .page()
            .expect("un documento en blanco tiene página");
        assert_eq!(page.layers.len(), 1, "{}", case.label);
        let layer = &page.layers[0];
        assert_eq!(layer.name, case.name, "{}", case.label);
        assert_eq!(layer.transform.width, case.w, "{}", case.label);
        assert_eq!(layer.transform.height, case.h, "{}", case.label);
        // Centrada en la página (origen + mitad del tamaño = centro).
        let (cx, cy) = layer.transform.center();
        assert!(
            (cx - page.width / 2.0).abs() < 1e-9,
            "{}: centrado en x",
            case.label
        );
        assert!(
            (cy - page.height / 2.0).abs() < 1e-9,
            "{}: centrado en y",
            case.label
        );
        match (case.kind, &layer.content) {
            (LayerKind::Text, LayerContent::Text(_)) => {}
            (LayerKind::Shape(k), LayerContent::Shape(s)) => {
                assert_eq!(s.kind, k, "{}", case.label)
            }
            (expected, got) => panic!(
                "{}: esperaba {:?}, la capa tiene {:?}",
                case.label, expected, got
            ),
        }
        // La capa nueva queda seleccionada (la inserta y selecciona).
        assert_eq!(
            state.selection,
            Selection::single(layer.id),
            "{}",
            case.label
        );
    }
}

/// Cualquier etiqueta desconocida cae al caso por defecto: una flecha.
#[test]
fn insert_item_unknown_label_falls_back_to_an_arrow() {
    let mut state = EditorState::new_blank(800.0, 600.0);
    insert_item(&mut state, "NoSuchItem");
    let page = state
        .doc
        .page()
        .expect("un documento en blanco tiene página");
    assert_eq!(page.layers.len(), 1);
    let layer = &page.layers[0];
    assert_eq!(layer.name, "Arrow");
    assert_eq!(layer.transform.width, 400.0);
    assert_eq!(layer.transform.height, 200.0);
    let LayerContent::Shape(s) = &layer.content else {
        panic!(
            "el caso por defecto debe crear una forma, no {:?}",
            layer.content
        );
    };
    assert_eq!(s.kind, ShapeKind::Arrow);
    let (cx, cy) = layer.transform.center();
    assert!((cx - page.width / 2.0).abs() < 1e-9);
    assert!((cy - page.height / 2.0).abs() < 1e-9);
    assert_eq!(state.selection, Selection::single(layer.id));
}

/// `insert_item` es deshacible: cada inserción apila un paso en el
/// historial, un `undo()` devuelve la página a su estado anterior (sin
/// capas) y el `redo()` restaura la capa. Se comprueba para cada etiqueta
/// del panel y para el caso por defecto (etiqueta desconocida).
#[test]
fn insert_item_is_undoable_and_redoable_for_every_tile() {
    for label in INSERT_ITEMS.iter().map(|i| i.label).chain(["NoSuchItem"]) {
        let mut state = EditorState::new_blank(800.0, 600.0);
        insert_item(&mut state, label);
        let page = state
            .doc
            .page()
            .expect("un documento en blanco tiene página");
        assert_eq!(page.layers.len(), 1, "{label}: debe insertar una capa");
        let inserted = page.layers[0].id;

        state.undo();
        let page = state
            .doc
            .page()
            .expect("un documento en blanco tiene página");
        assert!(
            page.layers.is_empty(),
            "{label}: deshacer debe dejar la página sin capas"
        );
        assert!(
            !state.selection.contains(inserted),
            "{label}: la selección debe olvidar la capa deshecha"
        );

        state.redo();
        let page = state
            .doc
            .page()
            .expect("un documento en blanco tiene página");
        assert_eq!(
            page.layers.len(),
            1,
            "{label}: rehacer debe restaurar la capa insertada"
        );
    }
}

/// Dos inserciones del panel se apilan en el orden de inserción (índice
/// 0 = abajo, arriba del todo = última) y el deshacer las quita en orden
/// inverso, restaurando el rehacer el apilado original.
#[test]
fn insert_item_stacks_layers_in_order_and_undo_removes_them_in_reverse() {
    let mut state = EditorState::new_blank(800.0, 600.0);
    insert_item(&mut state, "Rect"); // primera, abajo del todo
    insert_item(&mut state, "Heart"); // segunda, encima

    let page = state
        .doc
        .page()
        .expect("un documento en blanco tiene página");
    assert_eq!(page.layers.len(), 2);
    assert_eq!(
        page.layers[0].name, "Rectangle",
        "la primera inserción queda abajo"
    );
    assert_eq!(
        page.layers[1].name, "Heart",
        "la segunda inserción queda encima"
    );
    // La última insertada es la que manda: seleccionada y en el tope.
    assert_eq!(state.selection, Selection::single(page.layers[1].id));

    state.undo();
    let page = state
        .doc
        .page()
        .expect("un documento en blanco tiene página");
    assert_eq!(page.layers.len(), 1);
    assert_eq!(
        page.layers[0].name, "Rectangle",
        "el primer deshacer quita la de arriba (Heart), no la de abajo"
    );

    state.undo();
    let page = state
        .doc
        .page()
        .expect("un documento en blanco tiene página");
    assert!(
        page.layers.is_empty(),
        "el segundo deshacer deja la página sin capas"
    );

    // El rehacer las restaura en el mismo orden de apilado original.
    state.redo();
    state.redo();
    let page = state
        .doc
        .page()
        .expect("un documento en blanco tiene página");
    assert_eq!(page.layers.len(), 2);
    assert_eq!(page.layers[0].name, "Rectangle");
    assert_eq!(page.layers[1].name, "Heart");
}

/// Un documento con tres capas raíz (`Rect`, `Ellipse`, `Heart`, de
/// abajo arriba) y sus ids, listo para reordenar.
fn state_with_three_layers() -> (EditorState, [LayerId; 3]) {
    let mut state = EditorState::new_blank(800.0, 600.0);
    let mut ids = Vec::new();
    for label in ["Rect", "Ellipse", "Heart"] {
        insert_item(&mut state, label);
        let page = state
            .doc
            .page()
            .expect("un documento en blanco tiene página");
        ids.push(page.layers.last().expect("insert_item añade una capa").id);
    }
    (state, [ids[0], ids[1], ids[2]])
}

/// `push_rows` recorre TODAS las capas del documento (las de los grupos
/// incluidas) y `row_ui` las pinta sin romper: tantas filas como capas,
/// en orden de panel (grupo primero, hijos después, raíz más alta la
/// última) y con la sangría por profundidad.
#[test]
fn row_ui_renders_every_layer_of_the_document() {
    let mut state = EditorState::new_blank(800.0, 600.0);
    insert_item(&mut state, "Rect");
    insert_item(&mut state, "Ellipse");
    insert_item(&mut state, "Heart");
    // Grupo con Ellipse y Heart: [Rect, Group(Ellipse, Heart)].
    let page = state
        .doc
        .page()
        .expect("un documento en blanco tiene página");
    let ids: Vec<LayerId> = page.layers.iter().map(|l| l.id).collect();
    state.selection = Selection::single(ids[1]);
    state.selection.toggle(ids[2]);
    group_selection(&mut state);
    let page = state
        .doc
        .page()
        .expect("un documento en blanco tiene página");
    let group = page.layers[1].id;
    assert!(page.is_group(group), "Ellipse y Heart quedan en un grupo");

    // Una fila por capa, en orden de panel (de arriba a abajo).
    let mut rows = Vec::new();
    push_rows(page, None, 0, &mut rows);
    assert_eq!(rows.len(), 4, "todas las capas tienen fila");
    assert_eq!(
        rows[0].id, group,
        "el grupo va primero (arriba en el panel)"
    );
    assert_eq!(rows[0].depth, 0);
    assert!(rows[0].is_group);
    assert_eq!(rows[1].id, ids[2], "Heart, hija del grupo, después");
    assert_eq!(rows[1].depth, 1);
    assert_eq!(rows[2].id, ids[1], "Ellipse, hija del grupo, después");
    assert_eq!(rows[2].depth, 1);
    assert_eq!(rows[3].id, ids[0], "Rect, raíz, la última (abajo)");
    assert_eq!(rows[3].depth, 0);

    // Pintar cada fila en un frame headless: sin gesto de arrastre no
    // debe devolver ninguna soltada.
    let ctx = egui::Context::default();
    ctx.set_fonts(egui::FontDefinitions::empty());
    let mut drops = Vec::new();
    let _ = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            ..Default::default()
        },
        |ui| {
            for row in &rows {
                if let Some(drop) = row_ui(&mut state, ui, row) {
                    drops.push(drop);
                }
            }
        },
    );
    assert!(drops.is_empty(), "sin arrastre no debe haber soltada");
}

/// Arrastrar una capa «encima de» otra la sitúa justo encima del objetivo
/// en la pila (más arriba en el panel).
#[test]
fn apply_reorder_above_places_the_layer_above_its_target() {
    let (mut state, [a, b, c]) = state_with_three_layers();
    // C (arriba del todo) arrastrada «encima de» A (abajo del todo).
    apply_reorder(&mut state, &[c], Drop::Above(a));
    let page = state
        .doc
        .page()
        .expect("un documento en blanco tiene página");
    let ids: Vec<LayerId> = page.layers.iter().map(|l| l.id).collect();
    assert_eq!(ids, [a, c, b], "C queda entre A y B, justo encima de A");
}

/// Arrastrar una capa «debajo de» otra la sitúa justo debajo del objetivo
/// en la pila (más abajo en el panel).
#[test]
fn apply_reorder_below_places_the_layer_below_its_target() {
    let (mut state, [a, b, c]) = state_with_three_layers();
    // A (abajo del todo) arrastrada «debajo de» C (arriba del todo).
    apply_reorder(&mut state, &[a], Drop::Below(c));
    let page = state
        .doc
        .page()
        .expect("un documento en blanco tiene página");
    let ids: Vec<LayerId> = page.layers.iter().map(|l| l.id).collect();
    assert_eq!(ids, [b, a, c], "A queda entre B y C, justo debajo de C");
}

/// Arrastrar una capa «dentro de» un grupo la mete como último hijo.
#[test]
fn apply_reorder_into_puts_the_layer_inside_the_group() {
    let (mut state, [a, b, c]) = state_with_three_layers();
    // Grupo con A: [Group(A), B, C].
    state.selection = Selection::single(a);
    group_selection(&mut state);
    let page = state
        .doc
        .page()
        .expect("un documento en blanco tiene página");
    let group = page.layers[0].id;
    assert!(page.is_group(group));

    // C arrastrada «dentro de» el grupo: queda como último hijo.
    apply_reorder(&mut state, &[c], Drop::Into(group));
    let page = state
        .doc
        .page()
        .expect("un documento en blanco tiene página");
    assert_eq!(
        page.children_of(Some(group)),
        [a, c],
        "C entra como hijo del grupo, encima de A"
    );
    assert_eq!(
        page.children_of(None),
        [group, b],
        "B sigue como raíz; el grupo y B son las únicas"
    );
}

/// Un arrastre con varias capas seleccionadas (ids sin orden garantizado)
/// conserva su apilamiento relativo dentro del destino.
#[test]
fn apply_reorder_with_several_layers_keeps_their_relative_order() {
    let (mut state, [a, b, c]) = state_with_three_layers();
    // Grupo con A: [Group(A), B, C].
    state.selection = Selection::single(a);
    group_selection(&mut state);
    let page = state
        .doc
        .page()
        .expect("un documento en blanco tiene página");
    let group = page.layers[0].id;

    // B y C (pasadas al revés, como llegan de la selección) «dentro de»
    // el grupo: entran ordenadas por pila, no por el orden del payload.
    apply_reorder(&mut state, &[c, b], Drop::Into(group));
    let page = state
        .doc
        .page()
        .expect("un documento en blanco tiene página");
    assert_eq!(
        page.children_of(Some(group)),
        [a, b, c],
        "el grupo recibe a B y C en su orden de pila"
    );
}

/// El reordenamiento es UN solo paso de deshacer: un undo restaura el
/// orden original completo.
#[test]
fn apply_reorder_is_a_single_undo_step() {
    let (mut state, [a, b, c]) = state_with_three_layers();
    apply_reorder(&mut state, &[c], Drop::Above(a));
    let page = state
        .doc
        .page()
        .expect("un documento en blanco tiene página");
    let ids: Vec<LayerId> = page.layers.iter().map(|l| l.id).collect();
    assert_ne!(ids, [a, b, c], "el reorden debe haber cambiado el orden");

    state.undo();
    let page = state
        .doc
        .page()
        .expect("un documento en blanco tiene página");
    let ids: Vec<LayerId> = page.layers.iter().map(|l| l.id).collect();
    assert_eq!(ids, [a, b, c], "un solo undo restaura el orden original");
}

/// `group_selection` mete las capas seleccionadas en un grupo nuevo que
/// ocupa el hueco de la más alta, conservando su orden de pila, y deja
/// el grupo seleccionado.
#[test]
fn group_selection_groups_the_selected_layers_in_stack_order() {
    let (mut state, [a, b, c]) = state_with_three_layers();
    state.selection = Selection::single(b);
    state.selection.toggle(c);
    group_selection(&mut state);

    let page = state
        .doc
        .page()
        .expect("un documento en blanco tiene página");
    let group = page.layers[1].id; // ocupa el hueco de B y C
    assert!(page.is_group(group));
    assert_eq!(page.children_of(None), [a, group], "A sigue de raíz");
    assert_eq!(
        page.children_of(Some(group)),
        [b, c],
        "los miembros conservan su orden de pila dentro del grupo"
    );
    assert_eq!(
        state.selection,
        Selection::single(group),
        "el grupo queda seleccionado"
    );
}

/// `ungroup_selection` disuelve el grupo seleccionado y sus hijos
/// DIRECTOS vuelven a su hueco en la pila, en el mismo orden.
#[test]
fn ungroup_selection_restores_the_children_in_place_and_order() {
    let (mut state, [a, b, c]) = state_with_three_layers();
    state.selection = Selection::single(b);
    state.selection.toggle(c);
    group_selection(&mut state);

    ungroup_selection(&mut state);
    let page = state
        .doc
        .page()
        .expect("un documento en blanco tiene página");
    let ids: Vec<LayerId> = page.layers.iter().map(|l| l.id).collect();
    assert_eq!(ids, [a, b, c], "los hijos vuelven a su hueco, en su orden");
    assert_eq!(page.children_of(None), [a, b, c]);
}

/// Agrupar es un solo paso de deshacer: un undo disuelve el grupo y
/// restaura el orden y la selección originales.
#[test]
fn group_selection_is_undoable() {
    let (mut state, [a, b, c]) = state_with_three_layers();
    state.selection = Selection::single(b);
    state.selection.toggle(c);
    group_selection(&mut state);
    let page = state
        .doc
        .page()
        .expect("un documento en blanco tiene página");
    assert_eq!(page.layers.len(), 4, "el grupo añade una capa");

    state.undo();
    let page = state
        .doc
        .page()
        .expect("un documento en blanco tiene página");
    let ids: Vec<LayerId> = page.layers.iter().map(|l| l.id).collect();
    assert_eq!(
        ids,
        [a, b, c],
        "un undo disuelve el grupo y restaura el orden"
    );
    assert!(
        state.selection.is_empty(),
        "la selección olvida el grupo deshecho"
    );
}

/// Desagrupar es un solo paso de deshacer: un undo vuelve a crear el
/// grupo con sus hijos dentro.
#[test]
fn ungroup_selection_is_undoable() {
    let (mut state, [_a, b, c]) = state_with_three_layers();
    state.selection = Selection::single(b);
    state.selection.toggle(c);
    group_selection(&mut state);
    let group = state
        .selection
        .primary()
        .expect("agrupar deja el grupo seleccionado");

    ungroup_selection(&mut state);
    let page = state
        .doc
        .page()
        .expect("un documento en blanco tiene página");
    assert_eq!(page.layers.len(), 3, "el grupo se disuelve");

    state.undo();
    let page = state
        .doc
        .page()
        .expect("un documento en blanco tiene página");
    assert_eq!(page.layers.len(), 4, "un undo restaura el grupo");
    assert!(page.is_group(group));
    assert_eq!(
        page.children_of(Some(group)),
        [b, c],
        "el grupo recupera a sus hijos en orden"
    );
}

/// Con varios grupos seleccionados, `ungroup_selection` los disuelve
/// todos y TODO el conjunto es un único paso de deshacer.
#[test]
fn ungroup_selection_dissolves_every_selected_group_in_one_undo_step() {
    let mut state = EditorState::new_blank(800.0, 600.0);
    let mut ids = Vec::new();
    for label in ["Rect", "Ellipse", "Heart", "Star"] {
        insert_item(&mut state, label);
        let page = state
            .doc
            .page()
            .expect("un documento en blanco tiene página");
        ids.push(page.layers.last().expect("insert_item añade una capa").id);
    }
    // Grupo con las dos de abajo y otro con las dos de arriba.
    state.selection = Selection::single(ids[0]);
    state.selection.toggle(ids[1]);
    group_selection(&mut state);
    state.selection = Selection::single(ids[2]);
    state.selection.toggle(ids[3]);
    group_selection(&mut state);
    let page = state
        .doc
        .page()
        .expect("un documento en blanco tiene página");
    let groups: Vec<LayerId> = page
        .layers
        .iter()
        .filter(|l| page.is_group(l.id))
        .map(|l| l.id)
        .collect();
    assert_eq!(groups.len(), 2);

    state.selection = Selection::single(groups[0]);
    state.selection.toggle(groups[1]);
    ungroup_selection(&mut state);
    let page = state
        .doc
        .page()
        .expect("un documento en blanco tiene página");
    assert_eq!(
        page.children_of(None),
        ids,
        "ambos grupos disueltos, las cuatro capas en el orden original"
    );

    state.undo();
    let page = state
        .doc
        .page()
        .expect("un documento en blanco tiene página");
    let groups_now: Vec<LayerId> = page
        .layers
        .iter()
        .filter(|l| page.is_group(l.id))
        .map(|l| l.id)
        .collect();
    assert_eq!(groups_now, groups, "un solo undo restaura los dos grupos");
}

/// Un frame headless de egui que pinta la fila `row` con los `events`
/// dados. El mismo `ctx` sirve para varios frames (el foco persiste).
fn run_row_frame(
    ctx: &egui::Context,
    state: &mut EditorState,
    row: &Row,
    events: Vec<egui::Event>,
) {
    let _ = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            let _ = row_ui(state, ui, row);
        },
    );
}

fn key_press(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}

/// Un clic real sobre la fila, en tres frames (mover el puntero, pulsar
/// y soltar): egui decide hover/clic con lo registrado en el frame
/// anterior, igual que en la app.
fn click_at(ctx: &egui::Context, state: &mut EditorState, row: &Row, pos: egui::Pos2) {
    run_row_frame(ctx, state, row, vec![egui::Event::PointerMoved(pos)]);
    run_row_frame(
        ctx,
        state,
        row,
        vec![egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    run_row_frame(
        ctx,
        state,
        row,
        vec![egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
}

/// Renombrado in situ: editar el nombre y confirmar con Enter emite un
/// `Rename` deshacible que cambia la capa y cierra la edición.
#[test]
fn renaming_commits_on_enter_with_an_undoable_rename() {
    let (mut state, [a, _b, _c]) = state_with_three_layers();
    let original = state.doc.layer(a).expect("la capa existe").name.clone();
    state.rename_edit = Some((a, "Renamed".to_owned(), original.clone()));
    let row = Row {
        id: a,
        depth: 0,
        is_group: false,
        collapsed: false,
    };
    let ctx = egui::Context::default();
    ctx.set_fonts(egui::FontDefinitions::empty());

    // Frame 1: se pinta el TextEdit; se le da el foco para el frame 2.
    run_row_frame(&ctx, &mut state, &row, vec![]);
    let text_id = egui::Id::new(("layer_row", a.raw())).with("rename");
    ctx.memory_mut(|m| m.request_focus(text_id));
    // Frame 2: Enter confirma la edición.
    run_row_frame(&ctx, &mut state, &row, vec![key_press(egui::Key::Enter)]);

    assert!(
        state.rename_edit.is_none(),
        "la edición se cierra al confirmar"
    );
    assert_eq!(state.doc.layer(a).expect("la capa existe").name, "Renamed");
    // El Rename es un paso de deshacer normal.
    state.undo();
    assert_eq!(
        state.doc.layer(a).expect("la capa existe").name,
        original,
        "un undo restaura el nombre original"
    );
}

/// Renombrado in situ: Escape cancela sin aplicar nada.
#[test]
fn renaming_cancels_with_escape() {
    let (mut state, [a, _b, _c]) = state_with_three_layers();
    let original = state.doc.layer(a).expect("la capa existe").name.clone();
    state.rename_edit = Some((a, "Renamed".to_owned(), original.clone()));
    let row = Row {
        id: a,
        depth: 0,
        is_group: false,
        collapsed: false,
    };
    let ctx = egui::Context::default();
    ctx.set_fonts(egui::FontDefinitions::empty());

    run_row_frame(&ctx, &mut state, &row, vec![key_press(egui::Key::Escape)]);

    assert!(state.rename_edit.is_none(), "Escape cierra la edición");
    assert_eq!(
        state.doc.layer(a).expect("la capa existe").name,
        original,
        "Escape no aplica el nombre editado"
    );
}

/// El botón del ojo (primer icono del prefijo: x ∈ [18, 36] en una fila
/// de raíz sin grupo) alterna la visibilidad, y el cambio es deshacible.
#[test]
fn the_eye_button_toggles_visibility_undoably() {
    let (mut state, [a, _b, _c]) = state_with_three_layers();
    let row = Row {
        id: a,
        depth: 0,
        is_group: false,
        collapsed: false,
    };
    assert!(
        state.doc.layer(a).expect("la capa existe").visible,
        "arranca visible"
    );
    let ctx = egui::Context::default();
    ctx.set_fonts(egui::FontDefinitions::empty());

    click_at(&ctx, &mut state, &row, egui::pos2(27.0, 9.0)); // el ojo
    assert!(
        !state.doc.layer(a).expect("la capa existe").visible,
        "el ojo oculta la capa"
    );

    state.undo();
    assert!(
        state.doc.layer(a).expect("la capa existe").visible,
        "un undo restaura la visibilidad"
    );
}

/// El botón del candado (segundo icono del prefijo: x ∈ [44, 62]) alterna
/// el bloqueo, y el cambio es deshacible.
#[test]
fn the_lock_button_toggles_locking_undoably() {
    let (mut state, [a, _b, _c]) = state_with_three_layers();
    let row = Row {
        id: a,
        depth: 0,
        is_group: false,
        collapsed: false,
    };
    assert!(
        !state.doc.layer(a).expect("la capa existe").locked,
        "arranca sin bloqueo"
    );
    let ctx = egui::Context::default();
    ctx.set_fonts(egui::FontDefinitions::empty());

    click_at(&ctx, &mut state, &row, egui::pos2(53.0, 9.0)); // el candado
    assert!(
        state.doc.layer(a).expect("la capa existe").locked,
        "el candado bloquea la capa"
    );

    state.undo();
    assert!(
        !state.doc.layer(a).expect("la capa existe").locked,
        "un undo quita el bloqueo"
    );
}

/// Un frame headless que pinta la tira de pestañas con los `events`
/// dados y devuelve el posible cambio de orden (swap por arrastre).
fn run_tab_frame(
    ctx: &egui::Context,
    active_tab: &mut LeftTab,
    layers_collapsed: &mut bool,
    order: LayersTabOrder,
    collapsed: bool,
    events: Vec<egui::Event>,
) -> Option<LayersTabOrder> {
    let mut out = None;
    let _ = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 400.0),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            out = vertical_tab_strip_ui(ui, active_tab, layers_collapsed, order, collapsed);
        },
    );
    out
}

/// Un clic real sobre la tira en `pos` (mover, pulsar y soltar).
fn click_tab(
    ctx: &egui::Context,
    active_tab: &mut LeftTab,
    layers_collapsed: &mut bool,
    order: LayersTabOrder,
    collapsed: bool,
    pos: egui::Pos2,
) -> Option<LayersTabOrder> {
    let mut out = None;
    for events in [
        vec![egui::Event::PointerMoved(pos)],
        vec![egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        }],
        vec![egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    ] {
        if let Some(o) = run_tab_frame(ctx, active_tab, layers_collapsed, order, collapsed, events)
        {
            out = Some(o);
        }
    }
    out
}

/// Un clic sobre otra pestaña cambia la activa sin tocar el colapso.
#[test]
fn a_click_on_another_tab_changes_the_active_tab() {
    let ctx = egui::Context::default();
    ctx.set_fonts(egui::FontDefinitions::empty());
    let mut active = LeftTab::Page;
    let mut collapsed = false;

    // Con orden PageFirst, el segundo tab (y ∈ [80, 144]) es Layers.
    let swapped = click_tab(
        &ctx,
        &mut active,
        &mut collapsed,
        LayersTabOrder::PageFirst,
        false,
        egui::pos2(18.0, 112.0),
    );

    assert_eq!(active, LeftTab::Layers, "el clic activa la pestaña pulsada");
    assert!(!collapsed, "un clic normal no colapsa");
    assert!(swapped.is_none(), "un clic no reordena");
}

/// Un clic sobre la pestaña YA activa colapsa el panel.
#[test]
fn a_click_on_the_active_tab_collapses_the_panel() {
    let ctx = egui::Context::default();
    ctx.set_fonts(egui::FontDefinitions::empty());
    let mut active = LeftTab::Page;
    let mut collapsed = false;

    // PageFirst: el primer tab (y ∈ [8, 72]) es Page, la activa.
    let swapped = click_tab(
        &ctx,
        &mut active,
        &mut collapsed,
        LayersTabOrder::PageFirst,
        false,
        egui::pos2(18.0, 40.0),
    );

    assert!(collapsed, "clic en la activa colapsa el panel");
    assert_eq!(active, LeftTab::Page, "la activa no cambia");
    assert!(swapped.is_none());
}

/// Con el panel colapsado, un clic en cualquier pestaña lo expande y la
/// activa.
#[test]
fn a_click_expands_a_collapsed_panel_and_activates_the_tab() {
    let ctx = egui::Context::default();
    ctx.set_fonts(egui::FontDefinitions::empty());
    let mut active = LeftTab::Page;
    let mut collapsed = true;

    click_tab(
        &ctx,
        &mut active,
        &mut collapsed,
        LayersTabOrder::PageFirst,
        true,
        egui::pos2(18.0, 112.0), // Layers
    );

    assert!(!collapsed, "el clic expande el panel");
    assert_eq!(active, LeftTab::Layers, "y activa la pestaña pulsada");
}

/// El orden físico de las pestañas sigue el ajuste: con LayersFirst,
/// Layers ocupa el primer tab (arriba del todo), no Page.
#[test]
fn the_tab_order_follows_the_setting() {
    let ctx = egui::Context::default();
    ctx.set_fonts(egui::FontDefinitions::empty());

    // LayersFirst: el tab superior (y ∈ [8, 72]) es Layers.
    let mut active = LeftTab::Page;
    let mut collapsed = false;
    click_tab(
        &ctx,
        &mut active,
        &mut collapsed,
        LayersTabOrder::LayersFirst,
        false,
        egui::pos2(18.0, 40.0),
    );
    assert_eq!(active, LeftTab::Layers, "Layers va primero con LayersFirst");

    // PageFirst: el tab superior es Page.
    let ctx = egui::Context::default();
    ctx.set_fonts(egui::FontDefinitions::empty());
    let mut active = LeftTab::Layers;
    let mut collapsed = false;
    click_tab(
        &ctx,
        &mut active,
        &mut collapsed,
        LayersTabOrder::PageFirst,
        false,
        egui::pos2(18.0, 40.0),
    );
    assert_eq!(active, LeftTab::Page, "Page va primero con PageFirst");
}

/// Arrastrar una pestaña sobre otra (superado el umbral de clic) no
/// cambia la activa, pero devuelve el orden intercambiado para que el
/// llamador lo persista en los ajustes.
#[test]
fn dragging_a_tab_over_another_returns_the_swapped_order() {
    let ctx = egui::Context::default();
    ctx.set_fonts(egui::FontDefinitions::empty());
    let mut active = LeftTab::Page;
    let mut collapsed = false;
    let order = LayersTabOrder::PageFirst;

    // PageFirst: Page (y ∈ [8, 72]) arrastrada sobre Layers (y ∈ [80, 144]).
    let start = egui::pos2(18.0, 40.0);
    let target = egui::pos2(18.0, 112.0);
    let mut out = None;
    for events in [
        vec![egui::Event::PointerMoved(start)],
        vec![egui::Event::PointerButton {
            pos: start,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        }],
        vec![egui::Event::PointerMoved(target)],
        vec![egui::Event::PointerButton {
            pos: target,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    ] {
        if let Some(o) = run_tab_frame(&ctx, &mut active, &mut collapsed, order, false, events) {
            out = Some(o);
        }
    }

    assert_eq!(
        out,
        Some(LayersTabOrder::LayersFirst),
        "el arrastre devuelve el orden intercambiado"
    );
    assert_eq!(
        active,
        LeftTab::Page,
        "la pestaña activa no cambia con el arrastre"
    );
    assert!(!collapsed, "el arrastre no colapsa");
}

/// Un frame headless que pinta la cuadrícula Insert con los `events`
/// dados, sobre un área de `width` (la mitad del panel, como en la app).
fn run_insert_frame(
    ctx: &egui::Context,
    state: &mut EditorState,
    width: f32,
    events: Vec<egui::Event>,
) {
    let _ = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 600.0),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            insert_tab_ui(state, ui);
        },
    );
}

/// Un clic real sobre un tile de Insert (mover, pulsar y soltar).
fn click_insert_tile(ctx: &egui::Context, state: &mut EditorState, width: f32, pos: egui::Pos2) {
    run_insert_frame(ctx, state, width, vec![egui::Event::PointerMoved(pos)]);
    run_insert_frame(
        ctx,
        state,
        width,
        vec![egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    run_insert_frame(
        ctx,
        state,
        width,
        vec![egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
}

/// Un clic sobre cada tile de la cuadrícula Insert inserta la capa de la
/// etiqueta de ESE tile (el clic llama a `insert_item` con su label).
#[test]
fn clicking_each_insert_tile_inserts_the_matching_layer() {
    let width = 400.0;
    // El mismo layout que `insert_tab_ui`: dos columnas de tiles.
    let pad = sidebar::PANEL_PAD * 2.0;
    let gap = 8.0;
    let tile_w = ((width - pad - gap) * 0.5).max(1.0);
    let x0 = pad / 2.0;

    for (i, item) in INSERT_ITEMS.iter().enumerate() {
        let expected = INSERT_CASES
            .iter()
            .find(|c| c.label == item.label)
            .expect("toda etiqueta del panel tiene caso esperado");
        let mut state = EditorState::new_blank(800.0, 600.0);
        let ctx = egui::Context::default();
        ctx.set_fonts(egui::FontDefinitions::empty());
        // Centro del tile: fila = i/2, columna = i%2.
        let center = egui::pos2(
            x0 + (i % 2) as f32 * (tile_w + gap) + tile_w / 2.0,
            (i / 2) as f32 * (INSERT_TILE_H + 10.0) + INSERT_TILE_H / 2.0,
        );
        click_insert_tile(&ctx, &mut state, width, center);

        let page = state
            .doc
            .page()
            .expect("un documento en blanco tiene página");
        assert_eq!(
            page.layers.len(),
            1,
            "{}: el clic inserta exactamente una capa",
            item.label
        );
        assert_eq!(
            page.layers[0].name, expected.name,
            "{}: el clic inserta la capa de la etiqueta del tile",
            item.label
        );
        assert_eq!(page.layers[0].transform.width, expected.w, "{}", item.label);
        assert_eq!(
            page.layers[0].transform.height, expected.h,
            "{}",
            item.label
        );
    }
}

#[test]
fn ordered_tabs_follows_the_setting() {
    assert_eq!(
        ordered_tabs(LayersTabOrder::PageFirst),
        [
            LeftTab::Page,
            LeftTab::Layers,
            LeftTab::Insert,
            LeftTab::Images
        ]
    );
    assert_eq!(
        ordered_tabs(LayersTabOrder::LayersFirst),
        [
            LeftTab::Layers,
            LeftTab::Page,
            LeftTab::Insert,
            LeftTab::Images
        ]
    );
}

#[test]
fn each_tab_appears_exactly_once() {
    let order = ordered_tabs(LayersTabOrder::LayersFirst);
    assert!(order.contains(&LeftTab::Page));
    assert!(order.contains(&LeftTab::Layers));
    assert!(order.contains(&LeftTab::Insert));
    assert!(order.contains(&LeftTab::Images));
}

// ---- Humo de pintado e interacción tras la partición tab_strip/tab_draw ----

/// Evento de pulsado/soltado del botón primario en `pos`.
fn pointer_button(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

/// Un frame headless que pinta la tira y devuelve el `FullOutput` completo,
/// para contar shapes y detectar que la fase de pintado sigue conectada.
fn run_tab_output(
    ctx: &egui::Context,
    active_tab: &mut LeftTab,
    layers_collapsed: &mut bool,
    order: LayersTabOrder,
    collapsed: bool,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 400.0),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            let _ = vertical_tab_strip_ui(ui, active_tab, layers_collapsed, order, collapsed);
        },
    )
}

/// Escape a mitad de un arrastre lo cancela: sin swap, sin colapso, y la
/// tira no queda «pegada» — el clic siguiente funciona con normalidad.
#[test]
fn escape_cancels_an_ongoing_tab_drag() {
    let ctx = egui::Context::default();
    ctx.set_fonts(egui::FontDefinitions::empty());
    let mut active = LeftTab::Page;
    let mut collapsed = false;
    let order = LayersTabOrder::PageFirst;
    let start = egui::pos2(18.0, 40.0);
    let target = egui::pos2(18.0, 112.0);

    let mut swapped = None;
    for events in [
        vec![egui::Event::PointerMoved(start)],
        vec![pointer_button(start, true)],
        // Supera el umbral de 6 px: es un arrastre, no un clic.
        vec![egui::Event::PointerMoved(target)],
        vec![key_press(egui::Key::Escape)],
        vec![pointer_button(target, false)],
    ] {
        if let Some(o) = run_tab_frame(&ctx, &mut active, &mut collapsed, order, false, events) {
            swapped = Some(o);
        }
    }
    assert!(swapped.is_none(), "Escape cancela el intercambio");
    assert!(!collapsed, "Escape no colapsa el panel");

    // El gesto murió de verdad: un clic normal después responde normal.
    click_tab(&ctx, &mut active, &mut collapsed, order, false, target);
    assert_eq!(
        active,
        LeftTab::Layers,
        "tras cancelar con Escape, la tira sigue respondiendo"
    );
    assert!(!collapsed);
}

/// La tira PINTA en ambos estados (regresión visual: si la fase de pintado
/// se desconectara del `PaintPass`, el conteo de shapes caería a ~0).
#[test]
fn the_tab_strip_paints_in_expanded_and_collapsed_states() {
    let ctx = egui::Context::default();
    ctx.set_fonts(egui::FontDefinitions::empty());
    let mut active = LeftTab::Page;
    let mut collapsed = false;

    let out = run_tab_output(
        &ctx,
        &mut active,
        &mut collapsed,
        LayersTabOrder::PageFirst,
        false,
        vec![],
    );
    assert!(
        out.shapes.len() >= 6,
        "expandida: {} shapes, esperaba ≥ 6 (4 pestañas con icono + fondo e indicador de la activa)",
        out.shapes.len()
    );

    let out = run_tab_output(
        &ctx,
        &mut active,
        &mut collapsed,
        LayersTabOrder::PageFirst,
        true,
        vec![],
    );
    assert!(
        out.shapes.len() >= 6,
        "colapsada: {} shapes, la tira también se pinta",
        out.shapes.len()
    );
}

/// El panel COMPLETO (`left_panel_ui`) se renderiza headless en sus cuatro
/// pestañas sin pánico y pintando en cada una — humo de integración que
/// ejercita la orquestación real (tira + cuerpo de la pestaña activa).
#[test]
fn left_panel_renders_every_tab_without_panicking() {
    let (tx, _rx) = std::sync::mpsc::channel();
    let mut state = EditorState::new_blank(800.0, 600.0);
    let mut collapsed = false;
    let ctx = egui::Context::default();
    ctx.set_fonts(egui::FontDefinitions::empty());

    for tab in [
        LeftTab::Page,
        LeftTab::Layers,
        LeftTab::Insert,
        LeftTab::Images,
    ] {
        state.active_left_tab = tab;
        let out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(420.0, 600.0),
                )),
                ..Default::default()
            },
            |ui| {
                let _ = left_panel_ui(
                    &mut state,
                    ui,
                    &mut collapsed,
                    LayersTabOrder::PageFirst,
                    &tx,
                );
            },
        );
        assert!(
            !out.shapes.is_empty(),
            "{tab:?}: el panel debe pintar algo en cada pestaña"
        );
    }
}
