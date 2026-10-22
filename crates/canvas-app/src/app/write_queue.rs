//! La cola ÚNICA de escritura del editor: cualquier lienzo sucio se guarda
//! solo, sea el que está en pantalla o uno de fondo, de uno en uno.
//!
//! Antes había dos caminos que no se hablaban: el autoguardado
//! (`app/autosave.rs`) solo conocía el documento en pantalla —al cambiar de
//! lienzo, lo editado se quedaba sin escribir hasta el «Save all»— y la cola
//! de clips (`app/clip_save.rs`) solo sabía de clips recién añadidos. Aquí la
//! misma cola encola cualquier ranura sucia y la escribe por el camino que le
//! toque:
//!
//! - clip (documento cuyo origen es un vídeo): render de la composición a mp4;
//! - diseño (`.canvas`): escritura del sidecar;
//! - raster: horneado de la página + escritura + sidecar.
//!
//! Un solo worker a la vez en toda la ventana: ffmpeg y la GPU ya cuestan lo
//! suyo y dos escrituras simultáneas se pelearían por el atlas y por los
//! nombres reservados. Sin diálogos: lo que necesita una decisión del usuario
//! (`write_target::background_target`) se queda para «Save all» o para el
//! `Ctrl+S` de su propio lienzo, que sí pueden preguntar.
//!
//! Un fallo se reintenta igual que el del autoguardado del activo
//! (`app/autosave.rs`): un fallo TRANSITORIO (RAM crítica, GPU ocupada, archivo
//! bloqueado, horneado inestable) espera la misma tabla de backoff y vuelve a
//! intentarlo solo — antes se aparcaba para siempre y un lienzo editado y
//! abandonado podía quedarse horas sin escribir—; uno PERMANENTE (destino sin
//! permisos, formato sin codificador) se aparca hasta una edición nueva, porque
//! reintentarlo solo repite un horneado completo. `SaveFlow::blocked_writes`
//! guarda ranura, revisión e intentos: mientras una ranura espera su reintento
//! no se encola, así que un destino rompible no gasta un horneado por frame.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use eframe::egui;
use eframe::egui_wgpu::RenderState;

use crate::deck;
use crate::editor;

use super::autosave::{is_permanent_save_error, retry_delay};
use super::clip_save::{clip_save_target, is_clip_document};
use super::frame::EditorFrame;
use super::write_target::{background_target, BackgroundRequest, BackgroundWrite};

/// Reintento de una escritura de FONDO que falló: qué revisión falló, cuántos
/// intentos lleva y cuándo toca el siguiente. Uno por ranura en
/// `SaveFlow::blocked_writes`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WriteRetry {
    /// Revisión del historial en la que falló: una edición nueva (otra
    /// revisión) rearma el ciclo, igual que en el autoguardado del activo.
    pub(crate) revision: u64,
    /// Fallos acumulados de esa revisión, para elegir la espera.
    pub(crate) attempts: usize,
    /// Antes de este instante no se reintenta. `None` = aparcado hasta una
    /// edición nueva (fallo permanente o tabla de esperas agotada).
    pub(crate) retry_at: Option<Instant>,
}

impl WriteRetry {
    /// Aparcada: no vuelve a intentarse sola. Es la ranura que la barra de
    /// estado cuenta como «sin guardar» (ver `toolbar::save_status`).
    pub(crate) fn is_parked(&self) -> bool {
        self.retry_at.is_none()
    }
}

/// Anota un fallo de escritura de fondo y programa el siguiente intento. Un
/// fallo transitorio espera `retry_delay` (5 s, 15 s, 60 s) y al agotar la
/// tabla se aparca; uno permanente se aparca a la primera. Pura salvo el reloj
/// que recibe (tabla de tests).
pub(super) fn record_failure(
    blocked: &mut HashMap<u64, WriteRetry>,
    slot: u64,
    revision: u64,
    permanent: bool,
    now: Instant,
) {
    let attempts = match blocked.get(&slot) {
        Some(entry) if entry.revision == revision => entry.attempts + 1,
        _ => 1,
    };
    let retry_at = if permanent {
        None
    } else {
        retry_delay(attempts).map(|delay| now + delay)
    };
    if retry_at.is_none() {
        tracing::info!(
            ranura = slot,
            intentos = attempts,
            permanente = permanent,
            "escritura de fondo: aparcada hasta la próxima edición"
        );
    }
    blocked.insert(
        slot,
        WriteRetry {
            revision,
            attempts,
            retry_at,
        },
    );
}

/// ¿Hay un fallo PERMANENTE detrás de este error? El mismo criterio que el
/// autoguardado (`autosave::is_permanent_save_error`): lo transitorio se
/// reintenta, lo que no admite escritura no.
pub(super) fn failure_is_permanent(error: Option<&canvas_io::IoError>) -> bool {
    error.is_some_and(is_permanent_save_error)
}

/// Cuánto falta para el próximo reintento programado: la cola pide un
/// repintado para ese instante (egui deja de pintar en reposo y, sin este
/// latido, un fallo transitorio esperaría a que el usuario tocara algo).
///
/// Solo mira los reintentos FUTUROS: uno ya vencido lo toma la cola en este
/// mismo frame, y pedir un repintado a 0/1 ms cada frame giraría el bucle sin
/// avanzar (el caso típico: un diálogo abierto impide arrancar la escritura).
pub(super) fn next_wake(save: &super::SaveFlow, now: Instant) -> Option<Duration> {
    save.blocked_writes
        .values()
        .filter_map(|entry| entry.retry_at)
        .filter(|at| *at > now)
        .map(|at| at.saturating_duration_since(now))
        .min()
}

/// Escrituras pendientes de una ventana. Los ids son los de RANURA (estables
/// durante la sesión, no el índice), así que reordenar la baraja no guarda el
/// lienzo equivocado. La generación de la baraja viaja con la cola: al abrir
/// otro proyecto los ids viejos no valen y la cola se vacía sola (una
/// respuesta en vuelo de la baraja anterior se descarta en su mensaje, que ya
/// compara generación).
#[derive(Debug, Default)]
pub(crate) struct WriteQueue {
    pending: VecDeque<u64>,
    /// Ranura cuya escritura está en vuelo (su `SlotDoc` lleva `saving`).
    in_flight: Option<u64>,
    generation: u64,
}

impl WriteQueue {
    /// Encola ranuras para escribir. Idempotente: un id que ya espera (o se
    /// está escribiendo) no se duplica.
    pub(super) fn enqueue(&mut self, generation: u64, ids: impl IntoIterator<Item = u64>) {
        self.sync(generation);
        for id in ids {
            if self.in_flight == Some(id) || self.pending.contains(&id) {
                continue;
            }
            self.pending.push_back(id);
        }
    }

    /// Olvida la cola si la baraja es otra (proyecto nuevo, vuelta a la
    /// galería, otra ventana con su propio `SaveFlow`).
    pub(super) fn sync(&mut self, generation: u64) {
        if self.generation != generation {
            self.generation = generation;
            self.pending.clear();
            self.in_flight = None;
        }
    }

    /// Siguiente ranura a escribir (pasa a «en vuelo»). `None` mientras haya
    /// una en vuelo: una escritura a la vez, siempre.
    pub(super) fn next(&mut self) -> Option<u64> {
        if self.in_flight.is_some() {
            return None;
        }
        let id = self.pending.pop_front()?;
        self.in_flight = Some(id);
        Some(id)
    }

    /// La escritura de `slot` terminó (bien o mal): la cola puede seguir.
    pub(super) fn finished(&mut self, slot: u64) {
        if self.in_flight == Some(slot) {
            self.in_flight = None;
        }
    }

    /// ¿Hay escrituras pendientes o en vuelo en ESTA baraja? Es la compuerta
    /// que impide salir del editor mientras un lienzo se está escribiendo.
    pub(super) fn busy(&self, generation: u64) -> bool {
        self.generation == generation && (self.in_flight.is_some() || !self.pending.is_empty())
    }

    /// Cuántos lienzos faltan por escribir (para la barra de estado).
    pub(super) fn remaining(&self, generation: u64) -> usize {
        if self.generation != generation {
            return 0;
        }
        self.pending.len() + usize::from(self.in_flight.is_some())
    }
}

/// Espacio mínimo entre dos escrituras de fondo consecutivas. El autoguardado
/// del lienzo activo ya espera `QUIET` tras la última edición; para el fondo
/// no hay gesto posible, pero sí ráfagas (tres clips añadidos a la vez, un
/// «Save all» recién vaciado): este margen evita encadenar horneados sin
/// dejar respirar un frame a la UI.
const MIN_GAP: Duration = Duration::from_millis(250);

/// Punto de entrada del frame. Se llama DESPUÉS del autoguardado (un `Ctrl+S`
/// manual manda) y DESPUÉS del lienzo y los paneles, para que una edición de
/// este frame ya cuente.
pub(super) fn tick(
    state: &mut editor::EditorState,
    ctx: &egui::Context,
    rs: &RenderState,
    f: &mut EditorFrame<'_>,
) {
    let generation = f.deck.generation();
    f.save.writes.sync(generation);
    enqueue_dirty(f);
    // Latido del reintento: si una ranura espera su segundo intento, hay que
    // volver a pintar en ese instante (en reposo egui no corre solo).
    if let Some(delay) = next_wake(f.save, Instant::now()) {
        ctx.request_repaint_after(delay);
    }
    if !ready_to_start(state, f) {
        return;
    }
    let Some(slot_id) = f.save.writes.next() else {
        return;
    };
    let Some(index) = f.deck.find_by_id(slot_id) else {
        tracing::info!("escritura: la ranura {slot_id} ya no existe; se descarta de la cola");
        f.save.writes.finished(slot_id);
        ctx.request_repaint();
        return;
    };
    if index == f.deck.active {
        super::write_jobs::start_active(state, ctx, f, slot_id, index);
        return;
    }
    super::write_jobs::start_background(ctx, rs, f, slot_id, index);
}

/// Encola todo lo que quedaría sin guardar: ranuras de fondo sucias (lo que
/// el temporizador del autoguardado no ve, porque solo conoce el documento en
/// pantalla). Los clips recién añadidos ya entran al crearse
/// (`app/video_canvases.rs`); esto cubre lo EDITADO y luego abandonado.
fn enqueue_dirty(f: &mut EditorFrame<'_>) {
    let generation = f.deck.generation();
    let now = Instant::now();
    let ids = dirty_background_slots(f.deck, f.save, f.settings.skip_overwrite_warning, now);
    f.save.writes.enqueue(generation, ids);
    // Un bloqueo caduca con la revisión (edición nueva) o con la ranura: una
    // edición nueva vuelve a intentarlo desde el primer backoff.
    f.save.blocked_writes.retain(|id, entry| {
        f.deck.find_by_id(*id).is_some_and(|index| {
            let deck::SlotContent::Ready(doc) = &f.deck.slots[index].content else {
                return false;
            };
            doc.history.revision() == entry.revision
        })
    });
}

/// Ranuras de fondo que la cola debería escribir ahora mismo, en orden de
/// baraja: sucias, no bloqueadas por un fallo de esta revisión y con un
/// destino que la app puede escribir SIN preguntar. Pura respecto a la GPU y
/// al frame (tabla de tests).
pub(super) fn dirty_background_slots(
    deck: &deck::Deck,
    save: &super::SaveFlow,
    skip_overwrite_warning: bool,
    now: Instant,
) -> Vec<u64> {
    let mut ids = Vec::new();
    for (index, slot) in deck.slots.iter().enumerate() {
        if index == deck.active {
            continue;
        }
        let deck::SlotContent::Ready(doc) = &slot.content else {
            continue;
        };
        // Una escritura en vuelo ya está en la cola; una limpia no debe
        // volver a hornearse (el caso que hacía re-renderizar el mismo clip
        // cada `VIDEO_QUIET`).
        if doc.saving || !doc.history.is_dirty() {
            continue;
        }
        if is_blocked(save, slot.id, doc.history.revision(), now) {
            continue;
        }
        let runnable = if is_clip_document(&doc.doc) {
            clip_save_target(
                &doc.doc,
                doc.background_layer,
                &slot.path,
                slot.is_placeholder,
                deck.folder.as_deref(),
            )
            .is_some()
        } else {
            matches!(
                background_target(BackgroundRequest {
                    doc: &doc.doc,
                    is_placeholder: slot.is_placeholder,
                    is_design: doc.is_design,
                    born_blank: doc.born_blank,
                    external_change: doc.external_change,
                    app_written: doc
                        .doc
                        .source_path
                        .as_deref()
                        .is_some_and(|path| save.is_app_written(path)),
                    overwrite_ok: doc
                        .doc
                        .source_path
                        .as_deref()
                        .is_some_and(|path| save.overwrite_ok(path)),
                    skip_overwrite_warning,
                }),
                BackgroundWrite::Auto(_)
            )
        };
        if runnable {
            ids.push(slot.id);
        }
    }
    ids
}

/// ¿Esta ranura sigue bloqueada? Sí mientras el fallo sea de ESTA revisión y
/// su reintento no haya vencido (o esté aparcada). Un fallo viejo no gasta un
/// horneado y uno transitorio vuelve solo cuando toca.
fn is_blocked(save: &super::SaveFlow, slot: u64, revision: u64, now: Instant) -> bool {
    match save.blocked_writes.get(&slot) {
        Some(entry) if entry.revision == revision => match entry.retry_at {
            Some(at) => now < at,
            None => true,
        },
        _ => false,
    }
}

/// ¿Puede arrancar una escritura AHORA? Los mismos «no» que espera un
/// guardado: una en vuelo, un diálogo abierto o una edición a medias no
/// pueden quedar partidos por una escritura que el usuario no pidió.
fn ready_to_start(state: &editor::EditorState, f: &EditorFrame<'_>) -> bool {
    let generation = f.deck.generation();
    if !f.save.writes.busy(generation) {
        return false;
    }
    if f.save.last_write.is_some_and(|at| at.elapsed() < MIN_GAP) {
        return false;
    }
    if state.saving || state.exporting || state.framing.is_some() || state.ytdlp.edit.is_some() {
        return false;
    }
    if f.unsaved_dialog_open || !state.is_idle_except_save() {
        return false;
    }
    !(f.save.save_path_dialog_open
        || f.save.pending_save_as.is_some()
        || !f.save.save_all_queue.is_empty()
        || f.save.readonly_prompt.is_some()
        || f.save.overwrite_prompt.is_some()
        || f.save.discard_raster_prompt.is_some()
        || f.save.low_memory_prompt.is_some())
}

#[cfg(test)]
#[path = "write_queue_tests.rs"]
mod tests;
