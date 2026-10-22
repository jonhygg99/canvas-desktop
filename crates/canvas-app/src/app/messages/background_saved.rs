//! Adopción del resultado de una escritura de FONDO: un lienzo que no estaba
//! en pantalla cuando se escribió (un clip de la tanda recién añadida, o
//! cualquier ranura que quedó sucia al cambiar de baraja, ver
//! `app/write_queue.rs`).
//!
//! Estas escrituras no son una salida del usuario: no tocan el documento
//! activo, ni la navegación diferida, ni el «Save all». Solo actualizan SU
//! ranura —archivo, nombre, tipo, historial— y dejan un banner en ella si algo
//! falló, para que se vea al activarla.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui;

use super::super::{AppInner, Workspace};
use crate::deck;
use crate::gallery::ItemKind;
use crate::loader;

/// Una escritura de fondo ya normalizada desde su mensaje (raster, diseño o
/// render de vídeo). El `result` llega con el mensaje de usuario ya armado:
/// cada camino de escritura habla distinto.
pub(super) struct BackgroundWriteDone {
    pub generation: u64,
    pub slot_id: u64,
    pub path: PathBuf,
    pub result: Result<(), String>,
    pub new_source: bool,
    pub capture: (usize, u64),
    /// Tipo con el que la tira y la galería deben enseñar el archivo.
    pub kind: Option<ItemKind>,
    /// El fallo no se arregla reintentando (destino sin permisos, formato sin
    /// codificador): se aparca hasta que el usuario edite. Lo transitorio (RAM,
    /// GPU, archivo bloqueado) vuelve solo con el backoff del autoguardado.
    pub permanent: bool,
}

impl AppInner {
    pub(super) fn on_background_saved(
        &mut self,
        ws: &mut Workspace,
        done: BackgroundWriteDone,
        ctx: &egui::Context,
    ) {
        if done.generation != ws.deck.generation() {
            tracing::warn!(
                ranura = done.slot_id,
                ruta = %done.path.display(),
                "escritura de fondo: llegó con otra baraja; se descarta"
            );
            return;
        }
        let Some(index) = ws.deck.find_by_id(done.slot_id) else {
            tracing::warn!(
                ranura = done.slot_id,
                ruta = %done.path.display(),
                "escritura de fondo: la ranura desapareció durante la escritura"
            );
            return;
        };
        let ok = done.result.is_ok();
        let revision = {
            let deck::SlotContent::Ready(doc) = &mut ws.deck.slots[index].content else {
                // Se activó justo en este frame: el llamador ya la habría
                // enviado por el camino activo, así que aquí solo cabe dejarlo
                // escrito (nunca se marca un guardado en falso).
                tracing::warn!(
                    ranura = done.slot_id,
                    "escritura de fondo: la ranura está activa; se resuelve por el camino activo"
                );
                return;
            };
            doc.saving = false;
            doc.saving_capture = None;
            match &done.result {
                Ok(()) => {
                    doc.save_error = None;
                    doc.doc.source_path = Some(done.path.clone());
                    doc.canvas_name = None;
                    doc.born_blank = false;
                    // La captura sellada al lanzar la escritura decide si hubo
                    // ediciones DURANTE ella: si el historial se movió, el
                    // archivo no las contiene y la ranura sigue pendiente.
                    if !doc.history.mark_saved_at(done.capture.0, done.capture.1) {
                        doc.save_error = Some(
                            "Saved, but edits made during the save are still unsaved — \
                             save again to include them."
                                .into(),
                        );
                    }
                }
                Err(message) => {
                    tracing::warn!(
                        ruta = %done.path.display(),
                        error = %message,
                        "escritura de fondo falló; la ranura sigue pendiente"
                    );
                    doc.save_error = Some(message.clone());
                }
            }
            doc.history.revision()
        };
        if ok {
            // La ranura se escribió: el fallo anterior (si lo hubo) ya no
            // aplica, ni siquiera como reintento programado.
            ws.save.blocked_writes.remove(&done.slot_id);
            if done.new_source {
                ws.save.mark_app_written(&done.path);
            }
            let slot = &mut ws.deck.slots[index];
            slot.path = done.path.clone();
            slot.name = done
                .path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            slot.is_placeholder = false;
            if let Some(kind) = done.kind {
                slot.kind = kind;
            }
            tracing::info!(
                ruta = %done.path.display(),
                ranura = done.slot_id,
                "escritura de fondo: guardada"
            );
            // La tira y la galería enseñan el archivo recién escrito.
            if let Some(folder) = done.path.parent() {
                loader::spawn_single_thumb(
                    folder.to_path_buf(),
                    done.path.clone(),
                    self.thumb_cache.clone(),
                    ws.tx.clone(),
                    ctx.clone(),
                );
            }
            ws.ignore_fs_events_until = Some(Instant::now() + Duration::from_secs(2));
        } else {
            // Transitorio → reintento programado (5 s, 15 s, 60 s) y, agotada
            // la tabla, aparcado; permanente → aparcado ya. En los dos casos
            // una edición nueva (otra revisión) lo rearma, y mientras espera no
            // se encola: nada de un horneado por frame.
            crate::app::write_queue::record_failure(
                &mut ws.save.blocked_writes,
                done.slot_id,
                revision,
                done.permanent,
                Instant::now(),
            );
        }
        ctx.request_repaint();
    }
}
