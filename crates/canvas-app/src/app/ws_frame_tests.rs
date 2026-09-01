//! Tests de `capture_geometry` (`ws_frame.rs`), el contrato de la
//! geometría persistida de una ventana: posición del rect EXTERIOR
//! (fallback al interior) y tamaño SIEMPRE del rect INTERIOR — lo que
//! `with_inner_size`/`StoredWorkspace::size` entienden. Mezclar el rect
//! exterior como tamaño (como hacía `outer_rect.or(inner_rect)`) era la
//! mitad del bucle de redimensionado en Windows (ver
//! `spawn_child_viewports` y `tasks/plan.md`).

use eframe::egui;

use super::capture_geometry;

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
