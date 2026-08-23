//! Deshacer/rehacer en dos niveles: el historial LOCAL de cada lienzo
//! (`canvas_core::History`) y el historial GLOBAL de la baraja, que ademas
//! recuerda pasos que tocan disco (crear y borrar archivos).

use std::path::PathBuf;

use canvas_core::{Command, CoreError};

use super::EditorState;

/// Un paso del deshacer/rehacer GLOBAL (`EditorState::global_undo`/
/// `global_redo`), con el id de ranura al que pertenece (o, para `Delete`,
/// la ruta borrada). `Clone` en vez de `Copy`: `Delete` carga rutas.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) enum GlobalStep {
    /// Un comando normal, ya apilado en el `History` local de esa ranura —
    /// deshacerlo/rehacerlo es `History::undo`/`redo` sobre ese diseño.
    Edit(u64),
    /// La ranura se creó en este punto de la sesión (botón "+", zona "+" del
    /// lienzo, duplicar una provisional). Deshacerlo BORRA la ranura entera
    /// (vía el mismo camino que el botón «Delete»: papelera de reciclaje si
    /// ya tenía archivo, descarte en memoria si seguía siendo provisional).
    /// Sin simétrico en `global_redo`: crear no se "rehace" borrando otra vez.
    Create(u64),
    /// Se borró un archivo real (botón «Delete»/cabecera de un lienzo de
    /// fondo — nunca el borrado que ya ocurre al deshacer un `Create`, ver
    /// `EditorState::pending_delete_from_undo`). Deshacerlo restaura el
    /// archivo desde la papelera de reciclaje — no pertenece a ninguna
    /// ranura de la baraja (esa ya no existe), así que no hace falta saltar
    /// a ningún sitio primero. Sin simétrico en `global_redo`: tampoco se
    /// "rehace" volviendo a borrar.
    Delete(DeleteRecord),
}

/// Lo necesario para restaurar desde la papelera de reciclaje un archivo
/// borrado por el usuario — ver `GlobalStep::Delete`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct DeleteRecord {
    pub path: PathBuf,
    /// El sidecar `.canvas` que se borró junto al archivo, si tenía uno.
    pub sidecar: Option<PathBuf>,
}

impl GlobalStep {
    /// Solo válido para `Edit`/`Create` (los únicos que participan en el
    /// salto de baraja `pending_global_undo`/`_redo` resuelve en
    /// `main.rs`): `Delete` se resuelve de inmediato en `undo()`, sin pasar
    /// nunca por ese campo.
    pub(crate) fn slot_id(&self) -> u64 {
        match self {
            GlobalStep::Edit(id) | GlobalStep::Create(id) => *id,
            GlobalStep::Delete(_) => {
                unreachable!("Delete nunca se deja en pending_global_undo/_redo")
            }
        }
    }
}

impl EditorState {
    /// Aplica `cmd` al documento y lo apila como paso de deshacer del diseño
    /// activo — igual que `History::apply`, pero además registra el paso en
    /// la pila GLOBAL cruzada entre diseños (`global_undo`). Todo comando
    /// real (fuera del truco interno de `push_placeholder` en `deck.rs`,
    /// que no es una edición visible del usuario) debe pasar por aquí o por
    /// `push_undo_step`, nunca por `self.history` directamente — si no, ese
    /// paso quedaría invisible para el `Ctrl+Z` global.
    pub(crate) fn apply_undo_step(&mut self, cmd: Box<dyn Command>) -> Result<(), CoreError> {
        self.history.apply(&mut self.doc, cmd)?;
        self.record_edit_step();
        Ok(())
    }

    /// Apila un comando cuyo efecto YA está reflejado en el documento (fin
    /// de un gesto continuo) — ver `apply_undo_step`.
    pub(crate) fn push_undo_step(&mut self, cmd: Box<dyn Command>) {
        self.history.push_applied(cmd);
        self.record_edit_step();
    }

    /// Apila un borrado de archivo real como paso deshacible
    /// (`GlobalStep::Delete`) — llamado por `main.rs` tras un
    /// `spawn_document_delete` que el usuario pidió directamente (nunca el
    /// que ya ocurre como consecuencia de deshacer un `Create`, ver
    /// `pending_delete_from_undo`).
    pub(crate) fn record_delete(&mut self, record: DeleteRecord) {
        self.global_undo.push(GlobalStep::Delete(record));
        self.global_redo.clear();
    }

    /// Registra en la pila global el paso que `apply_undo_step`/
    /// `push_undo_step` ya reflejaron en el `History` local. Si este lienzo
    /// nació esta sesión y su creación aún no se había anotado
    /// (`pending_creation`), antepone un `GlobalStep::Create` — así "crear
    /// este lienzo" aparece como paso deshacible en el momento EXACTO de su
    /// primera edición real, sin importar cuántas ranuras "+" de relleno
    /// automático haya habido de por medio que el usuario nunca llegó a
    /// tocar (esas no generan ningún paso: nadie las editó).
    fn record_edit_step(&mut self) {
        if std::mem::take(&mut self.pending_creation) {
            self.global_undo
                .push(GlobalStep::Create(self.active_slot_id));
        }
        self.global_undo.push(GlobalStep::Edit(self.active_slot_id));
        self.global_redo.clear();
    }

    /// ¿Hay algo que deshacer/rehacer en TODA la sesión (no solo en el
    /// diseño activo)? Gobierna si el menú/atajo Undo-Redo están activos.
    pub fn can_undo(&self) -> bool {
        !self.global_undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.global_redo.is_empty()
    }

    /// Deshace la acción más reciente de TODA la sesión (menú Edit, clic
    /// derecho o Ctrl+Z) — no solo del diseño activo. Un `Edit` del diseño
    /// activo se deshace en el sitio; un `Create` SIEMPRE pasa por
    /// `pending_global_undo` (incluso si su ranura ya es la activa, para
    /// compartir un único camino con «otro diseño») como señal para que
    /// `main.rs` pida el salto de baraja y, en cuanto toque, llame a
    /// `finish_pending_global_undo`. Un `Delete` no pertenece a ninguna
    /// ranura (el archivo ya no existe): se resuelve de inmediato, sin
    /// esperar ningún salto.
    pub fn undo(&mut self) {
        let Some(step) = self.global_undo.last().cloned() else {
            tracing::info!("deshacer: nada que deshacer");
            return;
        };
        match step {
            GlobalStep::Edit(slot_id) if slot_id == self.active_slot_id => self.undo_local(),
            GlobalStep::Delete(record) => {
                self.global_undo.pop();
                self.pending_restore = Some(record);
            }
            _ => self.pending_global_undo = Some(step),
        }
    }

    /// Rehace el último paso deshecho de TODA la sesión (menú Edit, clic
    /// derecho o Ctrl+Y) — simétrico a `undo`. `GlobalStep::Create` nunca
    /// debería aparecer aquí (deshacer una creación no deja rastro en
    /// `global_redo`); el `match` de `finish_pending_global_redo` lo cubre
    /// solo por si acaso, no como camino esperado.
    pub fn redo(&mut self) {
        let Some(step) = self.global_redo.last().cloned() else {
            tracing::info!("rehacer: nada que rehacer");
            return;
        };
        match step {
            GlobalStep::Edit(slot_id) if slot_id == self.active_slot_id => self.redo_local(),
            GlobalStep::Delete(_) => {
                // No debería pasar: `undo()` nunca deja un `Delete` en
                // `global_redo` (no se "rehace" volver a borrar).
                tracing::warn!("rehacer global: entrada de borrado inesperada, se descarta");
                self.global_redo.pop();
            }
            _ => self.pending_global_redo = Some(step),
        }
    }

    /// Deshace de verdad en el documento activo (asume que ya es el diseño
    /// correcto) y mantiene las pilas globales en sincronía con la local.
    fn undo_local(&mut self) {
        match self.history.undo(&mut self.doc) {
            Ok(true) => {
                tracing::info!("deshacer OK");
                self.global_undo.pop();
                self.global_redo.push(GlobalStep::Edit(self.active_slot_id));
            }
            Ok(false) => tracing::info!("deshacer: nada que deshacer"),
            Err(e) => {
                tracing::error!("deshacer falló: {e}");
                self.save_error = Some(format!("Undo failed: {e}"));
                // Se descarta también la entrada global: insistir en un
                // comando cuyo revert falla dejaría el deshacer global
                // atascado para siempre en el mismo paso.
                self.global_undo.pop();
            }
        }
        self.forget_deleted_selection();
    }

    /// Simétrico de `undo_local` para rehacer.
    fn redo_local(&mut self) {
        match self.history.redo(&mut self.doc) {
            Ok(true) => {
                tracing::info!("rehacer OK");
                self.global_redo.pop();
                self.global_undo.push(GlobalStep::Edit(self.active_slot_id));
            }
            Ok(false) => tracing::info!("rehacer: nada que rehacer"),
            Err(e) => {
                tracing::error!("rehacer falló: {e}");
                self.save_error = Some(format!("Redo failed: {e}"));
                self.global_redo.pop();
            }
        }
        self.forget_deleted_selection();
    }

    /// `main.rs` llama a esto en cuanto el diseño pedido por
    /// `pending_global_undo` ya es el activo. Un `Edit` se deshace en el
    /// sitio; un `Create` no tiene "documento que revertir" — en vez de eso
    /// pide borrar la ranura entera por el mismo camino que el botón
    /// «Delete» (`delete_requested`, que `main.rs` ya sabe resolver contra
    /// una provisional o un archivo real).
    pub(crate) fn finish_pending_global_undo(&mut self) {
        let Some(step) = self.pending_global_undo.take() else {
            return;
        };
        match step {
            GlobalStep::Edit(_) => self.undo_local(),
            GlobalStep::Create(_) => {
                self.global_undo.pop();
                // Marca este borrado como "consecuencia de deshacer una
                // creación", no una decisión directa del usuario: evita que
                // genere su propio `GlobalStep::Delete` (si lo hiciera,
                // podrías "deshacer el deshacer" y restaurar un lienzo que
                // tú mismo acabas de decidir descartar).
                self.pending_delete_from_undo = true;
                self.delete_requested = true;
            }
            GlobalStep::Delete(_) => {
                unreachable!("undo() resuelve Delete de inmediato, nunca lo deja pendiente")
            }
        }
    }

    /// Simétrico de `finish_pending_global_undo` para rehacer. `Create`
    /// nunca debería llegar aquí (ver `redo`) — si pasara, se descarta con
    /// aviso en vez de intentar nada.
    pub(crate) fn finish_pending_global_redo(&mut self) {
        let Some(step) = self.pending_global_redo.take() else {
            return;
        };
        match step {
            GlobalStep::Edit(_) => self.redo_local(),
            GlobalStep::Create(_) => {
                tracing::warn!("rehacer global: entrada de creación inesperada, se descarta");
                self.global_redo.pop();
            }
            GlobalStep::Delete(_) => {
                unreachable!("Delete nunca se deja en pending_global_redo")
            }
        }
    }

    /// La ranura pedida por un deshacer/rehacer cruzado ya no existe
    /// (archivo borrado entre medias, por ejemplo): descarta esa entrada de
    /// la pila global correspondiente y avisa, sin encadenar automáticamente
    /// con la siguiente — un `Ctrl+Z` más la vuelve a pedir si hace falta.
    pub(crate) fn discard_pending_global_undo(&mut self) {
        if self.pending_global_undo.take().is_some() {
            tracing::warn!("deshacer global: la ranura pedida ya no existe, se descarta ese paso");
            self.global_undo.pop();
        }
    }

    pub(crate) fn discard_pending_global_redo(&mut self) {
        if self.pending_global_redo.take().is_some() {
            tracing::warn!("rehacer global: la ranura pedida ya no existe, se descarta ese paso");
            self.global_redo.pop();
        }
    }

    /// Olvida de la selección los ids que ya no existen en el documento
    /// (tras deshacer/rehacer un borrado, o después de cortar/borrar).
    pub(crate) fn forget_deleted_selection(&mut self) {
        if let Ok(page) = self.doc.page() {
            self.selection.retain_existing(page);
        }
    }
}
