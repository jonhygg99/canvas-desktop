//! Navegación: abrir algo (archivo/carpeta/segunda instancia/arrastrar y
//! soltar), resolver y sembrar la baraja del editor, saltar entre vistas
//! (con o sin preguntar por cambios sin guardar), y los atajos de la tira
//! (eje de apilado, lado, añadir lienzo).

use std::path::{Path, PathBuf};

use eframe::egui;

use crate::{deck, editor, gallery, loader, settings};

use super::{AppInner, Nav, View, Workspace};

use super::persistence::{resolve_canvas_sidecar, seed_gallery_from_deck};

const DEFAULT_NEW_CANVAS_SIZE: (f64, f64) = (1920.0, 1080.0);

impl AppInner {
    /// Punto único de entrada para abrir algo en UN workspace, venga de
    /// argv, diálogo, arrastrar y soltar, un clic en la galería o una
    /// segunda instancia.
    pub(crate) fn open_path(&mut self, ws: &mut Workspace, path: PathBuf, ctx: &egui::Context) {
        tracing::debug!(solicitado = %path.display(), "open_path: ruta recibida");
        let path = resolve_canvas_sidecar(path);
        tracing::debug!(resuelto = %path.display(), "open_path: ruta tras sidecar");
        if path.is_dir() {
            let path = gallery::normalize_folder(path);
            let mut gallery_state = seed_gallery_from_deck(
                &ws.deck,
                path.clone(),
                self.settings.gallery_sort,
                self.settings.gallery_folder_panel_side,
            );
            gallery_state.media_filter = self.settings.media_filter;
            loader::spawn_gallery_scan(
                path.clone(),
                self.thumb_cache.clone(),
                ws.tx.clone(),
                ctx.clone(),
            );
            self.push_recent(&path);
            ws.view = View::Gallery(Box::new(gallery_state));
        } else if canvas_io::is_canvas_file(&path) {
            loader::spawn_load_design(path.clone(), ws.tx.clone(), ctx.clone());
            self.push_recent(&path);
            ws.view = View::Loading { path };
        } else if canvas_io::is_media_file(&path) {
            loader::spawn_load_image(path.clone(), true, ws.tx.clone(), ctx.clone());
            self.push_recent(&path);
            ws.view = View::Loading { path };
        } else {
            ws.view = View::Welcome {
                error: Some(format!(
                    "\"{}\" is not a supported image or video format.",
                    path.display()
                )),
                failed_path: Some(path),
                failed_kind: Some(canvas_io::CorruptionKind::Unsupported),
            };
        }
        self.sync_title(ctx, ws);
    }

    /// Documento nuevo en blanco en un workspace.
    pub(crate) fn new_design(&mut self, ws: &mut Workspace, ctx: &egui::Context) {
        let (w, h) = DEFAULT_NEW_CANVAS_SIZE;
        ws.deck = deck::Deck::new_design((w, h));
        self.apply_deck_prefs(ws);
        let state = if self.settings.new_canvas_format == settings::NewCanvasFormat::Canvas {
            let mut state = editor::EditorState::new_blank(w, h);
            state.sidecar_enabled = self.settings.sidecar_default;
            state
        } else {
            editor::EditorState::new_blank_image(w, h)
        };
        ws.view = View::Editor(Box::new(state));
        self.sync_title(ctx, ws);
    }

    /// Decide qué baraja usar al terminar de cargar `path` en el editor.
    pub(crate) fn resolve_deck(&mut self, ws: &mut Workspace, path: &Path, ctx: &egui::Context) {
        if let Some(seed) = ws.deck_ops.pending_deck.take() {
            self.initialize_seeded_deck(ws, seed, path);
            self.start_seeded_deck_preload(ws, ctx);
        } else if let Some(idx) = ws.deck.find_by_path(path) {
            ws.deck.active = idx;
        } else {
            ws.deck = deck::Deck::single(path.to_path_buf());
            self.apply_deck_prefs(ws);
        }
    }

    fn initialize_seeded_deck(
        &mut self,
        ws: &mut Workspace,
        seed: deck::DeckSeed,
        active_path: &Path,
    ) {
        ws.deck = deck::Deck::from_seed(seed, active_path);
        self.apply_deck_prefs(ws);
    }

    fn start_seeded_deck_preload(&mut self, ws: &mut Workspace, ctx: &egui::Context) {
        self.spawn_deck_probe(ctx, ws);
        self.preload_nearby_slots(ws, ctx);
    }

    fn preload_nearby_slots(&mut self, ws: &mut Workspace, ctx: &egui::Context) {
        let Some(folder) = ws.deck.folder.clone() else {
            return;
        };
        for path in ws.deck.request_loads(&[]) {
            loader::spawn_load_slot(
                folder.clone(),
                path,
                ws.deck.generation(),
                self.settings.sidecar_default,
                ws.tx.clone(),
                ctx.clone(),
            );
        }
    }

    /// Aplica preferencias persistidas a una baraja recién construida.
    pub(crate) fn apply_deck_prefs(&mut self, ws: &mut Workspace) {
        ws.deck.axis = self.settings.deck_axis;
        ws.deck.strip_visible = self.settings.deck_strip_visible;
        ws.deck.strip_side = self.settings.deck_strip_side;
    }

    /// Sondea el tamaño real de las ranuras cuyo tamaño aún se desconoce.
    pub(crate) fn spawn_deck_probe(&self, ctx: &egui::Context, ws: &Workspace) {
        let Some(folder) = ws.deck.folder.clone() else {
            return;
        };
        let paths: Vec<PathBuf> = ws
            .deck
            .slots
            .iter()
            .filter(|s| s.page.is_none())
            .map(|s| s.path.clone())
            .collect();
        if paths.is_empty() {
            return;
        }
        loader::spawn_deck_probe(
            folder,
            ws.deck.generation(),
            paths,
            ws.tx.clone(),
            ctx.clone(),
        );
    }

    /// Alterna el eje de apilado de la baraja activa y lo persiste.
    pub(crate) fn toggle_deck_axis(&mut self, ws: &mut Workspace) {
        ws.deck.axis = ws.deck.axis.toggled();
        ws.deck.layout_dirty = true;
        self.settings.deck_axis = ws.deck.axis;
        self.settings.save_in_background();
    }

    /// Mueve la tira al siguiente lado y lo persiste.
    pub(crate) fn cycle_strip_side(&mut self, ws: &mut Workspace) {
        ws.deck.strip_side = ws.deck.strip_side.cycled();
        self.settings.deck_strip_side = ws.deck.strip_side;
        self.settings.save_in_background();
    }

    /// Añade un lienzo en blanco al final de la baraja activa y salta a él.
    pub(crate) fn add_canvas(&mut self, ws: &mut Workspace) {
        let ext = self.settings.new_canvas_format.extension();
        match ws.deck.push_placeholder(self.settings.last_page_size, ext) {
            Some(idx) => {
                ws.deck.jump_to = Some(idx);
                ws.deck.jump_reframe = true;
            }
            None => tracing::info!("«Add canvas» sin efecto: la baraja no tiene carpeta"),
        }
    }

    fn new_design_in_folder(
        &mut self,
        ws: &mut Workspace,
        seed: deck::DeckSeed,
        ctx: &egui::Context,
    ) {
        let page = self.settings.last_page_size;
        let ext = self.settings.new_canvas_format.extension();
        self.initialize_seeded_deck(ws, seed, Path::new(""));
        let Some(idx) = ws.deck.push_placeholder(page, ext) else {
            self.new_design(ws, ctx);
            return;
        };
        for (slot_index, slot) in ws.deck.slots.iter_mut().enumerate() {
            if slot_index != idx && matches!(slot.content, deck::SlotContent::Active) {
                slot.content = deck::SlotContent::Idle;
            }
        }
        let mut state = if ext == canvas_io::CANVAS_EXTENSION {
            editor::EditorState::new_blank(page.0, page.1)
        } else {
            editor::EditorState::new_blank_image(page.0, page.1)
        };
        state.from_gallery = ws.deck.folder.clone();
        ws.deck.active = idx;
        if let Some(slot) = ws.deck.slots.get_mut(idx) {
            slot.content = deck::SlotContent::Active;
        }
        self.start_seeded_deck_preload(ws, ctx);
        ws.view = View::Editor(Box::new(state));
        ws.deck.layout_dirty = true;
        self.sync_title(ctx, ws);
    }

    /// Aplica una navegación diferida sobre un workspace.
    pub(crate) fn navigate(&mut self, ws: &mut Workspace, nav: Nav, ctx: &egui::Context) {
        let blocked = nav_block_reason(
            match &ws.view {
                View::Editor(state) => Some(&**state),
                _ => None,
            },
            &ws.save,
            ws.deck.generation(),
        );
        if let Some(reason) = blocked {
            self.park_nav(ws, nav, reason);
            return;
        }
        match nav {
            Nav::Open(path) => self.open_path(ws, path, ctx),
            Nav::OpenGallery { path, navigation } => {
                let path = gallery::normalize_folder(path);
                let mut gallery_state = gallery::GalleryState::with_navigation(
                    path.clone(),
                    self.settings.gallery_sort,
                    navigation,
                    self.settings.gallery_folder_panel_side,
                );
                gallery_state.media_filter = self.settings.media_filter;
                loader::spawn_gallery_scan(
                    path.clone(),
                    self.thumb_cache.clone(),
                    ws.tx.clone(),
                    ctx.clone(),
                );
                self.push_recent(&path);
                ws.view = View::Gallery(Box::new(gallery_state));
                self.sync_title(ctx, ws);
            }
            Nav::CloseProject => {
                ws.deck = deck::Deck::default();
                ws.deck_ops.pending_deck = None;
                ws.watcher = None;
                ws.view = View::Welcome {
                    error: None,
                    failed_path: None,
                    failed_kind: None,
                };
                self.sync_title(ctx, ws);
            }
            Nav::NewDesign => self.new_design(ws, ctx),
            Nav::NewDesignInFolder { seed } => self.new_design_in_folder(ws, seed, ctx),
            Nav::OpenVideo { accept } => self.open_video_canvases(ws, vec![accept], ctx),
            Nav::OpenVideos { accepts } => self.open_video_canvases(ws, accepts, ctx),
        }
    }

    /// Navega un workspace, pero si hay algún lienzo con cambios sin guardar
    /// delante pregunta primero (Save / Discard / Cancel). El modal corre en
    /// un hilo aparte y responde por `AppMsg::UnsavedDialogAnswer`: un
    /// `rfd::…::show()` sincrónico dentro del pase de un viewport diferido
    /// congela todo el event loop multi-ventana.
    ///
    /// «Save» guarda TODOS los lienzos sucios (el activo de inmediato y el
    /// resto con el flujo de «Save all», que salta y guarda uno a uno) y la
    /// navegación/cierre diferidos solo se ejecutan cuando la cola se vacía
    /// (A05): con varios sucios ya no se pierde el fondo en silencio.
    pub(crate) fn request_nav(&mut self, ws: &mut Workspace, nav: Nav, ctx: &egui::Context) {
        let blocked = nav_block_reason(
            match &ws.view {
                View::Editor(state) => Some(&**state),
                _ => None,
            },
            &ws.save,
            ws.deck.generation(),
        );
        if let Some(reason) = blocked {
            self.park_nav(ws, nav, reason);
            return;
        }
        // Nada en vuelo que deba terminar: se consolidan las ediciones
        // transitorias ANTES del chequeo de «cambios sin guardar», para que
        // cuenten como cambios y el diálogo las proteja en vez de perderse
        // al destruir la vista.
        if let View::Editor(state) = &mut ws.view {
            editor::prepare_for_navigation(state);
        }
        // Añadir conserva todos los documentos: no es una salida destructiva.
        if matches!(nav, Nav::OpenVideo { .. } | Nav::OpenVideos { .. }) {
            self.navigate(ws, nav, ctx);
            return;
        }
        let names = ws.dirty_canvas_names();
        tracing::info!(
            sucios = names.len(),
            nombres = ?names,
            "navegación: revisión de cambios sin guardar"
        );
        if names.is_empty() {
            self.navigate(ws, nav, ctx);
            return;
        }
        if ws.unsaved_dialog.is_some() {
            // Ya hay un diálogo en vuelo para esta ventana: no se apila otro
            // (el nativo se quedaría detrás del primero, invisible). Se
            // repinta para refrescar la UI y se deja constancia: antes esto
            // era un `return` mudo y el clic de Back «no hacía nada».
            tracing::info!(
                "navegación ignorada: ya hay un diálogo de cambios sin guardar en vuelo"
            );
            ctx.request_repaint();
            return;
        }
        ws.unsaved_dialog = Some(super::UnsavedDialog::Navigate(nav.clone()));
        let target = match &nav {
            Nav::Open(p) | Nav::OpenGallery { path: p, .. } => format!(
                "\"{}\"",
                p.file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| p.display().to_string())
            ),
            Nav::CloseProject => "the Welcome screen".to_owned(),
            Nav::NewDesign => "a new design".to_owned(),
            Nav::NewDesignInFolder { .. } => "a new design".to_owned(),
            Nav::OpenVideos { accepts } => format!("{} video canvases", accepts.len()),
            Nav::OpenVideo { accept } => format!(
                "\"{}\"",
                accept
                    .path
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| accept.path.display().to_string())
            ),
        };
        let description = unsaved_dialog_description(&names, &format!("opening {target}"));
        let tx = ws.tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let choice = rfd::MessageDialog::new()
                .set_level(rfd::MessageLevel::Warning)
                .set_title("Unsaved changes")
                .set_description(description)
                .set_buttons(rfd::MessageButtons::YesNoCancelCustom(
                    "Save".to_owned(),
                    "Discard".to_owned(),
                    "Cancel".to_owned(),
                ))
                .show();
            use crate::loader::{AppMsg, DialogDecision};
            let decision = match choice {
                rfd::MessageDialogResult::Yes => DialogDecision::Save,
                rfd::MessageDialogResult::Custom(ref c) if c == "Save" => DialogDecision::Save,
                rfd::MessageDialogResult::No => DialogDecision::Discard,
                rfd::MessageDialogResult::Custom(ref c) if c == "Discard" => {
                    DialogDecision::Discard
                }
                _ => DialogDecision::Cancel,
            };
            let _ = tx.send(AppMsg::UnsavedDialogAnswer(decision));
            ctx.request_repaint();
        });
    }
}

impl AppInner {
    /// Aparca una navegación SIN perderla (el drenaje de `ws_frame` la
    /// reintenta en cuanto el editor queda ocioso) y deja el motivo visible
    /// una vez: el usuario ve que su clic no se perdió y qué lo está
    /// esperando. Va al canal de avisos (`EditorState::notice`), no al de
    /// errores: no hay nada roto, solo algo que terminar.
    fn park_nav(&self, ws: &mut Workspace, nav: Nav, reason: &str) {
        tracing::info!(
            motivo = reason,
            cola = ws.deferred_nav.len(),
            "navegación aparcada; se reintenta al quedar ocioso"
        );
        // Volver a pulsar Back mientras la primera petición espera no debe
        // apilar otra salida idéntica: la cola ya tiene ese destino y se
        // ejecutará igual (antes se encolaba una por clic y el log lo
        // mostraba como cola=4, 5, 6…).
        if !ws
            .deferred_nav
            .back()
            .is_some_and(|queued| same_nav_target(queued, &nav))
        {
            ws.deferred_nav.push_back(nav);
        }
        if let View::Editor(state) = &mut ws.view {
            // Canal de AVISOS, no de errores: la barra de estado decía «Error
            // al guardar» (y el banner pintaba un ⚠) mientras el guardado iba
            // perfectamente y lo único que pasaba era que había que esperar.
            state.notice = Some(park_notice(reason));
        }
    }
}

/// ¿Las dos peticiones llevan al MISMO destino, de forma que repetir el clic
/// no aporta nada? Pura (tabla de tests). Las navegaciones ADITIVAS —crear un
/// lienzo de vídeo o un diseño nuevo— nunca se deduplican: dos peticiones
/// seguidas crean dos lienzos, no uno.
fn same_nav_target(a: &Nav, b: &Nav) -> bool {
    match (a, b) {
        (Nav::Open(x), Nav::Open(y)) => x == y,
        (Nav::OpenGallery { path: x, .. }, Nav::OpenGallery { path: y, .. }) => x == y,
        (Nav::OpenVideo { .. }, _)
        | (Nav::OpenVideos { .. }, _)
        | (Nav::NewDesignInFolder { .. }, _) => false,
        _ => std::mem::discriminant(a) == std::mem::discriminant(b),
    }
}

/// Motivo por el que una navegación no puede ejecutarse AHORA, o `None` si
/// puede. Pura: la tabla de tests fija el orden de prioridad y que solo haya
/// un motivo cuando de verdad hay un bloqueo. El último brazo es el de
/// «Save all» (si ninguno de los anteriores aplica, es esa cola).
fn nav_block_reason(
    state: Option<&editor::EditorState>,
    save: &super::SaveFlow,
    deck_generation: u64,
) -> Option<&'static str> {
    // Solo bloquea lo que de verdad debe terminar antes de cambiar de vista
    // (guardado/exportación en vuelo, framing ocupado). Un gesto, un
    // renombrado o una edición de panel a medias se consolidan o descartan
    // en `editor::prepare_for_navigation`: exigir `is_idle()` aquí dejaba
    // «Back to gallery» esperando para siempre con cualquier bandera de UI
    // atascada, sin forma de saber cuál era.
    if let Some(reason) = state.and_then(editor::EditorState::blocks_leaving) {
        return Some(reason);
    }
    if save.save_path_dialog_open {
        Some("the save-path dialog, which is still open")
    } else if save.pending_save_as.is_some() {
        Some("a pending “Save as…”")
    } else if save.writes.busy(deck_generation) {
        // La cola única de escritura tiene un lienzo en vuelo (un clip
        // recién añadido o una ranura de fondo que quedó sucia): cambiar de
        // vista tiraría el worker y el archivo se quedaría a medias (el mp4 y
        // el PNG se reemplazan al final). Se espera, como con un guardado en
        // curso.
        Some("the canvases that are still being saved")
    } else if !save.save_all_queue.is_empty() {
        Some("the “Save all” queue, which is still running")
    } else {
        None
    }
}

/// Texto visible cuando una navegación queda aparcada (`park_nav`). Pura:
/// un solo sitio para que el aviso diga siempre lo mismo.
fn park_notice(reason: &str) -> String {
    park_notice_with(reason, "Leaving")
}

/// Mismo aviso para las otras salidas que esperan (los lienzos de vídeo).
/// `action` es lo que el usuario pidió, para que el texto no invente un
/// guardado cuando lo que está en vuelo es una exportación o un framing.
pub(super) fn park_notice_with(reason: &str, action: &str) -> String {
    format!("{action} is waiting on {reason}. It will continue as soon as that finishes.")
}

/// Texto del diálogo «cambios sin guardar», compartido por la navegación
/// (`request_nav`) y el cierre de ventana (`confirm_window_close`): un solo
/// sitio para que ambos avisen lo mismo (A05). Con un lienzo sucio nombra al
/// culpable; con varios los lista y deja claro que «Save» los guarda TODOS
/// (flujo «Save all») antes de continuar, y que «No» descarta todos.
/// `target` describe el destino («opening "fotos"», «closing»…).
pub(crate) fn unsaved_dialog_description(names: &[String], target: &str) -> String {
    if names.len() == 1 {
        format!(
            "\"{}\" has unsaved changes.\nSave them before {}? (\"No\" discards them.)",
            names[0], target
        )
    } else {
        format!(
            "{} canvases have unsaved changes:\n\u{2022} {}\n\n\"Save\" saves them all before {}. \
             \"No\" discards every change.",
            names.len(),
            names.join("\n\u{2022} "),
            target
        )
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use canvas_core::LayerId;

    use crate::app::SaveFlow;
    use crate::editor;

    use super::{nav_block_reason, park_notice, unsaved_dialog_description};

    /// La compuerta del editor ya no es «no estar ocioso»: guardar en vuelo sí
    /// bloquea, pero un campo de panel a medias (la bandera atascada que dejó
    /// Back esperando) NO.
    #[test]
    fn only_an_in_flight_save_blocks_navigation_from_the_editor() {
        let save = SaveFlow::default();
        let mut state = editor::EditorState::new_blank_image(10.0, 10.0);
        assert_eq!(nav_block_reason(Some(&state), &save, 0), None);

        // `rename_edit` es una de las banderas de UI que antes bloqueaban
        // Back (la cobertura de `panel_edit` y compañía vive en
        // `editor::state::idle_tests`, que sí ve los campos privados).
        state.rename_edit = Some((LayerId::from_raw(1), "a".to_owned(), "b".to_owned()));
        state.file_rename_edit = Some("b".to_owned());
        assert_eq!(
            nav_block_reason(Some(&state), &save, 0),
            None,
            "las banderas de UI se consolidan al salir, no bloquean"
        );

        state.saving = true;
        assert_eq!(
            nav_block_reason(Some(&state), &save, 0),
            Some("a save that is still writing")
        );
    }

    #[test]
    fn nav_block_reason_prefers_the_first_gate_and_never_invents_one() {
        // (path_dialog, pending_save_as, save_all_pending, esperado)
        let cases = [
            (false, false, false, None),
            (true, false, false, Some("save-path dialog")),
            (false, true, false, Some("Save as")),
            (false, false, true, Some("Save all")),
            // Varias compuertas a la vez: manda la primera en el orden.
            (true, true, true, Some("save-path dialog")),
        ];
        for (dialog, save_as, save_all, expected) in cases {
            let save = SaveFlow {
                save_path_dialog_open: dialog,
                pending_save_as: save_as.then(|| PathBuf::from("x.png")),
                save_all_queue: if save_all { vec![1] } else { Vec::new() },
                ..SaveFlow::default()
            };
            let reason = nav_block_reason(None, &save, 0);
            assert_eq!(
                reason.is_some(),
                expected.is_some(),
                "dialog={dialog} save_as={save_as} save_all={save_all}"
            );
            if let Some(fragment) = expected {
                let reason = reason.expect("debe haber motivo");
                assert!(
                    reason.contains(fragment),
                    "motivo {reason:?} no nombra {fragment:?}"
                );
            }
        }
    }

    /// Salir mientras un lienzo se escribe (un clip o una ranura de fondo
    /// sucia) tiraría su worker y dejaría el archivo a medias: se aparca como
    /// un guardado en curso. La cola de OTRA baraja no bloquea nada (sus ids
    /// ya no existen).
    #[test]
    fn canvases_still_being_written_block_leaving_the_editor() {
        let mut save = SaveFlow::default();
        save.writes.enqueue(7, [1, 2]);
        let state = editor::EditorState::new_blank_image(10.0, 10.0);
        let reason = nav_block_reason(Some(&state), &save, 7).expect("motivo");
        assert!(reason.contains("canvases"), "{reason}");
        assert_eq!(
            nav_block_reason(Some(&state), &save, 8),
            None,
            "otra baraja no hereda la cola de escritura"
        );
        save.writes.next();
        save.writes.finished(1);
        assert!(
            nav_block_reason(Some(&state), &save, 7).is_some(),
            "el segundo lienzo aún espera"
        );
        save.writes.next();
        save.writes.finished(2);
        assert_eq!(nav_block_reason(Some(&state), &save, 7), None);
    }

    /// Volver a pulsar Back mientras la primera petición espera no apila
    /// salidas idénticas; las navegaciones que CREAN lienzos sí se apilan.
    #[test]
    fn repeating_a_parked_navigation_does_not_queue_it_twice() {
        use super::{same_nav_target, Nav};
        assert!(same_nav_target(&Nav::CloseProject, &Nav::CloseProject));
        assert!(same_nav_target(&Nav::NewDesign, &Nav::NewDesign));
        assert!(same_nav_target(
            &Nav::Open("a.png".into()),
            &Nav::Open("a.png".into())
        ));
        assert!(!same_nav_target(
            &Nav::Open("a.png".into()),
            &Nav::Open("b.png".into())
        ));
        assert!(!same_nav_target(&Nav::CloseProject, &Nav::NewDesign));
        // Aditivas: dos peticiones = dos lienzos, nunca una.
        assert!(!same_nav_target(
            &Nav::OpenVideos { accepts: vec![] },
            &Nav::OpenVideos { accepts: vec![] }
        ));
    }

    #[test]
    fn park_notice_names_the_reason_and_promises_continuation() {
        let text = park_notice("the “Save all” queue, which is still running");
        assert!(text.contains("Save all"), "{text}");
        assert!(text.contains("will continue"), "{text}");
    }

    #[test]
    fn single_dirty_names_the_canvas() {
        let text = unsaved_dialog_description(&["foto.png".to_owned()], "opening \"fotos\"");
        assert!(text.contains("\"foto.png\""), "{text}");
        assert!(text.contains("opening \"fotos\""), "{text}");
    }

    #[test]
    fn several_dirty_list_every_canvas_and_promise_save_all() {
        // A05: el fondo debe aparecer en el aviso y «Save» debe guardarlos
        // todos, no solo el activo.
        let names = ["a.png".to_owned(), "b.png".to_owned()];
        let text = unsaved_dialog_description(&names, "going back to the gallery");
        assert!(text.contains("2 canvases"), "{text}");
        assert!(text.contains("a.png") && text.contains("b.png"), "{text}");
        assert!(text.contains("saves them all"), "{text}");
    }
}
