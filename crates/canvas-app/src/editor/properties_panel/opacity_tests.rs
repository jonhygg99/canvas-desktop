//! Tests de la opacidad por capa. Sin GPU ni egui: conducen las mismas dos
//! funciones que usa el slider (`set_opacity_live` durante el arrastre,
//! `commit_opacity` al soltar), no una imitación de ellas.

use canvas_core::{LayerContent, LayerId, Selection, ShapeContent, ShapeKind, Transform};
use eframe::egui;

use super::effects::{commit_opacity, set_opacity_live};
use super::{properties_ui, EditorState};

/// Dibuja el panel entero en un frame de egui de verdad (CPU, sin backend) y
/// devuelve el texto que ha pintado. Es lo más cerca que se puede estar de
/// «abrir la app y mirar el panel» sin un humano delante.
fn painted_text(state: &mut EditorState) -> String {
    let ctx = egui::Context::default();
    let out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 1200.0),
            )),
            ..Default::default()
        },
        |ui| properties_ui(state, ui),
    );
    let mut text = String::new();
    for shape in out.shapes {
        collect_text(&shape.shape, &mut text);
    }
    text
}

fn collect_text(shape: &egui::epaint::Shape, out: &mut String) {
    match shape {
        egui::epaint::Shape::Text(t) => {
            out.push_str(t.galley.text());
            out.push('\n');
        }
        egui::epaint::Shape::Vec(v) => {
            for s in v {
                collect_text(s, out);
            }
        }
        _ => {}
    }
}

fn layer(state: &mut EditorState, name: &str) -> LayerId {
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

fn opacity_of(state: &EditorState, id: LayerId) -> f32 {
    state.doc.layer(id).expect("la capa existe").opacity
}

fn state_with_layer() -> (EditorState, LayerId) {
    let mut state = EditorState::new_blank(800.0, 600.0);
    let id = layer(&mut state, "capa");
    assert_eq!(opacity_of(&state, id), 1.0, "una capa nace opaca");
    (state, id)
}

#[test]
fn dragging_the_slider_updates_the_document_live() {
    // Vista previa inmediata: el lienzo tiene que reflejarlo mientras se
    // arrastra, sin esperar a soltar.
    let (mut state, id) = state_with_layer();

    set_opacity_live(&mut state, id, 0.4);

    assert!((opacity_of(&state, id) - 0.4).abs() < 1e-6);
}

#[test]
fn a_whole_drag_is_a_single_undo_step_back_to_the_original() {
    // El gesto pasa por muchos valores intermedios; deshacer debe volver al
    // de partida, no al penúltimo frame del arrastre.
    let (mut state, id) = state_with_layer();

    for pct in [0.9, 0.7, 0.5, 0.3] {
        set_opacity_live(&mut state, id, pct);
    }
    commit_opacity(&mut state);

    assert!((opacity_of(&state, id) - 0.3).abs() < 1e-6);

    state.undo();

    assert_eq!(opacity_of(&state, id), 1.0, "no volvió al valor original");
    assert!(
        !state.history.can_undo(),
        "el arrastre dejó más de un paso de deshacer"
    );
}

#[test]
fn a_drag_that_ends_where_it_started_leaves_no_undo_step() {
    let (mut state, id) = state_with_layer();

    set_opacity_live(&mut state, id, 0.5);
    set_opacity_live(&mut state, id, 1.0);
    commit_opacity(&mut state);

    assert_eq!(opacity_of(&state, id), 1.0);
    assert!(
        !state.history.can_undo(),
        "un gesto que no cambia nada no debe ensuciar el historial"
    );
}

#[test]
fn committing_without_a_pending_edit_does_nothing() {
    let (mut state, _id) = state_with_layer();

    commit_opacity(&mut state);

    assert!(!state.history.can_undo());
}

#[test]
fn the_value_is_clamped_to_the_valid_range() {
    // El slider no deja pasar de 0..=100, pero el modelo tampoco debe aceptar
    // nada fuera de rango por otra vía.
    let (mut state, id) = state_with_layer();

    set_opacity_live(&mut state, id, 2.5);
    assert_eq!(opacity_of(&state, id), 1.0);

    set_opacity_live(&mut state, id, -1.0);
    assert_eq!(opacity_of(&state, id), 0.0);
}

#[test]
fn an_edit_in_flight_keeps_the_editor_busy() {
    // `is_idle()` frena saltar de lienzo y guardar a mitad de un gesto: si la
    // opacidad no contara, se podría guardar el documento a medio arrastre.
    let (mut state, id) = state_with_layer();
    assert!(state.is_idle());

    set_opacity_live(&mut state, id, 0.5);
    assert!(!state.is_idle(), "un arrastre en curso no es estar ocioso");

    commit_opacity(&mut state);
    assert!(state.is_idle());
}

#[test]
fn switching_layers_mid_drag_commits_the_edit_of_the_first_one() {
    // Lo que hace el panel cuando cambia la selección sin haber soltado.
    let (mut state, first) = state_with_layer();
    let second = layer(&mut state, "otra");

    set_opacity_live(&mut state, first, 0.25);
    commit_opacity(&mut state); // el panel lo llama al ver que cambió la capa

    assert!((opacity_of(&state, first) - 0.25).abs() < 1e-6);
    assert_eq!(opacity_of(&state, second), 1.0, "no debía tocar a la otra");
    assert!(
        state.history.can_undo(),
        "el cambio se perdió sin registrar"
    );
}

#[test]
fn a_group_can_have_its_own_opacity() {
    // Es la única propiedad de esa sección del panel que también vale para un
    // grupo: el render multiplica la suya por la de sus ancestros.
    let mut state = EditorState::new_blank(800.0, 600.0);
    let grupo = state
        .doc
        .add_layer(
            "grupo",
            Transform::new(0.0, 0.0, 10.0, 10.0),
            LayerContent::Group(canvas_core::GroupContent::default()),
        )
        .expect("hay página");
    let hija = layer(&mut state, "hija");
    state
        .doc
        .page_mut()
        .expect("hay página")
        .move_subtree(hija, Some(grupo), 0)
        .expect("mover la hija dentro del grupo");

    set_opacity_live(&mut state, grupo, 0.5);
    commit_opacity(&mut state);

    let page = state.doc.page().expect("hay página");
    assert!((page.effective_opacity(hija) - 0.5).abs() < 1e-6);
}

#[test]
fn nested_opacities_multiply_down_the_group_chain() {
    let mut state = EditorState::new_blank(800.0, 600.0);
    let grupo = state
        .doc
        .add_layer(
            "grupo",
            Transform::new(0.0, 0.0, 10.0, 10.0),
            LayerContent::Group(canvas_core::GroupContent::default()),
        )
        .expect("hay página");
    let hija = layer(&mut state, "hija");
    state
        .doc
        .page_mut()
        .expect("hay página")
        .move_subtree(hija, Some(grupo), 0)
        .expect("mover la hija dentro del grupo");

    set_opacity_live(&mut state, grupo, 0.5);
    commit_opacity(&mut state);
    set_opacity_live(&mut state, hija, 0.5);
    commit_opacity(&mut state);

    let page = state.doc.page().expect("hay página");
    assert!(
        (page.effective_opacity(hija) - 0.25).abs() < 1e-6,
        "0.5 dentro de un grupo al 0.5 tiene que dar 0.25"
    );
}

#[test]
fn the_panel_actually_shows_an_opacity_control_for_a_layer() {
    let (mut state, id) = state_with_layer();
    state.selection = Selection::single(id);

    let painted = painted_text(&mut state);

    assert!(
        painted.contains("Opacity"),
        "el panel no pintó el control de opacidad; pintó:
{painted}"
    );
}

#[test]
fn the_panel_shows_the_opacity_control_for_a_group_too() {
    // El resto de la sección se salta los grupos con un retorno temprano; la
    // opacidad va ANTES de ese retorno justo para no perderse aquí.
    let mut state = EditorState::new_blank(800.0, 600.0);
    let grupo = state
        .doc
        .add_layer(
            "grupo",
            Transform::new(0.0, 0.0, 10.0, 10.0),
            LayerContent::Group(canvas_core::GroupContent::default()),
        )
        .expect("hay página");
    state.selection = Selection::single(grupo);

    let painted = painted_text(&mut state);

    assert!(
        painted.contains("Opacity"),
        "un grupo se quedó sin control de opacidad; pintó:
{painted}"
    );
    assert!(
        painted.contains("Group: grupo"),
        "y debe seguir mostrando el aviso del grupo"
    );
}
