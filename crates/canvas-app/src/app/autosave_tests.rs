//! Tabla del autoguardado: cuándo guarda solo (y, sobre todo, cuándo NO), y
//! cómo se comporta el temporizador ante fallos. Hermano de `autosave.rs`
//! (convención `*_tests.rs`).
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::SaveFlow;
use super::{autosave_path, retry_delay, Autosave, AutosaveStatus, QUIET, RETRY_BACKOFF};
use crate::editor;

/// Documento del caso normal.
fn doc() -> Option<&'static Path> {
    Some(Path::new("foto.png"))
}
fn other() -> Option<&'static Path> {
    Some(Path::new("otra.png"))
}

/// Estado sucio con destino PNG ya escrito alguna vez en esta sesión: el
/// caso «normal» en que el autoguardado SÍ debe disparar.
fn editable() -> (editor::EditorState, SaveFlow) {
    let mut state = editor::EditorState::new_blank_image(100.0, 100.0);
    state.doc.source_path = Some(PathBuf::from("foto.png"));
    state.born_blank = false;
    state.history.mark_unsaved();
    let save = SaveFlow {
        overwrite_confirmed: true,
        discard_raster_confirmed: true,
        ..SaveFlow::default()
    };
    (state, save)
}

#[test]
fn a_clean_document_never_autosaves() {
    let (mut state, save) = editable();
    state.history.mark_saved();
    assert_eq!(autosave_path(&state, &save, false, false), None);
}

#[test]
fn a_dirty_saved_png_autosaves_in_place() {
    let (state, save) = editable();
    assert_eq!(
        autosave_path(&state, &save, false, false),
        Some(PathBuf::from("foto.png"))
    );
}

/// Las dos decisiones del usuario que NO se pueden resolver por detrás: el
/// banner de cambio externo (Reload / Keep mine) y el diálogo de cambios sin
/// guardar (Save / Discard / Cancel). Escribir antes de que conteste
/// convertiría «Discard» en una mentira.
#[test]
fn a_pending_user_decision_disables_autosave() {
    let (mut state, save) = editable();
    state.external_change = true;
    assert_eq!(autosave_path(&state, &save, false, false), None);
    state.external_change = false;
    assert_eq!(autosave_path(&state, &save, false, true), None);
    // Sin decisión pendiente, escribe.
    assert!(autosave_path(&state, &save, false, false).is_some());
}

#[test]
fn the_destructive_first_save_gates_are_respected() {
    // Sobrescritura del original sin confirmar: el Ctrl+S manual pregunta, así
    // que el autoguardado no puede escribir por detrás.
    let (state, mut save) = editable();
    save.overwrite_confirmed = false;
    assert_eq!(autosave_path(&state, &save, false, false), None);

    // Documento sin capas de imagen (el usuario borró la última foto).
    let (mut state, mut save) = editable();
    save.discard_raster_confirmed = false;
    assert_eq!(autosave_path(&state, &save, false, false), None);
    // …salvo en un lienzo que nació en blanco: no hay foto que destruir.
    state.born_blank = true;
    assert!(autosave_path(&state, &save, false, false).is_some());
}

#[test]
fn open_dialogs_and_in_flight_work_disable_autosave() {
    type Break = fn(&mut editor::EditorState, &mut SaveFlow);
    let cases: [(&str, Break); 6] = [
        ("ruta", |_, save| save.save_path_dialog_open = true),
        ("save as", |_, save| {
            save.pending_save_as = Some(PathBuf::from("otro.png"))
        }),
        ("save all", |_, save| save.save_all_queue = vec![1]),
        ("aviso", |_, save| {
            save.overwrite_prompt = Some(PathBuf::from("a.png"))
        }),
        ("guardando", |state, _| state.saving = true),
        ("exportando", |state, _| state.exporting = true),
    ];
    for (label, break_it) in cases {
        let (mut state, mut save) = editable();
        break_it(&mut state, &mut save);
        assert_eq!(autosave_path(&state, &save, false, false), None, "{label}");
    }
}

/// Las banderas de UI a medias (`blur_edit`, `panel_edit`, texto inline…)
/// apagan el autoguardado vía `is_idle_except_save`, cubierta en
/// `editor::state::idle_tests` (ahí sí se ven esos campos privados).
#[test]
fn expensive_or_unwritable_targets_disable_autosave() {
    // SVG/GIF y vídeo no se sobrescriben con `Ctrl+S`: tampoco solos.
    for path in ["dibujo.svg", "anim.gif", "clip.mp4"] {
        let (mut state, save) = editable();
        state.doc.source_path = Some(PathBuf::from(path));
        assert_eq!(autosave_path(&state, &save, false, false), None, "{path}");
    }

    // Sin destino en disco no hay nada que escribir en el sitio.
    let (mut state, save) = editable();
    state.doc.source_path = None;
    assert_eq!(autosave_path(&state, &save, false, false), None);

    // Un diseño se autoguarda en su `.canvas`, no en un PNG prestado.
    let (mut state, save) = editable();
    state.is_design = true;
    assert_eq!(autosave_path(&state, &save, false, false), None);
    state.doc.source_path = Some(PathBuf::from("diseno.canvas"));
    assert!(autosave_path(&state, &save, false, false).is_some());
}

#[test]
fn the_timer_waits_for_quiet_and_fires() {
    let mut auto = Autosave::default();
    let t0 = Instant::now();
    let revision = 7;
    // Primer aviso: arma la espera, no guarda.
    assert!(!auto.tick(t0, doc(), revision, true, true));
    assert!(!auto.tick(
        t0 + QUIET - Duration::from_millis(1),
        doc(),
        revision,
        true,
        true
    ));
    // Cumplido el silencio, guarda.
    assert!(auto.tick(t0 + QUIET, doc(), revision, true, true));
}

#[test]
fn a_new_revision_rearms_the_wait_and_clears_the_backoff() {
    let mut auto = Autosave::default();
    let t0 = Instant::now();
    assert!(!auto.tick(t0, doc(), 3, true, true));
    assert!(auto.tick(t0 + QUIET, doc(), 3, true, true));
    auto.on_failed(t0 + QUIET);
    // Sin edición nueva, el reintento espera su backoff.
    assert!(!auto.tick(t0 + QUIET + Duration::from_secs(1), doc(), 3, true, true));
    // Edición nueva: se rearma desde cero y no hereda el backoff.
    let edit = t0 + QUIET + Duration::from_secs(2);
    assert!(!auto.tick(edit, doc(), 4, true, true));
    assert!(auto.tick(edit + QUIET, doc(), 4, true, true));
}

/// «Siempre» no significa «una vez y a esperar otra edición»: un fallo
/// transitorio se reintenta solo (5 s, 15 s, 60 s) y solo se rinde después,
/// hasta la próxima edición.
#[test]
fn a_failed_attempt_retries_with_backoff_and_then_gives_up() {
    let mut auto = Autosave::default();
    let t0 = Instant::now();
    assert!(!auto.tick(t0, doc(), 9, true, true));
    assert!(auto.tick(t0 + QUIET, doc(), 9, true, true));
    auto.on_failed(t0 + QUIET);
    let retry1 = t0 + QUIET + RETRY_BACKOFF[0];
    assert!(!auto.tick(retry1 - Duration::from_millis(1), doc(), 9, true, true));
    assert!(auto.tick(retry1, doc(), 9, true, true));
    auto.on_failed(retry1);
    let retry2 = retry1 + RETRY_BACKOFF[1];
    assert!(!auto.tick(retry2 - Duration::from_millis(1), doc(), 9, true, true));
    assert!(auto.tick(retry2, doc(), 9, true, true));
    auto.on_failed(retry2);
    // Agotados los intentos: ni el tiempo ni un frame lejano lo relanzan.
    assert!(!auto.tick(retry2 + Duration::from_secs(3600), doc(), 9, true, true));
    // Pero una edición nueva sí.
    let edit = retry2 + Duration::from_secs(3700);
    assert!(!auto.tick(edit, doc(), 10, true, true));
    assert!(auto.tick(edit + QUIET, doc(), 10, true, true));
}

#[test]
fn a_success_clears_the_backoff() {
    let mut auto = Autosave::default();
    let t0 = Instant::now();
    assert!(!auto.tick(t0, doc(), 2, true, true));
    assert!(auto.tick(t0 + QUIET, doc(), 2, true, true));
    auto.on_failed(t0 + QUIET);
    assert!(!auto.tick(t0 + QUIET + Duration::from_secs(1), doc(), 2, true, true));
    auto.on_saved();
    assert!(auto.tick(t0 + QUIET + Duration::from_secs(1), doc(), 2, true, true));
}

/// Abrir otro lienzo no hereda la espera ni el backoff del anterior: las
/// revisiones de historiales distintos pueden coincidir.
#[test]
fn changing_documents_resets_the_wait_and_the_attempts() {
    let mut auto = Autosave::default();
    let t0 = Instant::now();
    assert!(!auto.tick(t0, doc(), 1, true, true));
    assert!(auto.tick(t0 + QUIET, doc(), 1, true, true));
    auto.on_failed(t0 + QUIET);
    assert_eq!(auto.status(), AutosaveStatus::Retrying);
    // Misma revisión, otro documento: ciclo nuevo y sin backoff heredado.
    assert!(!auto.tick(t0 + QUIET, other(), 1, true, true));
    assert_eq!(auto.status(), AutosaveStatus::Idle);
    assert!(auto.tick(t0 + QUIET + QUIET, other(), 1, true, true));
}

/// «Discard» llega antes del frame del editor y la navegación se aplica
/// después: sin desarmar, el autoguardado escribiría justo lo descartado.
#[test]
fn discard_disarms_the_frame_between_the_dialog_and_navigation() {
    let mut auto = Autosave::default();
    let t0 = Instant::now();
    assert!(!auto.tick(t0, doc(), 1, true, true));
    auto.on_discarded();
    assert!(!auto.tick(t0 + QUIET, doc(), 1, true, true));
    // La siguiente edición arma el ciclo con normalidad.
    let edit = t0 + QUIET + Duration::from_secs(1);
    assert!(!auto.tick(edit, doc(), 2, true, true));
    assert!(auto.tick(edit + QUIET, doc(), 2, true, true));
}

#[test]
fn retry_delay_table() {
    assert_eq!(retry_delay(0), None);
    assert_eq!(retry_delay(1), Some(RETRY_BACKOFF[0]));
    assert_eq!(retry_delay(2), Some(RETRY_BACKOFF[1]));
    assert_eq!(retry_delay(3), Some(RETRY_BACKOFF[2]));
    assert_eq!(retry_delay(4), None);
}

/// El latido que se pide a egui: sin él, la ventana en reposo no pinta frames
/// y el silencio de 1,5 s no se cumple nunca.
#[test]
fn next_wake_points_at_the_quiet_then_at_the_retry() {
    let mut auto = Autosave::default();
    let t0 = Instant::now();
    assert_eq!(auto.next_wake(t0), None);
    assert!(!auto.tick(t0, doc(), 1, true, true));
    assert_eq!(auto.next_wake(t0), Some(QUIET));
    assert_eq!(auto.next_wake(t0 + QUIET / 2), Some(QUIET - QUIET / 2));
    // Tras fallar, el próximo aviso es el reintento (el silencio ya cumplido).
    assert!(auto.tick(t0 + QUIET, doc(), 1, true, true));
    auto.on_failed(t0 + QUIET);
    assert_eq!(auto.next_wake(t0 + QUIET), Some(RETRY_BACKOFF[0]));
    // Limpio: nada que despertar.
    assert!(!auto.tick(t0 + QUIET, doc(), 1, false, true));
    assert_eq!(auto.next_wake(t0 + QUIET), None);
}

#[test]
fn status_reports_only_a_pending_retry() {
    let mut auto = Autosave::default();
    let now = Instant::now();
    assert_eq!(auto.status(), AutosaveStatus::Idle);
    auto.on_failed(now);
    assert_eq!(auto.status(), AutosaveStatus::Retrying);
    auto.on_saved();
    assert_eq!(auto.status(), AutosaveStatus::Idle);
}

#[test]
fn disallowed_never_fires_and_a_clean_document_resets() {
    let mut auto = Autosave::default();
    let t0 = Instant::now();
    assert!(!auto.tick(t0, doc(), 3, true, false));
    assert!(!auto.tick(t0 + QUIET * 2, doc(), 3, true, false));
    // En cuanto se permite, el silencio ya estaba cumplido: guarda.
    assert!(auto.tick(t0 + QUIET * 2, doc(), 3, true, true));
    // Limpio: se desarma (el próximo cambio arranca de cero).
    assert!(!auto.tick(t0 + QUIET * 3, doc(), 3, false, true));
    assert_eq!(auto.next_wake(t0 + QUIET * 3), None);
    assert!(!auto.tick(t0 + QUIET * 4, doc(), 4, true, true));
    assert!(auto.tick(t0 + QUIET * 5, doc(), 4, true, true));
}
