//! Contenido de cada vista (`View::Welcome`/`Loading`/`Gallery`/`Editor`)
//! dentro del `CentralPanel` del frame. `editor_view_ui` es una función
//! libre, no un método de `App`: se llama mientras `state` sigue prestado de
//! `self.view` (dentro de `match &mut self.view { View::Editor(state) =>
//! ... }`), así que recibe cada campo de `App` que necesita por separado —
//! mismo patrón que ya usan `editor::canvas_ui`/`deck_strip::deck_strip_ui`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::time::Instant;

use canvas_render::CanvasRenderer;
use eframe::egui;
use eframe::egui_wgpu::RenderState;

use crate::loader::AppMsg;
use crate::surface::CanvasSurface;
use crate::{
    deck, deck_strip, editor, export, gallery, layers_panel, loader, menus, settings, watcher,
    welcome,
};

use super::persistence::{start_save, start_save_design};
use super::ui_modals::{export_flow_ui, overwrite_modal_ui, readonly_modal_ui};
use super::Nav;

/// Vista de bienvenida: accesos rápidos (nuevo diseño, abrir archivo/
/// carpeta, recientes) cuando no hay ningún proyecto abierto.
pub(super) fn welcome_view_ui(
    ui: &mut egui::Ui,
    error: Option<&str>,
    recent_files: &[PathBuf],
    last_page_size: (f64, f64),
    show_settings: &mut bool,
    tx: &Sender<AppMsg>,
    ctx: &egui::Context,
) -> Option<Nav> {
    let mut open_next = None;
    match welcome::show(ui, error, recent_files, last_page_size) {
        Some(welcome::WelcomeAction::NewProject) => {
            open_next = Some(Nav::NewDesign);
        }
        Some(welcome::WelcomeAction::OpenFile) => {
            loader::spawn_pick_file(tx.clone(), ctx.clone());
        }
        Some(welcome::WelcomeAction::OpenFolder) => {
            loader::spawn_pick_folder(tx.clone(), ctx.clone());
        }
        Some(welcome::WelcomeAction::OpenSettings) => {
            *show_settings = true;
        }
        Some(welcome::WelcomeAction::OpenRecent(path)) => {
            open_next = Some(Nav::Open(path));
        }
        None => {}
    }
    open_next
}

/// Vista de carga: solo un spinner mientras el archivo/diseño elegido
/// termina de abrirse en segundo plano.
pub(super) fn loading_view_ui(ui: &mut egui::Ui, path: &std::path::Path) {
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    egui::CentralPanel::default().show(ui, |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() * 0.4);
            ui.add(egui::Spinner::new().size(28.0));
            ui.add_space(8.0);
            ui.label(format!("Loading {name}…"));
        });
    });
}

/// Vista de galería: cuadrícula de una carpeta, con sus operaciones de
/// archivo (crear/duplicar/pegar/renombrar/borrar) siempre en un hilo aparte.
pub(super) fn gallery_view_ui(
    g: &mut gallery::GalleryState,
    ui: &mut egui::Ui,
    settings: &mut settings::AppSettings,
    pending_deck: &mut Option<deck::DeckSeed>,
    tx: &Sender<AppMsg>,
    ctx: &egui::Context,
) -> Option<Nav> {
    let mut open_next = None;
    match gallery::show(g, ui) {
        Some(gallery::GalleryAction::CycleFolderPanelSide) => {
            g.folder_panel_side = gallery::next_folder_panel_side(g.folder_panel_side);
            settings.gallery_folder_panel_side = g.folder_panel_side;
            settings.save_in_background();
        }
        Some(gallery::GalleryAction::Open(path)) => {
            // Se lleva las miniaturas ya cargadas al editor: si el archivo
            // resulta tener hermanos, la tira arranca sin parpadeo de ⏳
            // (`resolve_deck` la consume al terminar de cargar).
            *pending_deck = Some(deck::DeckSeed::from_gallery(g));
            open_next = Some(Nav::Open(path));
        }
        Some(gallery::GalleryAction::OpenFolder(folder)) => {
            let (path, navigation) = g.navigation_to_folder(folder);
            open_next = Some(Nav::OpenGallery { path, navigation });
        }
        Some(gallery::GalleryAction::Back) => {
            if let Some((path, navigation)) = g.navigation_back() {
                open_next = Some(Nav::OpenGallery { path, navigation });
            }
        }
        Some(gallery::GalleryAction::Forward) => {
            if let Some((path, navigation)) = g.navigation_forward() {
                open_next = Some(Nav::OpenGallery { path, navigation });
            }
        }
        Some(gallery::GalleryAction::SortChanged(sort)) => {
            settings.gallery_sort = sort;
            settings.save_in_background();
        }
        Some(gallery::GalleryAction::NewDesign) => {
            let seed = deck::DeckSeed::from_gallery(g);
            open_next = Some(Nav::NewDesignInFolder { seed });
        }
        Some(gallery::GalleryAction::Duplicate(path)) => {
            loader::spawn_gallery_op(
                loader::GalleryOp::Duplicate { path },
                false,
                tx.clone(),
                ctx.clone(),
            );
        }
        Some(gallery::GalleryAction::PasteHere(src)) => {
            loader::spawn_gallery_op(
                loader::GalleryOp::CopyInto {
                    src,
                    folder: g.folder.clone(),
                },
                false,
                tx.clone(),
                ctx.clone(),
            );
        }
        Some(gallery::GalleryAction::Rename(path, new_stem)) => {
            loader::spawn_gallery_op(
                loader::GalleryOp::Rename { path, new_stem },
                false,
                tx.clone(),
                ctx.clone(),
            );
        }
        Some(gallery::GalleryAction::RenameFolder(path, new_name)) => {
            loader::spawn_gallery_op(
                loader::GalleryOp::RenameFolder { path, new_name },
                false,
                tx.clone(),
                ctx.clone(),
            );
        }
        Some(gallery::GalleryAction::DeleteFolder(path)) => {
            loader::spawn_gallery_op(
                loader::GalleryOp::DeleteFolder { path },
                false,
                tx.clone(),
                ctx.clone(),
            );
        }
        Some(gallery::GalleryAction::CreateFolder(parent, name)) => {
            loader::spawn_gallery_op(
                loader::GalleryOp::CreateFolder { parent, name },
                false,
                tx.clone(),
                ctx.clone(),
            );
        }
        Some(gallery::GalleryAction::Delete(path)) => {
            loader::spawn_gallery_op(
                loader::GalleryOp::Delete { path },
                false,
                tx.clone(),
                ctx.clone(),
            );
        }
        None => {}
    }
    open_next
}

/// Vista de editor: baraja + panel de capas/propiedades + lienzo, y toda la
/// orquestación de guardado, exportación, navegación de la baraja y
/// deshacer/rehacer global de ese frame. `rs` ya viene resuelto por el
/// llamador (si `frame.wgpu_render_state()` fuera `None`, el llamador corta
/// el frame entero antes de entrar aquí, no solo esta vista).
#[allow(clippy::too_many_arguments)]
pub(super) fn editor_view_ui(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    rs: &RenderState,
    state: &mut editor::EditorState,
    paste_requested: bool,
    deck: &mut deck::Deck,
    renderer: &mut CanvasRenderer,
    surface: &mut Option<CanvasSurface>,
    tx: &Sender<AppMsg>,
    settings: &mut settings::AppSettings,
    show_settings: &mut bool,
    save_requested: &mut bool,
    close_after_save: &mut bool,
    after_save: &mut Option<Nav>,
    allow_close: &mut bool,
    overwrite_confirmed: &mut bool,
    overwrite_prompt: &mut Option<PathBuf>,
    overwrite_dont_ask: &mut bool,
    readonly_prompt: &mut Option<PathBuf>,
    export_dialog: &mut Option<export::ExportDialog>,
    pending_export_settings: &mut Option<export::ExportSettings>,
    pending_export: &mut Option<(PathBuf, export::ExportSettings)>,
    pending_save_as: &mut Option<PathBuf>,
    ignore_fs_events_until: &mut Option<Instant>,
    watcher: &mut Option<watcher::DocWatcher>,
    undoable_deletes: &mut HashMap<PathBuf, Option<PathBuf>>,
    materializing: &mut Option<u64>,
    materialize_blocked: &mut Option<u64>,
    save_all_queue: &mut Vec<u64>,
    save_all_attempted: &mut bool,
) -> (Option<Nav>, Option<menus::MenuAction>) {
    let mut open_next: Option<Nav> = None;
    // Acción del menú contextual del lienzo (clic derecho): se resuelve por
    // el llamador, una vez liberado el préstamo de `state`.
    let mut pending_menu_action: Option<menus::MenuAction> = None;

    // El deshacer/rehacer global (`push_undo_step`/`undo`/`redo` en
    // `editor.rs`) etiqueta cada paso con esta id: hay que tenerla al día
    // ANTES de `handle_shortcuts` (que puede disparar un Ctrl+Z ese mismo
    // frame) y de cualquier edición que ocurra más abajo en `canvas_ui`.
    // Barato de refrescar cada frame; más simple que perseguir cada sitio
    // donde `deck.active`/`self.view` pueden cambiar.
    state.active_slot_id = deck.slots.get(deck.active).map_or(0, |s| s.id);
    state.handle_shortcuts(ctx, paste_requested, deck.rename_edit.is_some());

    // Recarga pedida desde el banner de «cambió en disco».
    if std::mem::take(&mut state.reload_requested) {
        match state.doc.source_path.clone() {
            Some(path) => open_next = Some(Nav::Open(path)),
            None => state.external_change = false,
        }
    }

    // Volver a la galería (preguntando si hay cambios sin guardar).
    if state.return_requested {
        state.return_requested = false;
        if let Some(folder) = state.from_gallery.clone() {
            if !state.is_dirty() {
                open_next = Some(Nav::Open(folder));
            } else {
                let choice = rfd::MessageDialog::new()
                    .set_level(rfd::MessageLevel::Warning)
                    .set_title("Unsaved changes")
                    .set_description(format!(
                        "\"{}\" has unsaved changes.\nSave them before going back to the gallery? (\"No\" discards them.)",
                        state.file_name()
                    ))
                    .set_buttons(rfd::MessageButtons::YesNoCancelCustom(
                        "Save".to_owned(),
                        "Discard".to_owned(),
                        "Cancel".to_owned(),
                    ))
                    .show();
                // Igual que en confirm_close: en Windows el resultado llega
                // como Yes/No/Cancel, no Custom.
                match choice {
                    rfd::MessageDialogResult::Yes => {
                        *save_requested = true;
                        *after_save = Some(Nav::Open(folder));
                    }
                    rfd::MessageDialogResult::Custom(c) if c == "Save" => {
                        *save_requested = true;
                        *after_save = Some(Nav::Open(folder));
                    }
                    rfd::MessageDialogResult::No => {
                        open_next = Some(Nav::Open(folder));
                    }
                    rfd::MessageDialogResult::Custom(c) if c == "Discard" => {
                        open_next = Some(Nav::Open(folder));
                    }
                    _ => {}
                }
            }
        }
    }

    // Renombrar/borrar el archivo abierto (lápiz junto al nombre / botón
    // «Delete» del panel).
    if let Some(new_stem) = state.file_rename_requested.take() {
        if let Some(path) = state.doc.source_path.clone() {
            // Misma ventana de gracia que un guardado (`Saved` de éxito, más
            // abajo): se abre ANTES de lanzar la operación, no solo al
            // recibir la respuesta. Renombrar hace que el watcher (que
            // sigue mirando la ruta vieja) vea el archivo "desaparecer" —
            // un evento mucho más inmediato que el de un guardado en el
            // sitio — y sin esto el banner de «cambió por fuera» podía
            // saltar en la carrera entre ese evento y el mensaje
            // `DocumentRenamed` que actualiza `source_path`.
            *ignore_fs_events_until = Some(Instant::now() + std::time::Duration::from_secs(2));
            *watcher = None;
            loader::spawn_document_rename(path, new_stem, tx.clone(), ctx.clone());
        }
    }
    if std::mem::take(&mut state.delete_requested) {
        // Si viene de deshacer un `Create` (ver `pending_delete_from_undo`),
        // este borrado en concreto no debe poder deshacerse a su vez — se
        // consume aquí, ANTES de decidir si la ruta entra en
        // `undoable_deletes`.
        let from_undo = std::mem::take(&mut state.pending_delete_from_undo);
        let placeholder_id = deck
            .slots
            .get(deck.active)
            .filter(|slot| slot.is_placeholder)
            .map(|slot| slot.id);
        if let Some(id) = placeholder_id {
            deck.discard_placeholder(id, state);
        } else if let Some(path) = state.doc.source_path.clone() {
            *ignore_fs_events_until = Some(Instant::now() + std::time::Duration::from_secs(2));
            *watcher = None;
            if !from_undo {
                let sidecar = canvas_io::find_sidecar(&path);
                undoable_deletes.insert(path.clone(), sidecar);
            }
            loader::spawn_document_delete(path, tx.clone(), ctx.clone());
        }
    }
    // Ranura PROVISIONAL que se convierte en archivo de verdad en cuanto el
    // usuario la edita — sin diálogo. El usuario pidió «un lienzo nuevo», no
    // «guardar como»; preguntarle un nombre justo después de su primer
    // trazo rompería el flujo. Va DESPUÉS de `handle_messages` (una
    // respuesta de este mismo frame ya está aplicada) y ANTES del bloque de
    // guardado (el `save_clicked` que la respuesta deja preparado se
    // consume ese mismo frame, más abajo).
    // La extensión a reservar es la del nombre YA asomado en la ranura
    // (`push_placeholder` la fijó al crearla), no la del ajuste actual: si
    // el usuario cambió `new_canvas_format` mientras esta provisional
    // seguía sin editar, el nombre que se ve en la tira («1.png») y el que
    // se reserva de verdad deben seguir siendo el mismo.
    let placeholder = deck
        .slots
        .get(deck.active)
        .filter(|s| s.is_placeholder)
        .map(|s| {
            (
                s.id,
                s.path
                    .extension()
                    .map(|e| e.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            )
        });
    if let Some((id, ext)) = placeholder {
        let has_canvas_content = state.doc.page().is_ok_and(|page| !page.layers.is_empty());
        if has_canvas_content
            && state.is_dirty()
            && !state.saving
            && materializing.is_none()
            && *materialize_blocked != Some(id)
        {
            if let Some(folder) = deck.folder.clone() {
                *materializing = Some(id);
                loader::spawn_reserve_canvas_path(folder, id, ext, tx.clone(), ctx.clone());
            }
        }
    }

    // Guardar / Guardar como: botones del panel o atajos de teclado (el
    // orden importa: Ctrl+Shift+S primero).
    let save_as = std::mem::take(&mut state.save_as_clicked)
        || ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                egui::Key::S,
            ))
        });
    let mut save = *save_requested
        || std::mem::take(&mut state.save_clicked)
        || ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::COMMAND,
                egui::Key::S,
            ))
        });
    *save_requested = false;

    if save_as {
        if state.is_design {
            loader::spawn_pick_design_path(Some(state.file_name()), tx.clone(), ctx.clone());
        } else {
            loader::spawn_pick_save_path(Some(state.file_name()), tx.clone(), ctx.clone());
        }
        save = false;
    }
    if save {
        if !state.is_dirty() {
            // Un guardado sin cambios no reescribe nada: en JPEG,
            // recomprimir sin motivo costaría calidad. Si veníamos de un
            // diálogo de cerrar/volver, su flujo continúa.
            tracing::info!("documento sin cambios: no se reescribe el archivo");
            if *close_after_save {
                *allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else if let Some(nav) = after_save.take() {
                open_next = Some(nav);
            }
        } else if state.is_design {
            // Un diseño no se rasteriza: no hay nada destructivo que
            // avisar, ni SVG/GIF que redirigir, así que se saltan ambos
            // modales.
            match state.doc.source_path.clone() {
                Some(path) => start_save_design(
                    state,
                    renderer,
                    rs,
                    tx,
                    ctx,
                    path,
                    false,
                    ignore_fs_events_until,
                ),
                None => {
                    loader::spawn_pick_design_path(Some(state.file_name()), tx.clone(), ctx.clone())
                }
            }
        } else {
            match state.doc.source_path.clone() {
                // SVG/GIF: no se sobrescriben nunca; se explica y se
                // redirige a «Save as…».
                Some(path) if !canvas_io::can_overwrite(&path) => {
                    *readonly_prompt = Some(path);
                }
                Some(path) => {
                    // Aviso de sobrescritura destructiva: la primera vez de
                    // cada sesión (salvo que el usuario pidiera no volver a
                    // preguntar), y NUNCA para un lienzo `born_blank` — lo
                    // creó la propia app en blanco, no hay píxeles del
                    // usuario que este primer guardado pudiera destruir.
                    if !state.born_blank
                        && !settings.skip_overwrite_warning
                        && !*overwrite_confirmed
                    {
                        *overwrite_dont_ask = false;
                        *overwrite_prompt = Some(path);
                    } else {
                        start_save(
                            state,
                            renderer,
                            rs,
                            tx,
                            ctx,
                            path,
                            false,
                            settings.jpeg_quality,
                            ignore_fs_events_until,
                        );
                    }
                }
                // Sin origen en disco: cae a «Guardar como…».
                None => {
                    loader::spawn_pick_save_path(Some(state.file_name()), tx.clone(), ctx.clone())
                }
            }
        }
    }
    if let Some(path) = pending_save_as.take() {
        // La extensión final de la ruta elegida decide la rama, venga del
        // diálogo de diseño o del de imagen (ambos acaban en el mismo
        // `SaveAsPicked`).
        if canvas_io::is_canvas_file(&path) {
            start_save_design(
                state,
                renderer,
                rs,
                tx,
                ctx,
                path,
                true,
                ignore_fs_events_until,
            );
        } else {
            start_save(
                state,
                renderer,
                rs,
                tx,
                ctx,
                path,
                true,
                settings.jpeg_quality,
                ignore_fs_events_until,
            );
        }
    }

    // Modal de aviso de sobrescritura destructiva.
    overwrite_modal_ui(
        state,
        renderer,
        rs,
        tx,
        ctx,
        settings,
        overwrite_prompt,
        overwrite_confirmed,
        overwrite_dont_ask,
        close_after_save,
        after_save,
        ignore_fs_events_until,
    );

    // Modal para SVG/GIF: no se pueden sobrescribir, se explica por qué y
    // se ofrece «Save as…» en su lugar.
    readonly_modal_ui(
        state,
        tx,
        ctx,
        readonly_prompt,
        close_after_save,
        after_save,
    );

    // Diálogo de exportación.
    export_flow_ui(
        state,
        renderer,
        rs,
        tx,
        ctx,
        export_dialog,
        pending_export_settings,
        pending_export,
    );

    // Tira de lienzos de la baraja: solo con más de un archivo en la
    // carpeta de origen. Va antes que "layers" para quedar pegada al borde
    // exterior de la ventana.
    let mut strip_action = None;
    // Acción pedida desde la cabecera de un lienzo del área central
    // (renombrar/duplicar/borrar) — se llena dentro del `CentralPanel` de
    // más abajo, se resuelve junto a `strip_action`.
    let mut canvas_action = None;
    if deck.is_visible() {
        let active_dirty = state.is_dirty();
        // Ids DISTINTOS por lado (no el mismo panel reetiquetado): así el
        // tamaño recordado de la tira a la izquierda (ancho) no se aplica
        // como alto al moverla arriba, y viceversa — mismo criterio que ya
        // separa "layers" de "properties". `.resizable(true)` es
        // obligatorio en Top/Bottom (egui los crea con `resizable(false)`
        // por defecto) e inofensivo-pero-explícito en Left/Right. Orden
        // importa: `.default_size` ENSANCHA el rango si se llama después
        // de `.size_range`, así que va primero.
        match deck.strip_side {
            deck::StripSide::Left => {
                egui::Panel::left("deck_strip_left")
                    .default_size(120.0)
                    .size_range(96.0..=280.0)
                    .resizable(true)
                    .show(ui, |ui| {
                        strip_action = deck_strip::deck_strip_ui(deck, active_dirty, ui);
                    });
            }
            deck::StripSide::Right => {
                egui::Panel::right("deck_strip_right")
                    .default_size(120.0)
                    .size_range(96.0..=280.0)
                    .resizable(true)
                    .show(ui, |ui| {
                        strip_action = deck_strip::deck_strip_ui(deck, active_dirty, ui);
                    });
            }
            deck::StripSide::Top => {
                egui::Panel::top("deck_strip_top")
                    .default_size(140.0)
                    .size_range(120.0..=280.0)
                    .resizable(true)
                    .show(ui, |ui| {
                        strip_action = deck_strip::deck_strip_ui(deck, active_dirty, ui);
                    });
            }
            deck::StripSide::Bottom => {
                egui::Panel::bottom("deck_strip_bottom")
                    .default_size(140.0)
                    .size_range(120.0..=280.0)
                    .resizable(true)
                    .show(ui, |ui| {
                        strip_action = deck_strip::deck_strip_ui(deck, active_dirty, ui);
                    });
            }
        }
    }
    // Diseño bloqueado (`Slot::locked`, cabecera del lienzo en el área
    // central): deshabilita también los paneles, no solo los gestos sobre
    // el propio lienzo — "no se puede editar" sin matizar por qué vía.
    let locked = deck.slots.get(deck.active).is_some_and(|s| s.locked);
    egui::Panel::left("layers")
        .default_size(220.0)
        .show(ui, |ui| {
            ui.add_enabled_ui(!locked, |ui| layers_panel::layers_panel_ui(state, ui));
        });
    egui::Panel::right("properties")
        .default_size(260.0)
        .show(ui, |ui| {
            ui.add_enabled_ui(!locked, |ui| editor::properties_ui(state, ui));
        });
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE)
        .show(ui, |ui| {
            canvas_action = editor::canvas_ui(
                state,
                deck,
                ui,
                rs,
                renderer,
                surface,
                tx,
                settings.new_canvas_format.extension(),
                settings.sidecar_default,
            );
        });

    // Saltar a otro lienzo de la baraja: clic en el propio lienzo (ya deja
    // `self.deck.jump_to` listo, dentro de `canvas_ui`), tira lateral, o
    // teclado (PageUp/PageDown/Home/End). El intercambio es SIN PÉRDIDA —
    // el lienzo saliente queda guardado en su propia ranura con su
    // historial de deshacer intacto — así que, a diferencia de «Back to
    // gallery» (que sí sale del editor), no hace falta preguntar por
    // cambios sin guardar para saltar aquí dentro.
    let mut deck_target = state.deck_nav.take().and_then(|nav| match nav {
        editor::DeckNav::Next => deck.next_path(),
        editor::DeckNav::Prev => deck.prev_path(),
        editor::DeckNav::First => deck.first_path(),
        editor::DeckNav::Last => deck.last_path(),
    });
    match strip_action {
        Some(deck_strip::StripAction::Open(path)) => {
            deck_target = deck_target.or(Some(path));
        }
        // Inline, no `self.toggle_deck_axis()`: el borrow checker no ve que
        // ese método solo toca campos disjuntos de `state`.
        Some(deck_strip::StripAction::ToggleAxis) => {
            deck.axis = deck.axis.toggled();
            deck.layout_dirty = true;
            settings.deck_axis = deck.axis;
            settings.save_in_background();
        }
        Some(deck_strip::StripAction::CycleSide) => {
            deck.strip_side = deck.strip_side.cycled();
            settings.deck_strip_side = deck.strip_side;
            settings.save_in_background();
            // Sin `layout_dirty = true`: mover el panel no cambia la
            // geometría de la baraja, solo el rect del panel central — que
            // `Viewport::note_size` ya detecta y reajusta.
        }
        Some(deck_strip::StripAction::AddCanvas) => {
            if let Some(idx) = deck.push_placeholder(
                settings.last_page_size,
                settings.new_canvas_format.extension(),
            ) {
                deck.jump_to = Some(idx);
                deck.jump_center = true;
            }
        }
        None => {}
    }
    // Renombrar/duplicar/borrar desde la cabecera de un lienzo (activo o de
    // fondo) en el área central — mismas operaciones que ya existían para
    // la ranura activa (lápiz junto al nombre, botón «Delete» del panel) o
    // desde la galería (duplicar), generalizadas por id/ruta en vez de
    // asumir "la activa".
    match canvas_action {
        Some(editor::CanvasAction::Rename(id, new_stem)) => {
            let is_active = deck.slots.get(deck.active).map(|s| s.id) == Some(id);
            let is_placeholder = deck
                .find_by_id(id)
                .and_then(|index| deck.slots.get(index))
                .is_some_and(|slot| slot.is_placeholder);
            if is_placeholder {
                deck.discard_placeholder(id, state);
            } else if is_active {
                // Reutiliza el camino ya existente (lápiz junto al
                // nombre): se recoge y se lanza más arriba, en el próximo
                // frame.
                state.file_rename_requested = Some(new_stem);
            } else if let Some(path) = deck
                .find_by_id(id)
                .and_then(|i| deck.slots.get(i))
                .map(|s| s.path.clone())
            {
                *ignore_fs_events_until = Some(Instant::now() + std::time::Duration::from_secs(2));
                *watcher = None;
                loader::spawn_document_rename(path, new_stem, tx.clone(), ctx.clone());
            }
        }
        Some(editor::CanvasAction::Duplicate(id)) => {
            let source = deck
                .find_by_id(id)
                .and_then(|i| deck.slots.get(i))
                .map(|slot| (slot.is_placeholder, slot.page, slot.path.clone()));
            if let Some((true, page, path)) = source {
                let ext = path
                    .extension()
                    .and_then(|value| value.to_str())
                    .unwrap_or(settings.new_canvas_format.extension());
                deck.push_placeholder(page.unwrap_or(settings.last_page_size), ext);
            } else if let Some((false, _, path)) = source {
                loader::spawn_gallery_op(
                    loader::GalleryOp::Duplicate { path },
                    false,
                    tx.clone(),
                    ctx.clone(),
                );
            }
        }
        Some(editor::CanvasAction::Delete(id)) => {
            let is_active = deck.slots.get(deck.active).map(|s| s.id) == Some(id);
            let is_placeholder = deck
                .find_by_id(id)
                .and_then(|index| deck.slots.get(index))
                .is_some_and(|slot| slot.is_placeholder);
            if is_placeholder {
                deck.discard_placeholder(id, state);
            } else if is_active {
                // Reutiliza el camino ya existente (botón «Delete» del
                // panel de propiedades).
                state.delete_requested = true;
            } else if let Some(path) = deck
                .find_by_id(id)
                .and_then(|i| deck.slots.get(i))
                .map(|s| s.path.clone())
            {
                *ignore_fs_events_until = Some(Instant::now() + std::time::Duration::from_secs(2));
                *watcher = None;
                let sidecar = canvas_io::find_sidecar(&path);
                undoable_deletes.insert(path.clone(), sidecar);
                loader::spawn_document_delete(path, tx.clone(), ctx.clone());
            }
        }
        Some(editor::CanvasAction::ReplaceFromLocal(layer)) => {
            loader::spawn_pick_replacement_image(layer, None, tx.clone(), ctx.clone());
        }

        Some(editor::CanvasAction::ReplaceFromUrl(layer, url)) => {
            loader::spawn_load_replacement_image_from_url(layer, url, tx.clone(), ctx.clone());
        }
        Some(editor::CanvasAction::Menu(action)) => {
            pending_menu_action = Some(action);
        }
        None => {}
    }
    if let Some(target) = deck_target {
        if let Some(idx) = deck.find_by_path(&target) {
            deck.jump_to = Some(idx);
            // Por la tira o el teclado: el destino puede no estar a la
            // vista, así que sí hace falta recentrar (a diferencia de un
            // clic directo sobre el propio lienzo, que ya deja `jump_to`
            // sin esto).
            deck.jump_center = true;
        }
    } else if let Some(&next_id) = save_all_queue.first() {
        // «Save all»: sin una navegación más prioritaria este frame, salta
        // a la próxima ranura pendiente de la cola.
        if deck.slots.get(deck.active).map(|s| s.id) != Some(next_id) {
            match deck.find_by_id(next_id) {
                Some(idx) => {
                    deck.jump_to = Some(idx);
                    deck.jump_center = true;
                }
                // Desapareció (renombrada/borrada) mientras esperaba turno:
                // se salta sin más.
                None => {
                    save_all_queue.remove(0);
                }
            }
        }
    } else if let Some(id) = state
        .pending_global_undo
        .as_ref()
        .or(state.pending_global_redo.as_ref())
        .map(editor::GlobalStep::slot_id)
    {
        // Deshacer/rehacer global: el paso más reciente de toda la sesión
        // le tocaba a OTRO diseño de la baraja — salta a él para mostrarlo
        // (mismo patrón que «Save all», arriba). `request_loads`, más abajo
        // en `canvas_ui`, dispara la recarga de disco si esa ranura ya no
        // está `Ready` (fue descartada por presupuesto).
        match deck.find_by_id(id) {
            Some(idx) => {
                deck.jump_to = Some(idx);
                deck.jump_center = true;
            }
            // El diseño desapareció (archivo borrado) mientras esperaba
            // turno: se descarta ese paso, sin encadenar automáticamente
            // con el siguiente.
            None => {
                state.discard_pending_global_undo();
                state.discard_pending_global_redo();
            }
        }
    }
    // Aplica el salto si el destino ya está listo y el editor está ocioso;
    // si no, la petición queda pendiente y se reintenta en los próximos
    // frames — llamar aquí siempre, no solo cuando `deck_target` trae algo
    // nuevo, es lo que reintenta un salto que aún esperaba a que su carga
    // terminase. Recentra la vista SOLO si quien pidió el salto lo marcó
    // (`jump_center`): un clic directo sobre el propio lienzo ya se ve,
    // recentrar ahí sería mover la cámara sin que el usuario lo pidiera.
    //
    // NUNCA mientras haya un modal de guardado pendiente
    // (`overwrite_prompt`/`readonly_prompt`): `is_idle()` ya cubre
    // `saving`, pero esos modales aparecen ANTES de que `start_save` los
    // ponga a `true` — sin este freno, saltar en ese hueco dejaría el modal
    // hablando de un archivo mientras `state` pasa a ser otro documento, y
    // al confirmarlo se guardarían los píxeles del documento EQUIVOCADO en
    // la ruta del modal. Igual con `materializing`: la reserva de nombre de
    // una provisional tampoco pone `saving` a `true` todavía, y saltar a
    // mitad de esa reserva dejaría la respuesta actuando sobre el lienzo
    // equivocado.
    let save_modal_pending =
        overwrite_prompt.is_some() || readonly_prompt.is_some() || materializing.is_some();
    if !save_modal_pending && deck::apply_jump(deck, state) && std::mem::take(&mut deck.jump_center)
    {
        state.viewport.request_center(deck.active_rect());
    }
    // «Save all»: si la activa ya es la ranura que tocaba, dispara su
    // guardado — mismo camino que Ctrl+S, un frame más tarde (el bloque de
    // guardado de este frame ya corrió antes de que se dibujaran los
    // paneles).
    if let Some(&next_id) = save_all_queue.first() {
        if deck.slots.get(deck.active).map(|s| s.id) == Some(next_id) {
            // El aviso de sobrescritura (primer lienzo raster del lote) o
            // el redirect de SVG/GIF cuentan como "en curso", no como
            // fallo: sin este freno, el intento ya marcado se leería como
            // fallido mientras el usuario todavía no ha respondido al
            // modal.
            let waiting_on_modal = overwrite_prompt.is_some() || readonly_prompt.is_some();
            if !state.is_dirty() {
                // Ya se guardó (`AppMsg::Saved` la sacó de la cola) o nunca
                // hizo falta: nada que hacer aquí.
            } else if state.saving || waiting_on_modal {
                // En curso, o esperando la respuesta del usuario.
            } else if *save_all_attempted {
                // Se pulsó "Guardar", no hay guardado en curso ni modal
                // pendiente, y sigue sucia: ese intento falló de verdad (o
                // el usuario canceló el modal). Se aborta el lote en vez de
                // reintentar sin fin sobre el mismo lienzo.
                tracing::warn!(
                    "Save all: se detiene en un lienzo de fondo (guardado fallido o cancelado)"
                );
                save_all_queue.clear();
                *save_all_attempted = false;
            } else {
                *save_all_attempted = true;
                state.save_clicked = true;
            }
        }
    }
    // Deshacer/rehacer global: si la activa ya es la ranura que le tocaba a
    // la petición pendiente (el salto de arriba se aplicó, esta misma
    // vuelta o en una anterior), ejecuta el paso local ahora que es la
    // activa y limpia la petición.
    if state
        .pending_global_undo
        .as_ref()
        .is_some_and(|step| deck.slots.get(deck.active).map(|s| s.id) == Some(step.slot_id()))
    {
        state.finish_pending_global_undo();
    }
    if state
        .pending_global_redo
        .as_ref()
        .is_some_and(|step| deck.slots.get(deck.active).map(|s| s.id) == Some(step.slot_id()))
    {
        state.finish_pending_global_redo();
    }
    // Deshacer un borrado (`GlobalStep::Delete`): no pertenece a ninguna
    // ranura, así que `undo()` ya lo resolvió sin esperar ningún salto —
    // solo queda lanzar la restauración.
    if let Some(record) = state.pending_restore.take() {
        loader::spawn_restore_from_trash(record.path, record.sidecar, tx.clone(), ctx.clone());
    }

    if std::mem::take(&mut state.settings_clicked) {
        *show_settings = true;
    }
    // El checkbox del sidecar en el editor ES el valor por defecto
    // persistido: cambiarlo ahí lo recuerda para el futuro. En un diseño el
    // checkbox ni se muestra: no debe tocar el ajuste.
    if !state.is_design && state.sidecar_enabled != settings.sidecar_default {
        settings.sidecar_default = state.sidecar_enabled;
        settings.save_in_background();
    }

    (open_next, pending_menu_action)
}
