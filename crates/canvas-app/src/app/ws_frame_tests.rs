//! Reglas del frame de una ventana que se pueden fijar sin UI: el contrato de
//! la geometría persistida (`capture_geometry`: posición del rect EXTERIOR
//! con fallback al interior y tamaño SIEMPRE del interior, lo que
//! `with_inner_size`/`StoredWorkspace::size` entienden — mezclar el exterior
//! como tamaño era la mitad del bucle de redimensionado en Windows, ver
//! `spawn_child_viewports`) y las compuertas de la navegación aparcada
//! (`can_drain_deferred_nav`).

use eframe::egui;

use super::capture_geometry;

#[test]
fn closing_root_does_not_discard_a_dirty_child() {
    use crate::app::{UnsavedDialog, View, Workspace};
    use std::sync::{Arc, Mutex};
    let mut root = Workspace::new(egui::ViewportId::ROOT);
    let mut child = Workspace::new(egui::ViewportId::from_hash_of("dirty-child"));
    let mut state = crate::editor::EditorState::new_blank(20.0, 20.0);
    state.history.mark_unsaved();
    child.view = View::Editor(Box::new(state));
    // Un diálogo ya pendiente evita abrir un modal nativo durante el test.
    child.unsaved_dialog = Some(UnsavedDialog::WindowClose);
    let child = Arc::new(Mutex::new(child));
    let workspaces = vec![
        Arc::new(Mutex::new(Workspace::new(egui::ViewportId::ROOT))),
        child.clone(),
    ];
    super::request_window_close(&workspaces, &mut root, &egui::Context::default(), true);
    assert!(!child.lock().unwrap().close_requested);
    assert!(!child.lock().unwrap().save.allow_close);
}

fn rect(min: (f32, f32), max: (f32, f32)) -> egui::Rect {
    egui::Rect::from_min_max(egui::pos2(min.0, min.1), egui::pos2(max.0, max.1))
}

/// Rect exterior con marco (~40 px de decoración) alrededor del interior:
/// la posición se toma del exterior y el tamaño del INTERIOR (1200×760,
/// no 1200×800).
#[test]
fn position_from_outer_size_from_inner() {
    let outer = rect((100.0, 120.0), (1300.0, 920.0));
    let inner = rect((100.0, 140.0), (1300.0, 900.0));
    assert_eq!(
        capture_geometry(Some(outer), Some(inner)),
        Some((egui::pos2(100.0, 120.0), egui::vec2(1200.0, 760.0)))
    );
}

/// Solo rect interior (el exterior aún no lo reporta el SO): ambos valores
/// salen de él.
#[test]
fn only_inner_rect_supplies_both() {
    let inner = rect((50.0, 60.0), (850.0, 660.0));
    assert_eq!(
        capture_geometry(None, Some(inner)),
        Some((egui::pos2(50.0, 60.0), egui::vec2(800.0, 600.0)))
    );
}

/// Sin ningún rect (ventana sin mapear): `None` y el llamador conserva la
/// geometría anterior en vez de corromperla.
#[test]
fn no_rects_is_none() {
    assert_eq!(capture_geometry(None, None), None);
}

/// Solo rect exterior: el tamaño interior es desconocido, así que `None`
/// — el tamaño NUNCA se toma del exterior (sería volver al bucle de
/// redimensionado).
#[test]
fn size_never_comes_from_outer() {
    let outer = rect((0.0, 0.0), (1000.0, 800.0));
    assert_eq!(capture_geometry(Some(outer), None), None);
}

/// La navegación aparcada no se saca mientras un clip de FONDO se escribe: su
/// render no bloquea el lienzo activo, así que sin esta compuerta se sacaba y
/// se volvía a aparcar en cada frame (y dos destinos distintos se
/// intercambiaban el orden). Con la cola libre sí se drena.
#[test]
fn a_background_clip_render_holds_the_parked_navigation() {
    use crate::app::{View, Workspace};
    let mut ws = Workspace::new(egui::ViewportId::ROOT);
    ws.view = View::Editor(Box::new(crate::editor::EditorState::new_blank_image(
        100.0, 100.0,
    )));
    ws.deck = crate::deck::Deck::new_design((1080.0, 1920.0));
    assert!(
        super::can_drain_deferred_nav(&ws),
        "un editor ocioso reintenta la navegación"
    );

    let generation = ws.deck.generation();
    ws.save.writes.enqueue(generation, [ws.deck.slots[0].id]);
    assert!(
        !super::can_drain_deferred_nav(&ws),
        "un clip escribiéndose no debe sacar la navegación de la cola"
    );

    ws.save.writes.next();
    ws.save.writes.finished(ws.deck.slots[0].id);
    assert!(
        super::can_drain_deferred_nav(&ws),
        "terminado el render, la navegación vuelve a intentarse"
    );
}
