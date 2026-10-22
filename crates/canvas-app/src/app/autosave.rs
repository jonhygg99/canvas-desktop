//! Autoguardado silencioso del lienzo activo: cuando el diseño CAMBIA y el
//! editor lleva un momento quieto, se guarda solo — sin diálogos y sin el
//! aviso de «operación en curso» al volver a la galería.
//!
//! Reglas (todas en `autosave_path`, pura y con tabla de tests):
//!
//! - Solo el lienzo ACTIVO (los de fondo del deck siguen con «Save all»).
//! - Nunca bajo un modal o un diálogo (ruta, «Save as…», avisos, «Save all»,
//!   y el diálogo de «cambios sin guardar»): guardar por debajo de un aviso
//!   confundiría al usuario sobre qué decidió. Si el diálogo ofrece
//!   «Discard», escribir antes de que conteste haría que «Discard» mintiera.
//! - Nunca mientras el archivo esté marcado como «cambió en disco»
//!   (`external_change`): el banner pregunta Reload / Keep mine, y escribir
//!   por detrás resolvería la pregunta pisando el cambio ajeno sin permiso.
//! - Nunca mientras el editor no está ocioso (`is_idle_except_save`): a mitad
//!   de un arrastre, un slider o un texto inline se guardaría un estado que
//!   el usuario todavía está moviendo.
//! - Nunca un formato que no se puede sobrescribir (SVG/GIF) ni un vídeo
//!   (renderizar el mp4 es caro y no es «guardar el diseño»).
//! - Nunca el PRIMER guardado destructivo sin confirmar, con las mismas dos
//!   compuertas que el `Ctrl+S` manual. La sobrescritura del original se
//!   PREGUNTA una vez por archivo (si no, un lienzo abierto del disco no se
//!   autoguardaba nunca); aplanar un documento sin capas de imagen se queda
//!   bloqueando, porque perder la copia editable no es algo que un
//!   temporizador deba proponer.
//!
//! «Siempre» exige dos cosas que el temporizador resuelve aquí:
//!
//! - **Que el silencio se cumpla aunque la ventana esté en reposo.** egui deja
//!   de pintar cuando nada cambia, así que el temporizador pide
//!   `request_repaint_after` para el instante en que toca: sin ese latido, el
//!   cambio se quedaría sin escribir hasta que el usuario tocara algo.
//! - **Que un fallo transitorio no espere a otra edición.** Un rechazo (RAM
//!   crítica, GPU ocupada, horneado inestable) o un fallo de escritura se
//!   reintenta solo con backoff; solo se rinde tras `RETRY_BACKOFF.len()`
//!   intentos y una revisión nueva lo rearma. Un fallo permanente (destino
//!   sin permisos, formato sin codificador) se rinde a la primera.
//!
//! **Coste del horneado, y por qué el silencio es fijo.** El guardado que
//! lanza el temporizador hornea la página en el hilo de UI, igual que el
//! `Ctrl+S` manual (`start_save`): en un documento grande esa pausa cuesta lo
//! que cueste el horneado, y `doc_needs_bake_verify` puede añadir una segunda
//! pasada. El silencio es 1,5 s para TODOS los documentos a propósito: una
//! ventana mayor para documentos pesados se probó y se descartó —cuánto
//! esperar a tener el trabajo en disco lo decide el usuario, no el número de
//! megapíxeles—, y mover el horneado a un hilo aparte exige sacar de la UI el
//! renderer y sus cachés (refactor que no entra aquí). Cuando no toca
//! guardar, el temporizador no cuesta nada: solo pide un repintado para el
//! instante en que vence.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui;
use eframe::egui_wgpu::RenderState;

use crate::deck;
use crate::editor::EditorState;

use super::clip_save::{self, clip_save_target};
use super::frame::EditorFrame;
use super::persistence::{
    has_raster_layers, start_save, start_save_design, SaveContext, SaveStart,
};
use super::SaveFlow;

/// Silencio exigido tras el último cambio antes de escribir. Suficiente para
/// no guardar entre dos pulsaciones de un mismo ajuste, corto para que el
/// lienzo esté en disco cuando el usuario mire el explorador.
pub(super) const QUIET: Duration = Duration::from_millis(1500);

/// Silencio exigido para un lienzo de CLIP. Renderizar un mp4 (x264, quizá
/// minutos de vídeo) cuesta muchísimo más que hornear un PNG, así que se le da
/// más margen: el autoguardado no debe re-codificar el clip entre dos ajustes
/// de trim. Con un solo render en vuelo, el resto espera a que termine.
pub(super) const VIDEO_QUIET: Duration = Duration::from_secs(5);

/// Esperas tras cada fallo, por intento. Un fallo transitorio (memoria, GPU,
/// archivo bloqueado) se reintenta solo; al agotar la tabla se espera a una
/// edición nueva (el banner ya explica el motivo).
const RETRY_BACKOFF: [Duration; 3] = [
    Duration::from_secs(5),
    Duration::from_secs(15),
    Duration::from_secs(60),
];

/// Estado del autoguardado para la UI. Solo importa lo que el usuario no
/// puede deducir del documento: que hay un fallo y que se está reintentando.
/// «Saving…» / «Saved» ya los dice el propio editor (`save_status`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AutosaveStatus {
    Idle,
    Retrying,
}

/// Temporizador del autoguardado. Uno por ventana (vive en `SaveFlow`).
#[derive(Default)]
pub(crate) struct Autosave {
    /// Documento al que pertenece el ciclo actual. Cambiar de lienzo (o
    /// abrir otro archivo) resetea intentos y esperas: las revisiones de
    /// historiales distintos coinciden por casualidad (todas empiezan en 0) y
    /// no deben heredar el estado de la anterior.
    seen_doc: Option<PathBuf>,
    /// Revisión del historial vista por última vez. Cambia con cada paso de
    /// deshacer (y con undo/redo), así que un cambio reinicia la espera.
    seen_revision: u64,
    /// Instante del último cambio observado con el documento sucio.
    armed_at: Option<Instant>,
    /// Momento a partir del cual se puede reintentar tras un fallo.
    retry_at: Option<Instant>,
    /// Intentos fallidos de la revisión actual (se reinicia con cada edición).
    attempts: usize,
    /// Revisión en la que el usuario cerró el aviso de sobrescritura con
    /// «Cancelar». Mientras esa siga siendo la revisión actual, el
    /// temporizador no dispara: sin esto, `tick` volvía a armar el ciclo en
    /// cuanto `armed_at` quedaba vacío y el aviso saltaba cada 1,5 s. Una
    /// edición nueva (otra revisión) lo vuelve a armar y a preguntar.
    declined_revision: Option<u64>,
    /// Silencio que se usó al armar el ciclo (ver `VIDEO_QUIET`): el latido de
    /// `next_wake` tiene que esperar lo mismo que se exigirá al disparar.
    quiet: Duration,
}

impl Autosave {
    /// `dirty` es `EditorState::is_dirty`, `allowed` lo que decide
    /// `autosave_path`. Devuelve `true` cuando toca intentar el guardado AHORA
    /// (y solo entonces; el fallo se registra con `on_failed`).
    pub(super) fn tick(
        &mut self,
        now: Instant,
        doc: Option<&std::path::Path>,
        revision: u64,
        dirty: bool,
        allowed: bool,
        quiet: Duration,
    ) -> bool {
        if self.seen_doc.as_deref() != doc {
            self.seen_doc = doc.map(std::path::Path::to_path_buf);
            self.armed_at = None;
            self.retry_at = None;
            self.attempts = 0;
            self.declined_revision = None;
        }
        if !dirty {
            // Limpio = el último guardado cuajó (o el usuario deshizo hasta el
            // punto guardado): la próxima edición vuelve a armar el ciclo.
            self.armed_at = None;
            self.retry_at = None;
            self.attempts = 0;
            self.declined_revision = None;
            return false;
        }
        if self.seen_revision != revision {
            self.seen_revision = revision;
            self.quiet = quiet;
            self.armed_at = Some(now);
            self.retry_at = None;
            self.attempts = 0;
            // Edición nueva: el «no» anterior ya no aplica a lo que hay ahora
            // en pantalla, así que vuelve a estar permitido preguntar.
            self.declined_revision = None;
            return false;
        }
        if self.declined_revision == Some(revision) {
            return false;
        }
        if self.armed_at.is_none() {
            self.quiet = quiet;
        }
        let armed_at = *self.armed_at.get_or_insert(now);
        if !allowed || self.attempts >= RETRY_BACKOFF.len() {
            return false;
        }
        if now.saturating_duration_since(armed_at) < self.quiet {
            return false;
        }
        if self.retry_at.is_some_and(|retry| now < retry) {
            return false;
        }
        true
    }

    /// Cuánto falta para el próximo frame útil: lo que queda del silencio o
    /// el reintento programado. `None` si no hay nada armado o se agotaron
    /// los intentos (entonces solo una edición nueva lo rearma). Pura.
    pub(super) fn next_wake(&self, now: Instant) -> Option<Duration> {
        let armed_at = self.armed_at?;
        if self.attempts >= RETRY_BACKOFF.len() {
            return None;
        }
        let wait = match self.retry_at {
            Some(retry) => retry.saturating_duration_since(now),
            None => self
                .quiet
                .saturating_sub(now.saturating_duration_since(armed_at)),
        };
        // Nunca 0: `request_repaint_after(0)` gira el bucle sin avanzar reloj.
        Some(wait.max(Duration::from_millis(1)))
    }

    /// El guardado lanzado por el autoguardado terminó bien: se puede volver
    /// a intentar (si hubo ediciones durante la escritura) sin backoff.
    pub(super) fn on_saved(&mut self) {
        self.attempts = 0;
        self.retry_at = None;
    }

    /// El guardado no llegó a escribirse (rechazo inmediato) o falló: programa
    /// el siguiente intento, o se rinde hasta la próxima edición.
    pub(super) fn on_failed(&mut self, now: Instant) {
        self.attempts = self.attempts.saturating_add(1);
        self.retry_at = retry_delay(self.attempts).map(|delay| now + delay);
        self.armed_at.get_or_insert(now);
    }

    /// El usuario eligió «Cancelar» en el aviso de sobrescritura: esta
    /// revisión no se escribe y el ciclo queda DESARMADO (si no, el aviso
    /// volvería a saltar cada 1,5 s y, peor, el temporizador se quedaría
    /// disparando contra una pregunta ya contestada sin escribir nada: el
    /// lienzo se veía «Unsaved changes» para siempre). La próxima edición
    /// vuelve a armar el ciclo y a preguntar.
    pub(super) fn on_overwrite_declined(&mut self) {
        self.armed_at = None;
        self.retry_at = None;
        self.attempts = 0;
        self.declined_revision = Some(self.seen_revision);
    }

    /// El fallo no se va a arreglar solo (destino de solo lectura, formato
    /// que no se puede escribir): se deja de reintentar y el banner ya
    /// explica el motivo. Una edición nueva vuelve a armar el ciclo.
    pub(super) fn give_up(&mut self) {
        self.attempts = RETRY_BACKOFF.len();
        self.retry_at = None;
    }

    /// El usuario eligió «Discard»: esta revisión no se escribe. La vista se
    /// destruye justo después, pero entre la respuesta del diálogo y la
    /// navegación todavía corre un frame del editor en el que el temporizador
    /// podría disparar; desarmarlo lo impide. La próxima edición (o el próximo
    /// documento) vuelve a armar el ciclo.
    pub(super) fn on_discarded(&mut self) {
        self.armed_at = None;
        self.retry_at = None;
        self.attempts = 0;
    }

    /// ¿Hay un fallo con reintento programado? Lo usa el indicador del editor.
    pub(super) fn status(&self) -> AutosaveStatus {
        if self.attempts > 0 && self.retry_at.is_some() {
            AutosaveStatus::Retrying
        } else {
            AutosaveStatus::Idle
        }
    }
}

/// ¿Un fallo que no se arregla reintentando? `canvas-io` ya reintenta las
/// violaciones de compartición (el archivo lo tiene abierto otro programa)
/// dentro de `replace_with_retry`, así que lo que llega aquí es un destino
/// que no admite la escritura (permisos, solo lectura, disco lleno) o un
/// problema de formato/codificación: reintentarlo solo repite un horneado
/// completo y otro banner igual. Pura (tabla de tests).
pub(super) fn is_permanent_save_error(error: &canvas_io::IoError) -> bool {
    matches!(
        error,
        canvas_io::IoError::Write { .. }
            | canvas_io::IoError::Encode { .. }
            | canvas_io::IoError::UnsupportedFormat { .. }
    )
}

/// Espera antes del intento número `attempt` (1 → 5 s, 2 → 15 s, 3 → 60 s;
/// `None` cuando ya no se reintenta). Pura (tabla de tests).
fn retry_delay(attempt: usize) -> Option<Duration> {
    RETRY_BACKOFF.get(attempt.checked_sub(1)?).copied()
}

/// Qué le toca hacer al autoguardado en este frame (ver reglas en el doc del
/// módulo). Pura: la decide `autosave_action`, sin relojes ni GPU.
#[derive(Clone, Debug, PartialEq, Eq)]
enum AutosaveAction {
    /// Escribir este archivo.
    Save(PathBuf),
    /// El primer guardado de este archivo necesita la misma confirmación
    /// destructiva que pide `Ctrl+S`. Antes se negaba en silencio y un archivo
    /// abierto desde el disco no se autoguardaba NUNCA; ahora se pregunta una
    /// vez y, si acepta, se guarda como cualquier otro.
    AskOverwrite(PathBuf),
    /// No tocar nada. El texto es el MOTIVO (inglés, va a la barra de estado):
    /// un autoguardado que no hace nada en silencio deja al usuario mirando
    /// «Unsaved changes» sin saber qué lo bloquea (el banner de cambio
    /// externo, un diálogo, un gesto a medias…).
    Skip(&'static str),
}

/// Qué debe hacer el autoguardado ahora con el lienzo activo. Pura.
fn autosave_action(
    state: &EditorState,
    save: &SaveFlow,
    deck: &deck::Deck,
    skip_overwrite_warning: bool,
    dialog_open: bool,
) -> AutosaveAction {
    if !state.is_dirty() {
        return AutosaveAction::Skip("no changes");
    }
    if state.saving {
        return AutosaveAction::Skip("the save that is still writing");
    }
    if state.exporting {
        return AutosaveAction::Skip("the export that is still running");
    }
    // Decisiones del usuario pendientes: el banner de cambio externo
    // (Reload / Keep mine) y el diálogo de «cambios sin guardar» (Save /
    // Discard). Escribir por detrás resolvería ambas por el usuario.
    if state.external_change {
        return AutosaveAction::Skip("the ⚠ file-changed-on-disk banner");
    }
    if dialog_open {
        return AutosaveAction::Skip("the unsaved-changes dialog");
    }
    if let Some(blocker) = state.autosave_blocker() {
        return AutosaveAction::Skip(blocker);
    }
    if state.framing.is_some() {
        return AutosaveAction::Skip("the framing view");
    }
    if state.ytdlp.edit.is_some() {
        return AutosaveAction::Skip("the video edit window");
    }
    if save.save_path_dialog_open {
        return AutosaveAction::Skip("the “Save as…” dialog");
    }
    if save.pending_save_as.is_some() {
        return AutosaveAction::Skip("a pending “Save as…”");
    }
    if !save.save_all_queue.is_empty() {
        return AutosaveAction::Skip("the “Save all” queue");
    }
    if save.readonly_prompt.is_some() {
        return AutosaveAction::Skip("the read-only-file dialog");
    }
    if save.overwrite_prompt.is_some() {
        return AutosaveAction::Skip("the overwrite confirmation");
    }
    if save.discard_raster_prompt.is_some() {
        return AutosaveAction::Skip("the “no image layers left” dialog");
    }
    if save.low_memory_prompt.is_some() {
        return AutosaveAction::Skip("the low-memory dialog");
    }
    // La cola única de escritura ya está escribiendo (un clip recién añadido o
    // un lienzo de fondo sucio): el activo espera a que le toque, porque en
    // toda la ventana hay un solo worker a la vez (ver `app/write_queue.rs`).
    if save.writes.busy(deck.generation()) {
        return AutosaveAction::Skip("the canvas that is still being written");
    }
    // Un lienzo de CLIP no pasa por las compuertas del raster: no hay foto
    // original que pisar (el destino es el nombre que la app reservó o el mp4
    // que ella misma renderizó), así que tampoco hay aviso que pedir. El
    // destino lo decide el mismo sitio que el `Ctrl+S` (`clip_save`): sin
    // fuente en disco, capas extra o baraja sin carpeta, no se escribe.
    if clip_save::is_clip_document(&state.doc) {
        let Some(slot) = deck.slots.get(deck.active) else {
            return AutosaveAction::Skip("a deck with no active canvas");
        };
        return match clip_save_target(
            &state.doc,
            state.background_layer,
            &slot.path,
            slot.is_placeholder,
            deck.folder.as_deref(),
        ) {
            Some(dst) => AutosaveAction::Save(dst),
            None => AutosaveAction::Skip("a clip with no automatic destination"),
        };
    }
    let Some(path) = state.doc.source_path.clone() else {
        return AutosaveAction::Skip("a canvas that was never saved");
    };
    // SVG/GIF solo lectura y vídeos (sin escritura en el sitio): `Ctrl+S`
    // tampoco los sobrescribe, así que el autoguardado no puede hacerlo.
    if !canvas_io::can_overwrite(&path) {
        return AutosaveAction::Skip("a format that is read-only in place");
    }
    if state.is_design {
        return if canvas_io::is_canvas_file(&path) {
            AutosaveAction::Save(path)
        } else {
            AutosaveAction::Skip("a design without a .canvas destination")
        };
    }
    // El aviso de «se borró la última foto» no se pregunta solo: aplanar el
    // documento y descartar la copia editable es una decisión del usuario, y
    // dos avisos seguidos desde un temporizador no son «guardar solo». Se
    // queda bloqueando hasta que lo responda un `Ctrl+S`.
    if !state.born_blank && !save.discard_raster_ok(&path) && !has_raster_layers(&state.doc) {
        return AutosaveAction::Skip("the “no image layers left” decision");
    }
    // Sobrescritura del original: la misma confirmación que pide `Ctrl+S`,
    // por ARCHIVO (la de otro canvas no vale). NO se pregunta por un archivo
    // que escribió la propia app (bulk, «Save as», lienzo nuevo): no hay
    // píxeles originales del usuario que este guardado pueda destruir, y
    // preguntarlo convertía una tarea normal (editar algo recién creado) en
    // un aviso que, al cancelarlo, dejaba el autoguardado parado en silencio.
    if !state.born_blank
        && !save.is_app_written(&path)
        && !save.overwrite_ok(&path)
        && !skip_overwrite_warning
    {
        return AutosaveAction::AskOverwrite(path);
    }
    AutosaveAction::Save(path)
}

/// Punto de entrada del frame: mueve el temporizador y, si toca, lanza el
/// guardado normal (`start_save`), que hornea, escribe atómico y marca el
/// historial como guardado al recibir `Saved`.
///
/// Se llama DESPUÉS del lienzo y de los paneles (ver `editor_view_ui`): una
/// edición hecha en este mismo frame tiene que contar como cambio antes de
/// que el temporizador decida, o la última edición antes de que egui se
/// duerma se quedaría sin ver.
pub(super) fn tick(
    state: &mut EditorState,
    ctx: &egui::Context,
    rs: &RenderState,
    f: &mut EditorFrame<'_>,
) {
    let now = Instant::now();
    let is_clip = clip_save::is_clip_document(&state.doc);
    // Un clip tarda mucho más en escribirse: se le exige más silencio.
    let quiet = if is_clip { VIDEO_QUIET } else { QUIET };
    let action = autosave_action(
        state,
        f.save,
        f.deck,
        f.settings.skip_overwrite_warning,
        f.unsaved_dialog_open,
    );
    // `Skip` es lo único que además apaga el reloj: pedir la confirmación
    // destructiva sí es parte del ciclo (el temporizador tiene que llegar a
    // vencer el silencio antes de enseñar el aviso).
    let allowed = !matches!(action, AutosaveAction::Skip(_));
    // El motivo viaja a la barra de estado: «Unsaved changes» a secas deja al
    // usuario sin saber que hay algo esperando (el banner de cambio externo,
    // un diálogo, un gesto a medias…) y parece que el autoguardado está roto.
    // Además se registra UNA vez por cambio: un lienzo que lleva rato sin
    // guardarse deja en el log qué lo está frenando, sin inundarlo (el motivo
    // se reevalúa cada frame).
    let previous_block = f.save.autosave_blocked;
    f.save.autosave_blocked = match &action {
        AutosaveAction::Skip(reason) => Some(*reason),
        _ => None,
    };
    if previous_block != f.save.autosave_blocked {
        match f.save.autosave_blocked {
            Some(reason) => tracing::info!(motivo = reason, "autoguardado: en pausa"),
            None => tracing::info!("autoguardado: reanudado"),
        }
    }
    // Identidad del ciclo: para un clip, la ruta de su RANURA (el nombre
    // reservado o el mp4 que se reescribe), no el temporal de Download — dos
    // tramos del mismo vídeo comparten temporal y no deben heredarse intentos
    // ni la pregunta de sobrescritura del otro.
    let identity = if is_clip {
        f.deck
            .slots
            .get(f.deck.active)
            .map(|slot| slot.path.as_path())
    } else {
        state.doc.source_path.as_deref()
    };
    let fire = f.save.autosave.tick(
        now,
        identity,
        state.history.revision(),
        state.is_dirty(),
        allowed,
        quiet,
    );
    if !fire {
        // Latido: egui se detiene cuando nada cambia, así que sin pedir el
        // repintado el silencio de 1,5 s podría no cumplirse nunca (la
        // ventana en reposo no pinta frames y el reloj no avanza). Se pide
        // solo cuando hay algo armado y permitido: con un diálogo abierto, la
        // interacción del usuario ya genera los frames.
        if allowed {
            if let Some(delay) = f.save.autosave.next_wake(now) {
                ctx.request_repaint_after(delay);
            }
        }
        return;
    }
    let path = match action {
        AutosaveAction::Save(path) => path,
        AutosaveAction::AskOverwrite(path) => {
            // El aviso se enseña mientras no haya otro abierto (`autosave_action`
            // ya devuelve `Skip` con el modal en pantalla). «Cancelar» lo cierra
            // y DESARMA el ciclo (`Autosave::on_overwrite_declined`): sin eso,
            // el temporizador seguía disparando contra una pregunta ya
            // contestada y el lienzo se quedaba en «Unsaved changes» para
            // siempre, sin ningún aviso visible de que el autoguardado estaba
            // parado.
            tracing::info!(
                ruta = %path.display(),
                "autoguardado: primer guardado de este archivo, se pide la confirmación"
            );
            f.save.overwrite_dont_ask = false;
            f.save.overwrite_prompt = Some(path);
            ctx.request_repaint();
            return;
        }
        AutosaveAction::Skip(_) => return,
    };
    tracing::info!(
        ruta = %path.display(),
        revision = state.history.revision(),
        intento = f.save.autosave.attempts + 1,
        "autoguardado: se guarda el lienzo activo sin preguntar"
    );
    let started = if is_clip {
        // El render del clip no hornea en la UI: sella la captura y delega en
        // el worker de vídeo, igual que el `Ctrl+S` de este lienzo.
        clip_save::start_clip_save(state, f, ctx, path)
    } else {
        let mut sctx = SaveContext {
            renderer: f.renderer,
            rs,
            tx: f.tx,
            ctx,
            ignore_fs_events_until: f.ignore_fs_events_until,
            scope: f.deck.slots.get(f.deck.active).map_or(0, |s| s.scope),
        };
        if state.is_design {
            start_save_design(state, &mut sctx, path, false)
        } else {
            start_save(
                state,
                &mut sctx,
                f.deck,
                path,
                false,
                f.settings.jpeg_quality,
            )
        }
    };
    match started {
        SaveStart::Started => f.save.autosave_in_flight = true,
        // Rechazo antes de escribir (RAM crítica, horneado vacío): no habrá
        // mensaje `Saved`, así que el reintento se programa aquí.
        SaveStart::Refused => f.save.autosave.on_failed(now),
        // Destino sin codificador: reintentar solo repite el horneado.
        SaveStart::RefusedPermanent => f.save.autosave.give_up(),
    }
    // El estado («Saving…» o el error del banner) se dibujó antes en este
    // frame: un repintado lo enseña ya, sin esperar a que el worker termine.
    ctx.request_repaint();
}

#[cfg(test)]
#[path = "autosave_tests.rs"]
mod tests;
