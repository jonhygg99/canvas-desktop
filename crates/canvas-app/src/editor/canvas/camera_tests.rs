//! Tests de `apply_camera`. Corren un frame de egui de verdad, sin backend:
//! `egui::Context::run_ui` toma un `RawInput` sintético y ejecuta el cuerpo en
//! CPU, así que se puede ejercitar el atajo, la rueda y el paneo tal cual los
//! ve la app.

use eframe::egui;

use std::path::{Path, PathBuf};

use crate::deck::{Deck, DeckAxis, DeckRect, DeckSeed, SeedItem};
use crate::editor::EditorState;
use crate::gallery::ItemKind;
use crate::settings::GallerySort;

use super::super::viewport::AutoFit;
use super::camera::{apply_camera, Camera};

const AVAIL: egui::Vec2 = egui::vec2(1000.0, 800.0);

fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

fn wheel(delta: egui::Vec2, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta,
        phase: egui::TouchPhase::Move,
        modifiers,
    }
}

fn raw(events: Vec<egui::Event>, modifiers: egui::Modifiers) -> egui::RawInput {
    // Ventana holgada: el área de dibujo la fija `allocate_exact_size`, no
    // esto, y así el mismo `RawInput` sirve para cualquier tamaño de área.
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(4000.0, 4000.0),
        )),
        modifiers,
        events,
        ..Default::default()
    }
}

/// Corre `apply_camera` en un frame headless. `warmup` ejecuta antes un frame
/// solo con el puntero: egui decide el hover con lo que registró el frame
/// anterior, así que los tests de rueda lo necesitan.
fn run(state: &mut EditorState, deck: &Deck, input: egui::RawInput, warmup: bool) -> Camera {
    run_at(state, deck, AVAIL, input, warmup)
}

/// Igual, pero con un área de dibujo de otro tamaño: es lo que de verdad
/// significa «la ventana cambió de tamaño».
fn run_at(
    state: &mut EditorState,
    deck: &Deck,
    avail: egui::Vec2,
    input: egui::RawInput,
    warmup: bool,
) -> Camera {
    let ctx = egui::Context::default();
    ctx.set_fonts(egui::FontDefinitions::empty());

    let go = |state: &mut EditorState, input: egui::RawInput| {
        let mut out = None;
        let _ = ctx.run_ui(input, |ui| {
            let (rect, response) = ui.allocate_exact_size(avail, egui::Sense::click_and_drag());
            out = Some(apply_camera(state, deck, ui, rect, &response));
        });
        out.expect("run_ui ejecuta el cuerpo exactamente una vez")
    };

    if warmup {
        let pointer = input
            .events
            .iter()
            .find_map(|e| match e {
                egui::Event::PointerMoved(p) => Some(*p),
                _ => None,
            })
            .unwrap_or(egui::pos2(500.0, 400.0));
        go(
            state,
            raw(
                vec![egui::Event::PointerMoved(pointer)],
                egui::Modifiers::NONE,
            ),
        );
    }
    go(state, input)
}

/// Baraja de dos lienzos apilados en vertical, para que «ajustar el activo» y
/// «ajustar toda la baraja» den resultados DISTINTOS.
fn deck_of_two() -> Deck {
    let seed = DeckSeed {
        folder: PathBuf::from("carpeta"),
        sort: GallerySort::Name,
        items: ["a.png", "b.png"]
            .iter()
            .map(|p| SeedItem {
                path: PathBuf::from(p),
                name: (*p).to_owned(),
                kind: ItemKind::Image,
                mtime: None,
                thumb: None,
                thumb_failed: false,
            })
            .collect(),
    };
    let mut deck = Deck::from_seed(seed, Path::new("a.png"));
    for slot in &mut deck.slots {
        slot.page = Some((800.0, 600.0));
    }
    deck.relayout();
    deck
}

fn ready_state() -> EditorState {
    let mut state = EditorState::new_blank(800.0, 600.0);
    // Sella el tamaño y desarma el ajuste del primer frame: cada test decide
    // qué quiere provocar.
    state.viewport.note_size(AVAIL);
    state.viewport.needs_fit = false;
    state.viewport.auto_fit = AutoFit::Off;
    state
}

fn pointer_at_center() -> egui::Event {
    egui::Event::PointerMoved(egui::pos2(500.0, 400.0))
}

#[test]
fn ctrl_zero_fits_the_active_canvas() {
    let mut state = ready_state();
    run(
        &mut state,
        &deck_of_two(),
        raw(
            vec![key(egui::Key::Num0, egui::Modifiers::COMMAND)],
            egui::Modifiers::COMMAND,
        ),
        false,
    );
    assert!(state.viewport.auto_fit == AutoFit::Active);
}

#[test]
fn ctrl_alt_zero_fits_the_whole_deck() {
    let mut state = ready_state();
    let mods = egui::Modifiers::COMMAND | egui::Modifiers::ALT;
    run(
        &mut state,
        &deck_of_two(),
        raw(vec![key(egui::Key::Num0, mods)], mods),
        false,
    );
    assert!(state.viewport.auto_fit == AutoFit::All);
}

#[test]
fn ctrl_alt_zero_is_not_swallowed_by_the_plain_ctrl_zero_branch() {
    // El orden importa: el atajo MÁS específico se consume primero. Si se
    // invirtiera, `Ctrl+Alt+0` acabaría ajustando solo el lienzo activo.
    let mut all = ready_state();
    let mut active = ready_state();
    let deck = deck_of_two();

    let both = egui::Modifiers::COMMAND | egui::Modifiers::ALT;
    run(
        &mut all,
        &deck,
        raw(vec![key(egui::Key::Num0, both)], both),
        false,
    );
    run(
        &mut active,
        &deck,
        raw(
            vec![key(egui::Key::Num0, egui::Modifiers::COMMAND)],
            egui::Modifiers::COMMAND,
        ),
        false,
    );

    assert!(
        all.viewport.zoom < active.viewport.zoom,
        "ajustar la baraja entera (dos lienzos) tiene que alejar más que ajustar uno: \
         all={} active={}",
        all.viewport.zoom,
        active.viewport.zoom
    );
}

#[test]
fn a_pending_center_request_is_consumed_and_arms_the_active_refit() {
    // Sin armar `Active`, el primer redimensionado posterior volvería a
    // encajar TODA la baraja y desharía el centrado.
    let mut state = ready_state();
    state.viewport.auto_fit = AutoFit::All;
    state.viewport.request_center(DeckRect {
        x: 0.0,
        y: 700.0,
        w: 800.0,
        h: 600.0,
    });
    let before = state.viewport.zoom;

    run(
        &mut state,
        &deck_of_two(),
        raw(Vec::new(), egui::Modifiers::NONE),
        false,
    );

    assert!(
        state.viewport.center_request.is_none(),
        "la petición no se consumió"
    );
    assert!(state.viewport.auto_fit == AutoFit::Active);
    assert_eq!(state.viewport.zoom, before, "centrar no debe tocar el zoom");
}

#[test]
fn a_resize_repeats_the_last_automatic_fit() {
    let mut state = ready_state();
    let deck = deck_of_two();
    let mods = egui::Modifiers::COMMAND | egui::Modifiers::ALT;
    run(
        &mut state,
        &deck,
        raw(vec![key(egui::Key::Num0, mods)], mods),
        false,
    );
    let fitted = state.viewport.zoom;

    // La ventana se hace más pequeña: `note_size` lo detecta y repite el
    // ajuste que estuviera armado.
    run_at(
        &mut state,
        &deck,
        egui::vec2(400.0, 300.0),
        raw(Vec::new(), egui::Modifiers::NONE),
        false,
    );

    assert_ne!(
        state.viewport.zoom, fitted,
        "no repitió el ajuste al redimensionar"
    );
    assert!(
        state.viewport.auto_fit == AutoFit::All,
        "y debe seguir armado"
    );
}

#[test]
fn a_resize_does_nothing_once_the_user_has_moved_the_view_by_hand() {
    let mut state = ready_state();
    state.viewport.zoom_at(egui::vec2(100.0, 100.0), 1.5); // gesto manual
    let manual = state.viewport.zoom;

    run_at(
        &mut state,
        &deck_of_two(),
        egui::vec2(400.0, 300.0),
        raw(Vec::new(), egui::Modifiers::NONE),
        false,
    );

    assert_eq!(
        state.viewport.zoom, manual,
        "le deshizo el zoom que hizo a mano"
    );
}

#[test]
fn a_zoom_requested_from_the_menu_is_applied_and_consumed() {
    let mut state = ready_state();
    let before = state.viewport.zoom;
    state.pending_zoom_factor = Some(2.0);

    run(
        &mut state,
        &deck_of_two(),
        raw(Vec::new(), egui::Modifiers::NONE),
        false,
    );

    assert!(state.viewport.zoom > before);
    assert!(
        state.pending_zoom_factor.is_none(),
        "quedó armado para el próximo frame"
    );
}

#[test]
fn the_wheel_pans_along_the_primary_axis_of_the_deck() {
    let mut state = ready_state();
    let before = state.viewport.pan;

    run(
        &mut state,
        &deck_of_two(), // eje vertical
        raw(
            vec![
                pointer_at_center(),
                wheel(egui::vec2(0.0, 40.0), egui::Modifiers::NONE),
            ],
            egui::Modifiers::NONE,
        ),
        true,
    );

    assert!(
        (state.viewport.pan.y - before.y).abs() > 1.0,
        "la rueda no desplazó en el eje primario: {:?} -> {:?}",
        before,
        state.viewport.pan
    );
    assert!(
        (state.viewport.pan.x - before.x).abs() < 1e-3,
        "no debía moverse en x"
    );
}

#[test]
fn the_wheel_scrolls_the_same_way_as_the_rest_of_the_app() {
    // `+=`, no `-=`: con el signo invertido la rueda del lienzo iba al revés
    // que cualquier `ScrollArea` de la propia app.
    let mut state = ready_state();
    let before = state.viewport.pan.y;

    run(
        &mut state,
        &deck_of_two(),
        raw(
            vec![
                pointer_at_center(),
                wheel(egui::vec2(0.0, 40.0), egui::Modifiers::NONE),
            ],
            egui::Modifiers::NONE,
        ),
        true,
    );

    assert!(
        state.viewport.pan.y > before,
        "una rueda positiva debe aumentar el pan, no restarlo"
    );
}

#[test]
fn shift_and_the_wheel_pan_along_the_cross_axis() {
    let mut state = ready_state();
    let before = state.viewport.pan;

    run(
        &mut state,
        &deck_of_two(), // eje vertical -> Shift pide el horizontal
        raw(
            vec![
                pointer_at_center(),
                wheel(egui::vec2(0.0, 40.0), egui::Modifiers::SHIFT),
            ],
            egui::Modifiers::SHIFT,
        ),
        true,
    );

    assert!(
        (state.viewport.pan.x - before.x).abs() > 1.0,
        "Shift no cambió al eje transversal: {:?} -> {:?}",
        before,
        state.viewport.pan
    );
}

#[test]
fn the_wheel_follows_a_horizontal_deck_without_shift() {
    let mut state = ready_state();
    let mut deck = deck_of_two();
    deck.axis = DeckAxis::Horizontal;
    let before = state.viewport.pan;

    run(
        &mut state,
        &deck,
        raw(
            vec![
                pointer_at_center(),
                wheel(egui::vec2(0.0, 40.0), egui::Modifiers::NONE),
            ],
            egui::Modifiers::NONE,
        ),
        true,
    );

    assert!(
        (state.viewport.pan.x - before.x).abs() > 1.0,
        "con la baraja en horizontal la rueda debe desplazar en x: {:?} -> {:?}",
        before,
        state.viewport.pan
    );
}

#[test]
fn ctrl_and_the_wheel_zoom_instead_of_panning() {
    let mut state = ready_state();
    let before = state.viewport.zoom;

    run(
        &mut state,
        &deck_of_two(),
        raw(
            vec![
                pointer_at_center(),
                wheel(egui::vec2(0.0, 40.0), egui::Modifiers::COMMAND),
            ],
            egui::Modifiers::COMMAND,
        ),
        true,
    );

    assert!(state.viewport.zoom > before, "Ctrl+rueda no hizo zoom");
}

#[test]
fn the_wheel_is_ignored_when_the_pointer_is_not_over_the_canvas() {
    let mut state = ready_state();
    let before = state.viewport.pan;

    run(
        &mut state,
        &deck_of_two(),
        raw(
            vec![wheel(egui::vec2(0.0, 40.0), egui::Modifiers::NONE)],
            egui::Modifiers::NONE,
        ),
        false,
    );

    assert_eq!(state.viewport.pan, before, "desplazó sin el puntero encima");
}

#[test]
fn holding_space_is_reported_to_the_rest_of_the_frame() {
    // El resto del frame lo usa para no arrastrar capas sin querer.
    let mut state = ready_state();
    let camera = run(
        &mut state,
        &deck_of_two(),
        raw(
            vec![key(egui::Key::Space, egui::Modifiers::NONE)],
            egui::Modifiers::NONE,
        ),
        false,
    );
    assert!(camera.space_down);
    assert!(!camera.panning, "sin arrastrar, espacio solo no es paneo");
}
