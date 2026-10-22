//! El cierre de ventana es la última barrera contra un mp4 a medias: con un
//! clip escribiéndose se CANCELA (y se explica por qué), y con la cola vacía
//! deja cerrar. Hermano de `window.rs` (convención `*_tests.rs`).

use std::path::PathBuf;

use eframe::egui;

use super::AppInner;
use crate::app::{View, Workspace};
use crate::{deck, editor};

/// Un editor de clip con baraja de proyecto (una ranura) y nada en vuelo.
fn editor_workspace() -> Workspace {
    let mut ws = Workspace::new(egui::ViewportId::ROOT);
    ws.view = View::Editor(Box::new(editor::EditorState::new_blank_image(
        1080.0, 1920.0,
    )));
    ws.deck = deck::Deck::new_design((1080.0, 1920.0));
    ws.deck.folder = Some(PathBuf::from("proyecto"));
    ws
}

/// Con un lienzo en la cola de escritura la ventana NO se cierra: se cancela
/// el cierre y se deja el motivo en el canal de avisos (una X muda parece un
/// botón roto y empuja a matar el proceso, justo el archivo que se estaba
/// protegiendo).
#[test]
fn a_canvas_still_being_written_cancels_the_close_and_says_why() {
    let ctx = egui::Context::default();
    let mut ws = editor_workspace();
    let generation = ws.deck.generation();
    ws.save.writes.enqueue(generation, [ws.deck.slots[0].id]);
    assert!(ws.save.writes.busy(generation));

    AppInner::confirm_window_close(&mut ws, &ctx, false);

    assert!(
        !ws.save.allow_close,
        "no se cierra mientras un lienzo se escribe"
    );
    assert!(!ws.close_requested);
    assert!(
        ws.unsaved_dialog.is_none(),
        "primero termina el render: no se pregunta por cambios"
    );
    let View::Editor(state) = &ws.view else {
        panic!("sigue en el editor");
    };
    let notice = state.notice.clone().unwrap_or_default();
    assert!(
        notice.contains("Closing") && notice.contains("canvases"),
        "el aviso debe decir qué está esperando: {notice:?}"
    );
}

/// Nada en vuelo y un lienzo limpio: el cierre continúa (la hija se marca
/// para que la raíz la retire).
#[test]
fn with_the_write_queue_empty_the_close_goes_through() {
    let ctx = egui::Context::default();
    let mut ws = editor_workspace();
    assert!(!ws.save.writes.busy(ws.deck.generation()));

    AppInner::confirm_window_close(&mut ws, &ctx, false);

    assert!(ws.save.allow_close);
    assert!(
        ws.close_requested,
        "una ventana hija se marca para que la raíz la retire"
    );
    assert!(ws.unsaved_dialog.is_none());
}
