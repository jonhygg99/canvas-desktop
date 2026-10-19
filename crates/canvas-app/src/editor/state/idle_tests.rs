//! `is_idle` frente a «puedo salir»: la distinción que arregla el Back
//! bloqueado. Un lienzo recién abierto está ocioso; una bandera de UI a
//! medias (campo de panel, renombrado, gesto) impide `is_idle` pero NO debe
//! impedir abandonar el editor, porque `prepare_for_navigation` la consolida
//! o la descarta.
use canvas_core::{LayerId, Transform};
use eframe::egui;

use super::EditorState;

fn blank() -> EditorState {
    EditorState::new_blank_image(100.0, 100.0)
}

/// Mutador de una bandera transitoria de UI, para recorrer los casos en tabla.
type SetFlag = fn(&mut EditorState);

#[test]
fn a_fresh_editor_is_idle_and_can_leave() {
    let state = blank();
    assert_eq!(state.idle_blocker(), None);
    assert!(state.is_idle());
    assert_eq!(state.blocks_leaving(), None);
}

#[test]
fn transient_ui_state_blocks_idleness_but_not_leaving() {
    let cases: [(&'static str, SetFlag); 4] = [
        ("an uncommitted transform field", |state| {
            state.panel_edit = Some((LayerId::from_raw(1), Transform::new(0.0, 0.0, 10.0, 10.0)));
        }),
        ("a layer rename being typed", |state| {
            state.rename_edit =
                Some((LayerId::from_raw(1), "nuevo".to_owned(), "viejo".to_owned()));
        }),
        ("a file rename being typed", |state| {
            state.file_rename_edit = Some("nuevo".to_owned());
        }),
        ("an uncommitted page-size field", |state| {
            state.page_edit = Some((800.0, 600.0));
        }),
    ];
    for (expected, set) in cases {
        let mut state = blank();
        set(&mut state);
        assert_eq!(state.idle_blocker(), Some(expected), "{expected}");
        assert!(!state.is_idle(), "{expected}");
        assert_eq!(
            state.blocks_leaving(),
            None,
            "{expected}: una bandera de UI no debe bloquear la salida"
        );
    }
}

/// El arreglo de «Back to gallery se queda esperando»: cualquier combinación
/// de banderas transitorias se resuelve en `prepare_for_navigation`, así que
/// tras pasar por ella el editor vuelve a estar ocioso y no bloquea nada.
#[test]
fn preparing_for_navigation_releases_every_transient_flag() {
    let mut state = blank();
    let id = LayerId::from_raw(1);
    state.panel_edit = Some((id, Transform::new(0.0, 0.0, 10.0, 10.0)));
    state.page_edit = Some((800.0, 600.0));
    state.opacity_edit = Some((id, 0.5));
    state.blur_edit = Some((id, 0.0));
    state.color_edit = Some((id, Default::default()));
    state.shadow_edit = Some((id, None));
    state.rename_edit = Some((id, "a".to_owned(), "b".to_owned()));
    state.file_rename_edit = Some("b".to_owned());
    state.gesture = super::Gesture::Move {
        layer: id,
        start: Transform::new(0.0, 0.0, 10.0, 10.0),
        origin: egui::Pos2::ZERO,
    };
    assert!(state.idle_blocker().is_some());

    crate::editor::prepare_for_navigation(&mut state);

    assert_eq!(state.idle_blocker(), None, "queda una bandera pegada");
    assert!(state.is_idle());
    assert_eq!(state.blocks_leaving(), None);
}

#[test]
fn saving_and_exporting_do_block_leaving() {
    let mut state = blank();
    state.saving = true;
    assert_eq!(state.idle_blocker(), Some("a save that is still writing"));
    assert_eq!(state.blocks_leaving(), Some("a save that is still writing"));

    let mut state = blank();
    state.exporting = true;
    assert_eq!(
        state.blocks_leaving(),
        Some("an export that is still running")
    );

    // `idle_blocker` recorre las mismas condiciones en el mismo orden que
    // `is_idle_except_save`: sin guardado, exportar también la impide.
    assert!(!state.is_idle_except_save());
}
