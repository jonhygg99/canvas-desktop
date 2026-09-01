//! Tests de `seed_builder_geometry` (`workspace_lifecycle.rs`): el
//! contrato de la siembra de geometría en el builder de nacimiento de las
//! ventanas hijas — la primera vez puede llevar la geometría heredada;
//! desde entonces NUNCA vuelve a ofrecer tamaño/posición. Fija la
//! regresión del redimensionado automático en Windows (eframe parchea el
//! builder cada frame y emite `InnerSize`/`OuterPosition` ante cualquier
//! cambio — ver `tasks/plan.md` y el doc de `seed_builder_geometry`).

use eframe::egui;

use super::seed_builder_geometry;

fn geometry() -> Option<(egui::Pos2, egui::Vec2)> {
    Some((egui::pos2(100.0, 120.0), egui::vec2(1280.0, 800.0)))
}

/// Primera vez con geometría heredada (Ctrl+N/Ctrl+T): el builder naciente
/// la ofrece y el flag queda sembrado.
#[test]
fn first_spawn_inherits_the_geometry() {
    let g = geometry();
    assert_eq!(seed_builder_geometry(false, g), (g, true));
}

/// Primera vez sin geometría (p. ej. `CANVAS_DEBUG_WINDOWS`): nada en el
/// builder, pero el flag se siembra igual — una geometría capturada
/// después es captura, no intención, y jamás debe llegar al builder.
#[test]
fn first_spawn_without_geometry_seeds_anyway() {
    assert_eq!(seed_builder_geometry(false, None), (None, true));
}

/// Tras el nacimiento el builder no lleva tamaño/posición aunque la
/// geometría capturada haya cambiado entre frames: la regresión del
/// redimensionado (el builder quedaba «dirty» para `patch` y la ventana
/// crecía la decoración por frame).
#[test]
fn after_birth_changed_geometry_never_reaches_the_builder() {
    let (applied, seeded) = seed_builder_geometry(false, geometry());
    assert_eq!(applied, geometry());
    assert!(seeded);
    // La captura en vivo cambia el valor (otra ventana, otro frame)…
    let live = Some((egui::pos2(-5.0, 7.0), egui::vec2(640.0, 480.0)));
    // …pero el contrato lo corta: nada para el builder, flag intacto.
    assert_eq!(seed_builder_geometry(seeded, live), (None, true));
    // Y una ventana ya sembrada sin geometría sigue siembrada y callada.
    assert_eq!(seed_builder_geometry(true, None), (None, true));
}

/// El flag nunca retrocede: una vez sembrado, todos los frames posteriores
/// devuelven `true` pase lo que pase con la geometría.
#[test]
fn seeded_flag_is_monotonic() {
    for geometry in [geometry(), None] {
        assert_eq!(seed_builder_geometry(true, geometry), (None, true));
    }
}
