//! Tests de `handle_shortcuts`: los atajos se prueban con frames headless de
//! egui, inyectando los eventos de teclado que produciría el usuario.

use eframe::egui;

use canvas_core::{LayerContent, Selection, ShapeContent};

use super::{DeckNav, EditorState};

fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

fn run_with_keys(state: &mut EditorState, events: Vec<egui::Event>) {
    let ctx = egui::Context::default();
    ctx.set_fonts(egui::FontDefinitions::empty());
    let _ = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            events,
            ..Default::default()
        },
        |_ui| {
            state.handle_shortcuts(&ctx, false, false);
        },
    );
}

/// Inserta una capa por el camino real (con paso de deshacer) y devuelve su
/// id.
fn inserted(state: &mut EditorState) -> canvas_core::LayerId {
    state.insert_layer_centered("Shape", 100.0, 80.0, LayerContent::Shape(ShapeContent::default()));
    let page = state.doc.page().expect("un documento en blanco tiene página");
    page.layers.last().expect("insert_layer_centered añade una capa").id
}

fn layer_count(state: &EditorState) -> usize {
    state.doc.page().expect("hay página").layers.len()
}

#[test]
fn ctrl_z_undoes_and_shift_z_or_ctrl_y_redoes() {
    let mut state = EditorState::new_blank(800.0, 600.0);
    inserted(&mut state);
    inserted(&mut state);
    assert_eq!(layer_count(&state), 2);

    run_with_keys(&mut state, vec![key(egui::Key::Z, egui::Modifiers::COMMAND)]);
    assert_eq!(layer_count(&state), 1, "Ctrl+Z deshace");

    run_with_keys(
        &mut state,
        vec![key(
            egui::Key::Z,
            egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
        )],
    );
    assert_eq!(layer_count(&state), 2, "Ctrl+Shift+Z rehace");

    run_with_keys(&mut state, vec![key(egui::Key::Z, egui::Modifiers::COMMAND)]);
    assert_eq!(layer_count(&state), 1);

    run_with_keys(&mut state, vec![key(egui::Key::Y, egui::Modifiers::COMMAND)]);
    assert_eq!(layer_count(&state), 2, "Ctrl+Y rehace");
}

#[test]
fn ctrl_g_groups_and_ctrl_shift_g_ungroups() {
    let mut state = EditorState::new_blank(800.0, 600.0);
    let a = inserted(&mut state);
    let b = inserted(&mut state);
    state.selection = Selection::single(a);
    state.selection.toggle(b);

    run_with_keys(&mut state, vec![key(egui::Key::G, egui::Modifiers::COMMAND)]);
    let page = state.doc.page().unwrap();
    let groups = page.layers.iter().filter(|l| matches!(l.content, LayerContent::Group(_))).count();
    assert_eq!(groups, 1, "Ctrl+G agrupa la selección");

    run_with_keys(
        &mut state,
        vec![key(
            egui::Key::G,
            egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
        )],
    );
    let page = state.doc.page().unwrap();
    let groups = page.layers.iter().filter(|l| matches!(l.content, LayerContent::Group(_))).count();
    assert_eq!(groups, 0, "Ctrl+Shift+G desagrupa");
}

#[test]
fn ctrl_backslash_toggles_the_layers_panel_flag() {
    let mut state = EditorState::new_blank(800.0, 600.0);
    assert!(!state.layers_panel_toggle);

    run_with_keys(&mut state, vec![key(egui::Key::Backslash, egui::Modifiers::COMMAND)]);
    assert!(state.layers_panel_toggle, "Ctrl+\\ pide plegar/desplegar el panel");
}

#[test]
fn delete_and_backspace_remove_the_selection() {
    let mut state = EditorState::new_blank(800.0, 600.0);
    let a = inserted(&mut state);
    state.selection = Selection::single(a);
    run_with_keys(&mut state, vec![key(egui::Key::Delete, egui::Modifiers::NONE)]);
    assert_eq!(layer_count(&state), 0, "Delete borra la selección");

    let b = inserted(&mut state);
    state.selection = Selection::single(b);
    run_with_keys(&mut state, vec![key(egui::Key::Backspace, egui::Modifiers::NONE)]);
    assert_eq!(layer_count(&state), 0, "Backspace borra la selección");
}

#[test]
fn page_keys_set_the_deck_navigation() {
    let mut state = EditorState::new_blank(800.0, 600.0);

    run_with_keys(&mut state, vec![key(egui::Key::PageDown, egui::Modifiers::NONE)]);
    assert!(matches!(state.deck_nav, Some(DeckNav::Next)));

    run_with_keys(&mut state, vec![key(egui::Key::PageUp, egui::Modifiers::NONE)]);
    assert!(matches!(state.deck_nav, Some(DeckNav::Prev)));

    run_with_keys(&mut state, vec![key(egui::Key::Home, egui::Modifiers::NONE)]);
    assert!(matches!(state.deck_nav, Some(DeckNav::First)));

    run_with_keys(&mut state, vec![key(egui::Key::End, egui::Modifiers::NONE)]);
    assert!(matches!(state.deck_nav, Some(DeckNav::Last)));
}

#[test]
fn ctrl_a_selects_all_the_root_layers() {
    let mut state = EditorState::new_blank(800.0, 600.0);
    inserted(&mut state);
    inserted(&mut state);
    inserted(&mut state);
    // Solo la última insertada queda seleccionada de entrada.
    assert_eq!(state.selection.len(), 1);

    run_with_keys(&mut state, vec![key(egui::Key::A, egui::Modifiers::COMMAND)]);
    assert_eq!(state.selection.len(), 3, "Ctrl+A selecciona todas las raíces");
}

#[test]
fn ctrl_d_duplicates_the_selection() {
    let mut state = EditorState::new_blank(800.0, 600.0);
    let a = inserted(&mut state);
    state.selection = Selection::single(a);
    assert_eq!(layer_count(&state), 1);

    run_with_keys(&mut state, vec![key(egui::Key::D, egui::Modifiers::COMMAND)]);
    assert_eq!(layer_count(&state), 2, "Ctrl+D duplica la capa seleccionada");
    assert_eq!(state.selection.len(), 1, "la copia queda seleccionada");
}

#[test]
fn ctrl_z_is_left_for_the_text_edit_while_renaming_a_layer() {
    let mut state = EditorState::new_blank(800.0, 600.0);
    let a = inserted(&mut state);
    // Renombrado en curso: Ctrl+Z debe quedarse en el TextEdit, no robar el
    // undo del documento.
    state.rename_edit = Some((a, "new name".to_owned(), "Shape".to_owned()));

    run_with_keys(&mut state, vec![key(egui::Key::Z, egui::Modifiers::COMMAND)]);

    assert!(
        state.doc.layer(a).is_ok(),
        "Ctrl+Z no debe deshacer la inserción mientras se renombra"
    );
    assert_eq!(layer_count(&state), 1);
}
