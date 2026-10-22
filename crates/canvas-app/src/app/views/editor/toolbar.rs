//! Navegación y acciones del documento, independientes del inspector.
use super::super::super::autosave::AutosaveStatus;
use super::super::super::frame::EditorFrame;
use crate::{editor, loader, menus};
use eframe::egui;

pub(super) fn show(
    state: &mut editor::EditorState,
    ui: &mut egui::Ui,
    f: &mut EditorFrame<'_>,
) -> Option<menus::MenuAction> {
    let mut action = None;
    // Estado del autoguardado ANTES del cierre: dentro del panel `f` se presta
    // mutable otra vez para las acciones.
    let autosave_status = f.save.autosave.status();
    // Por qué el autoguardado no escribe, si hay algo que lo impide (ver
    // `autosave::tick`): un estado sucio que no avanza necesita explicación.
    let autosave_blocked = f.save.autosave_blocked;
    // Lienzos que todavía se están escribiendo (la cola única de
    // `app/write_queue.rs`) más los que esperan su reintento programado: el
    // usuario ve que la app sigue guardando sola.
    let pending_writes = f.save.writes.remaining(f.deck.generation()) + f.save.retrying_writes();
    // Y los que ya NO se van a escribir solos (fallo permanente o reintentos
    // agotados): sin este aviso, un lienzo que se quedó sin guardar parecía
    // «Saved» porque su ranura no está en pantalla.
    let parked_writes = f.save.parked_writes();
    egui::Panel::top("document_toolbar").show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            if state.from_gallery.is_some()
                && ui.button(crate::i18n::tr("Back to gallery")).clicked()
            {
                state.return_requested = true;
            }
            crate::editor::properties_panel::file_name_ui(state, ui);
            ui.separator();
            ui.label(crate::i18n::tr(&save_status(
                state,
                autosave_status,
                pending_writes,
                parked_writes,
                autosave_blocked,
            )));
            if ui.button(crate::i18n::tr("Settings")).clicked() {
                state.settings_clicked = true;
            }
        });
        ui.horizontal_wrapped(|ui| {
            editor::document_bar::mode_ui(state, ui);
            ui.separator();
            action = actions(state, ui, f);
            editor::history_panel::show(state, ui);
            editor::palette::button(state, ui);
            if ui.button(crate::i18n::tr("Help")).clicked() {
                ui.data_mut(|d| {
                    let id = egui::Id::new("editor-help");
                    let open = d.get_temp::<bool>(id).unwrap_or(false);
                    d.insert_temp(id, !open);
                });
            }
        });
        help(state, ui);
    });
    editor::palette::show(state, ui.ctx());
    action
}

fn actions(
    state: &mut editor::EditorState,
    ui: &mut egui::Ui,
    f: &mut EditorFrame<'_>,
) -> Option<menus::MenuAction> {
    let mut action = None;
    let busy = state.saving || state.exporting || state.framing.is_some();
    ui.add_enabled_ui(!busy, |ui| {
        if ui
            .button(crate::i18n::tr("Save editable design…"))
            .on_hover_text(crate::i18n::tr(
                "Preserves layers in a standalone .canvas file",
            ))
            .clicked()
        {
            if state.is_design {
                state.save_clicked = true;
            } else {
                f.save.save_path_dialog_open = true;
                loader::spawn_pick_design_path(
                    Some(state.file_name()),
                    f.tx.clone(),
                    ui.ctx().clone(),
                );
            }
        }
        if !state.is_design
            && ui
                .button(crate::i18n::tr("Overwrite original"))
                .on_hover_text(crate::i18n::tr(
                    "Replaces the source image; the existing confirmation still applies",
                ))
                .clicked()
        {
            state.save_clicked = true;
        }
        if ui.button(crate::i18n::tr("Save as…")).clicked() {
            state.save_as_clicked = true;
        }
        if ui.button(crate::i18n::tr("Export image…")).clicked() {
            action = Some(menus::MenuAction::Export);
        }
    });
    action
}

fn help(state: &editor::EditorState, ui: &mut egui::Ui) {
    // Ayuda progresiva: no ocupa permanentemente el inspector.
    let help = egui::Id::new("editor-help");
    if ui.data(|d| d.get_temp::<bool>(help).unwrap_or(false)) {
        let command = crate::i18n::command();
        ui.weak(format!(
            "{} · {command}+wheel: {} · {command}+0: {}",
            crate::i18n::tr("Wheel: pan · Space: pan"),
            crate::i18n::tr("Zoom"),
            crate::i18n::tr("Fit to Window")
        ));
        ui.weak(format!(
            "{command}+S: {} · {command}+Z: {} · {command}+C/V: {}/{}",
            crate::i18n::tr("Save"),
            crate::i18n::tr("Undo"),
            crate::i18n::tr("Copy"),
            crate::i18n::tr("Paste")
        ));
        ui.weak(editor::palette::context_help(state));
    }
}

/// Texto para los lienzos de fondo aparcados. Singular y plural a mano, como
/// el resto de la barra: no hay i18n por número.
fn parked_writes_text(parked: usize) -> String {
    if parked == 1 {
        "A canvas is not saved".to_owned()
    } else {
        format!("{parked} canvases are not saved")
    }
}

/// Texto de estado del guardado. `pending_writes` son los lienzos que la cola
/// única todavía tiene por escribir (clips recién añadidos, ranuras de fondo
/// que quedaron sucias y los que esperan un reintento): mientras queden, la app
/// está guardando aunque el documento de pantalla esté limpio. `parked_writes`
/// son los que ya no se reintentan solos (fallo permanente o esperas agotadas).
fn save_status(
    state: &editor::EditorState,
    autosave: AutosaveStatus,
    pending_writes: usize,
    parked_writes: usize,
    autosave_blocked: Option<&'static str>,
) -> String {
    let fixed = |text: &str| text.to_owned();
    if let Some(session) = &state.framing {
        return fixed(if session.busy() {
            "Processing framing…"
        } else if session.is_dirty() {
            "Framing not saved"
        } else {
            "Framing saved"
        });
    }
    if state.saving {
        fixed("Saving…")
    } else if state.exporting {
        fixed("Exporting…")
    } else if pending_writes > 0 {
        format!("Saving canvases… ({pending_writes} left)")
    } else if state.save_error.is_some() {
        // Un autoguardado que falló no se queda en «Error al guardar»: si hay
        // reintento programado, el usuario ve que la app sigue intentándolo
        // sola (el detalle del error está en el banner del inspector).
        fixed(if autosave == AutosaveStatus::Retrying {
            "Save failed — retrying…"
        } else {
            "Save failed"
        })
    } else if state.is_dirty() {
        // Un autoguardado que lleva rato sin escribir dice POR QUÉ: sin esto,
        // «Unsaved changes» fijo parecía un autoguardado roto (el motivo era
        // un banner de cambio externo, un diálogo o un gesto a medias).
        match autosave_blocked {
            Some(reason) => format!("Unsaved — autosave paused: {reason}"),
            None => fixed("Unsaved changes"),
        }
    } else if parked_writes > 0 {
        // Una ranura de fondo se quedó sin escribir (destino que no admite la
        // escritura o reintentos agotados) y no se reintenta sola hasta que el
        // usuario la abra o edite. El lienzo de pantalla puede estar limpio:
        // decir «Saved» aquí escondía trabajo sin guardar.
        parked_writes_text(parked_writes)
    } else if state.doc.source_path.is_none() {
        fixed("Not saved yet")
    } else {
        fixed("Saved")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::autosave::AutosaveStatus;
    #[test]
    fn unsaved_new_document_is_not_reported_as_saved() {
        let state = editor::EditorState::new_blank(800.0, 600.0);
        assert_ne!(
            save_status(&state, AutosaveStatus::Idle, 0, 0, None),
            "Saved"
        );
    }
    #[test]
    fn ongoing_save_takes_precedence_over_previous_error() {
        let mut state = editor::EditorState::new_blank(800.0, 600.0);
        state.save_error = Some("previous failure".into());
        state.saving = true;
        assert_eq!(
            save_status(&state, AutosaveStatus::Idle, 0, 0, None),
            "Saving…"
        );
        state.saving = false;
        assert_eq!(
            save_status(&state, AutosaveStatus::Idle, 0, 0, None),
            "Save failed"
        );
    }

    /// Un aviso de espera (`notice`) no es un fallo: la barra no puede decir
    /// «Error al guardar» mientras el guardado va bien y solo hay que esperar.
    #[test]
    fn a_waiting_notice_is_not_reported_as_a_save_failure() {
        let mut state = editor::EditorState::new_blank(800.0, 600.0);
        state.notice = Some("Leaving is waiting on a save that is still writing.".to_owned());
        assert_ne!(
            save_status(&state, AutosaveStatus::Idle, 0, 0, None),
            "Save failed"
        );
        assert_eq!(
            save_status(&state, AutosaveStatus::Idle, 0, 0, None),
            "Not saved yet"
        );
    }

    /// Un fallo del autoguardado con reintento programado lo dice: el usuario
    /// no tiene que adivinar si la app sigue intentándolo sola.
    #[test]
    fn a_retrying_autosave_is_not_reported_as_a_plain_failure() {
        let mut state = editor::EditorState::new_blank(800.0, 600.0);
        state.save_error = Some("transient failure".into());
        assert_eq!(
            save_status(&state, AutosaveStatus::Retrying, 0, 0, None),
            "Save failed — retrying…"
        );
    }

    /// Los lienzos que la cola todavía escribe se ven aunque el documento de
    /// pantalla esté limpio: la app está guardando en segundo plano, y ese
    /// archivo no es el del lienzo activo.
    #[test]
    fn pending_background_writes_are_reported_even_when_the_active_canvas_is_clean() {
        let mut state = editor::EditorState::new_blank_image(800.0, 600.0);
        state.doc.source_path = Some(std::path::PathBuf::from("foto.png"));
        state.history.mark_saved();
        assert_eq!(
            save_status(&state, AutosaveStatus::Idle, 0, 0, None),
            "Saved"
        );
        assert_eq!(
            save_status(&state, AutosaveStatus::Idle, 2, 0, None),
            "Saving canvases… (2 left)"
        );
        // Un guardado propio del activo manda sobre la cola.
        state.saving = true;
        assert_eq!(
            save_status(&state, AutosaveStatus::Idle, 2, 0, None),
            "Saving…"
        );
    }

    /// Un lienzo sucio que el autoguardado no está escribiendo dice POR QUÉ.
    /// Antes se quedaba en «Unsaved changes» para siempre y parecía que el
    /// autoguardado estaba roto (el motivo solía ser el banner de cambio
    /// externo o un diálogo esperando respuesta).
    #[test]
    fn a_blocked_autosave_says_what_it_is_waiting_on() {
        let mut state = editor::EditorState::new_blank_image(800.0, 600.0);
        state.doc.source_path = Some(std::path::PathBuf::from("foto.png"));
        state.history.mark_unsaved();
        assert_eq!(
            save_status(&state, AutosaveStatus::Idle, 0, 0, None),
            "Unsaved changes"
        );
        assert_eq!(
            save_status(
                &state,
                AutosaveStatus::Idle,
                0,
                0,
                Some("the ⚠ file-changed-on-disk banner")
            ),
            "Unsaved — autosave paused: the ⚠ file-changed-on-disk banner"
        );
        // Un guardado en vuelo manda sobre el motivo: no está «pausado».
        state.saving = true;
        assert_eq!(
            save_status(&state, AutosaveStatus::Idle, 0, 0, Some("anything")),
            "Saving…"
        );
    }

    /// Una ranura de fondo aparcada (fallo permanente o reintentos agotados)
    /// se ve aunque el lienzo de pantalla esté limpio: antes la barra decía
    /// «Saved» y ese lienzo se quedaba sin escribir para siempre.
    #[test]
    fn a_parked_background_canvas_is_not_reported_as_saved() {
        let mut state = editor::EditorState::new_blank_image(800.0, 600.0);
        state.doc.source_path = Some(std::path::PathBuf::from("foto.png"));
        state.history.mark_saved();
        assert_eq!(
            save_status(&state, AutosaveStatus::Idle, 0, 1, None),
            "A canvas is not saved"
        );
        assert_eq!(
            save_status(&state, AutosaveStatus::Idle, 0, 3, None),
            "3 canvases are not saved"
        );
        // Un lienzo que se está escribiendo manda sobre lo aparcado.
        assert_eq!(
            save_status(&state, AutosaveStatus::Idle, 1, 1, None),
            "Saving canvases… (1 left)"
        );
    }
}
