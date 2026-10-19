//! Sonda de rendimiento del lienzo, APAGADA por defecto: se enciende con
//! `CANVAS_DEBUG_PERF=1` y deja una línea cada `REPORT_EVERY` frames con el
//! desglose del pintado. Nace de la regresión del «lienzo extremadamente
//! lento»: el número que delata ese bug es `display_rebuilds`, que en un
//! frame en reposo debe quedarse clavado (la copia reducida de una capa
//! grande solo se materializa cuando sus píxeles cambian, nunca por frame).
//!
//! Mismo contrato que la sonda del atlas (`CANVAS_DEBUG_ATLAS`): sin la
//! variable no hay logs ni trabajo extra. El `OnceLock` evita consultar el
//! entorno en cada frame; los tiempos se miden siempre porque son dos
//! lecturas de reloj monótono (nanosegundos) y simplifican el llamador.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

/// Frames entre dos líneas de la sonda (~1 s a 60 fps).
pub(crate) const REPORT_EVERY: u64 = 60;

/// Copias `display` vistas en el último reporte (para publicar el delta).
static LAST_REBUILDS: AtomicU64 = AtomicU64::new(0);
/// Número de frame desde que arrancó el proceso.
static FRAME: AtomicU64 = AtomicU64::new(0);
/// ¿El frame anterior materializó alguna copia reducida? Sirve para publicar
/// el PRIMER frame de cada ráfaga: la app en reposo pinta pocos frames, así
/// que esperar 60 escondería un frame de 100 ms de copia. En régimen normal
/// (cero copias por frame) no se dispara nunca y no hay ruido.
static LAST_HAD_REBUILDS: AtomicBool = AtomicBool::new(false);

pub(crate) fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("CANVAS_DEBUG_PERF").is_some())
}

/// ¿Toca publicar en este frame? Una de cada `REPORT_EVERY`. Pura.
pub(crate) fn should_report(frame: u64) -> bool {
    frame.is_multiple_of(REPORT_EVERY)
}

/// ¿Se publica este frame? El cadence periódico, o el PRIMER frame de una
/// ráfaga de copias reducidas (el síntoma del bug, que hay que ver al
/// instante aunque la app pinte pocos frames en reposo). Pura.
pub(crate) fn should_log(frame: u64, burst: bool) -> bool {
    burst || should_report(frame)
}

/// Cuántas copias `display` se materializaron desde el reporte anterior.
/// Pura: saturante, porque el contador del renderer es monótono pero el
/// último valor publicado puede venir de un frame posterior del otro
/// workspace que se pintó entre medias.
fn rebuilds_since(previous: u64, current: u64) -> u64 {
    current.saturating_sub(previous)
}

/// Cronómetro del pintado: `split()` devuelve ms desde que se creó.
pub(crate) struct Timer {
    start: Instant,
}

impl Timer {
    pub(crate) fn start() -> Self {
        Self {
            start: Instant::now(),
        }
    }

    /// Milisegundos transcurridos.
    pub(crate) fn split(&self) -> f64 {
        self.start.elapsed().as_secs_f64() * 1000.0
    }
}

/// Lo medido en un frame de `paint`.
pub(crate) struct FrameStats {
    pub total_ms: f64,
    pub encode_ms: f64,
    pub render_ms: f64,
    pub visible_slots: usize,
    pub layers_synced: usize,
    /// Total acumulado del renderer (`CanvasRenderer::display_rebuilds`).
    pub display_rebuilds: u64,
}

/// Publica el frame si la sonda está encendida y toca. Nunca cambia
/// conducta: sin `CANVAS_DEBUG_PERF` vuelve en la primera línea.
pub(crate) fn report(stats: FrameStats) {
    if !enabled() {
        return;
    }
    let frame = FRAME.fetch_add(1, Ordering::Relaxed) + 1;
    let rebuilds = rebuilds_since(
        LAST_REBUILDS.swap(stats.display_rebuilds, Ordering::Relaxed),
        stats.display_rebuilds,
    );
    let burst = rebuilds > 0 && !LAST_HAD_REBUILDS.swap(rebuilds > 0, Ordering::Relaxed);
    if !should_log(frame, burst) {
        return;
    }
    tracing::info!(
        frame,
        total_ms = stats.total_ms,
        encode_ms = stats.encode_ms,
        render_ms = stats.render_ms,
        visible_slots = stats.visible_slots,
        layers_synced = stats.layers_synced,
        display_rebuilds = rebuilds,
        display_rebuilds_total = stats.display_rebuilds,
        "perf lienzo: frame pintado"
    );
}

#[cfg(test)]
mod tests {
    use super::{rebuilds_since, should_log, should_report, REPORT_EVERY};

    #[test]
    fn reports_one_frame_out_of_every_batch() {
        assert!(!should_report(1));
        assert!(!should_report(REPORT_EVERY - 1));
        assert!(should_report(REPORT_EVERY));
        assert!(!should_report(REPORT_EVERY + 1));
        assert!(should_report(REPORT_EVERY * 2));
    }

    #[test]
    fn a_burst_logs_immediately_without_waiting_for_the_batch() {
        // El bug se manifiesta en frames sueltos (la app en reposo no llega a
        // 60): la primera copia de la ráfaga se publica ya.
        assert!(should_log(1, true));
        assert!(!should_log(1, false));
        assert!(!should_log(REPORT_EVERY - 1, false));
        assert!(should_log(REPORT_EVERY, false));
    }

    #[test]
    fn rebuild_delta_table() {
        // (antes, ahora, esperado): el contador es monótono, pero un frame de
        // otra ventana puede haberlo avanzado — nunca un valor negativo.
        let cases = [(0u64, 0u64, 0u64), (0, 3, 3), (3, 3, 0), (5, 2, 0)];
        for (previous, current, expected) in cases {
            assert_eq!(
                rebuilds_since(previous, current),
                expected,
                "antes={previous} ahora={current}"
            );
        }
    }
}
