//! El tipo `App` (estado de la ventana eframe), sus vistas (`View`/`Nav`),
//! el constructor, y el bucle de `impl eframe::App::ui` que ensambla el
//! frame entero: menús, la baraja/lienzo del editor o la galería/bienvenida,
//! y los modales (guardar, sobrescribir, cerrar).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};

use anyhow::{Context, Result};
use canvas_render::CanvasRenderer;
use canvas_shell::ShellIntegration as _;
use eframe::egui;

use crate::loader::AppMsg;
use crate::surface::CanvasSurface;
use crate::{
    deck, deck_strip, editor, export, gallery, layers_panel, loader, menus, paste_hook, settings,
    watcher, welcome,
};

mod menu_actions;
mod messages;
mod navigation;
mod persistence;
mod window;

use persistence::{is_jpeg_path, start_export, start_save, start_save_design};

enum View {
    Welcome { error: Option<String> },
    Loading { path: PathBuf },
    Gallery(gallery::GalleryState),
    Editor(Box<editor::EditorState>),
}

/// Navegación diferida: qué hacer cuando termine el guardado en curso o al
/// final del frame (para no pelear con el préstamo de `self.view`).
#[derive(Clone)]
enum Nav {
    Open(PathBuf),
    OpenGallery {
        path: PathBuf,
        navigation: gallery::FolderNavigation,
    },
    CloseProject,
    NewDesign,
}

pub(crate) struct App {
    renderer: CanvasRenderer,
    surface: Option<CanvasSurface>,
    view: View,
    tx: Sender<AppMsg>,
    rx: Receiver<AppMsg>,
    last_title: String,
    /// «Guardar como…» elegido, pendiente de hornear (necesita la GPU).
    pending_save_as: Option<PathBuf>,
    /// Guardar solicitado desde el diálogo de cierre.
    save_requested: bool,
    /// Cerrar la ventana en cuanto termine el guardado en curso.
    close_after_save: bool,
    /// El usuario ya confirmó el cierre: no volver a preguntar.
    allow_close: bool,
    /// Directorio de caché de miniaturas (si se pudo crear).
    thumb_cache: Option<PathBuf>,
    /// Baraja de lienzos del editor abierto: todos los archivos de su
    /// carpeta de origen, para saltar entre ellos sin volver a la galería.
    /// Sobrevive al cambio de vista (a diferencia de `GalleryState`, que se
    /// destruye), así que también sirve para sembrar la rejilla al volver.
    deck: deck::Deck,
    /// Semilla capturada de la galería justo antes de navegar a un lienzo
    /// suyo; `resolve_deck` la consume en cuanto la carga termina.
    pending_deck: Option<deck::DeckSeed>,
    /// Navegación pendiente para cuando termine el guardado en curso.
    after_save: Option<Nav>,
    /// Ajustes persistidos del usuario.
    settings: settings::AppSettings,
    /// El usuario ya confirmó la sobrescritura destructiva en esta sesión.
    overwrite_confirmed: bool,
    /// Sobrescritura pendiente de confirmar en el modal (ruta del original).
    overwrite_prompt: Option<PathBuf>,
    /// El original no admite sobrescritura (SVG/GIF): modal que redirige a
    /// «Save as…».
    readonly_prompt: Option<PathBuf>,
    /// Estado del checkbox «Don't ask again» mientras el modal está abierto.
    overwrite_dont_ask: bool,
    /// Ventana de ajustes visible.
    show_settings: bool,
    /// Resultado del último registro/desregistro del Explorador.
    shell_status: String,
    /// Menús nativos (muda en Windows); `None` si no se pudieron instalar.
    menus: Option<menus::AppMenus>,
    /// Ventana «About» visible.
    show_about: bool,
    /// Último tema aplicado a egui (para no reaplicar cada frame).
    applied_theme: Option<settings::ThemeChoice>,
    /// Último estado «hay editor abierto» comunicado al menú.
    menus_editor_open: bool,
    /// Último estado de `History::can_undo`/`can_redo` comunicado al menú.
    menus_can_undo: bool,
    menus_can_redo: bool,
    /// Watcher `notify` del archivo abierto en el editor, si lo hay.
    watcher: Option<watcher::DocWatcher>,
    /// Ventana de gracia tras un guardado propio: los eventos del watcher
    /// hasta este instante son nuestros y se descartan.
    ignore_fs_events_until: Option<std::time::Instant>,
    /// Diálogo de exportación visible.
    export_dialog: Option<export::ExportDialog>,
    /// Ajustes ya elegidos en el diálogo, pendientes de la ruta de archivo.
    pending_export_settings: Option<export::ExportSettings>,
    /// Ruta y ajustes de exportación, pendiente de hornear (necesita la GPU).
    pending_export: Option<(PathBuf, export::ExportSettings)>,
    /// «Save all»: ids (estables) de las ranuras sucias que faltan por
    /// guardar, el activo excluido — ese se guarda aparte, sin saltar,
    /// nada más pulsar. Se procesa una por frame: salta a ella (si no es ya
    /// la activa) y pulsa "Guardar" por su cuenta, reutilizando EXACTAMENTE
    /// el camino de Ctrl+S (mismo aviso de sobrescritura, que gracias a
    /// `overwrite_confirmed` solo pregunta una vez por lote).
    save_all_queue: Vec<u64>,
    /// Ya se pulsó "Guardar" para la ranura al frente de `save_all_queue`.
    /// Si sigue sucia y no hay un guardado en curso la próxima vez que se
    /// mira, ese intento falló — se usa para abortar el lote en vez de
    /// reintentar sin fin (no se puede usar `save_error` para esto: es un
    /// campo de propósito general, podría traer un error de otra cosa).
    save_all_attempted: bool,
    /// Id de la ranura provisional cuya reserva de nombre está en vuelo.
    /// Cerrojo de un solo disparo: la detección de «el usuario la ha
    /// editado» se cumple en TODOS los frames a partir del primero, y sin
    /// esto lanzaría un hilo de reserva por frame.
    materializing: Option<u64>,
    /// Id de una ranura provisional cuya reserva FALLÓ (carpeta de solo
    /// lectura, disco lleno). Sigue estando sucia, así que la detección
    /// volvería a cumplirse cada frame: se abandona en vez de reintentar sin
    /// fin, mismo criterio que `save_all_attempted` con un lote fallido.
    materialize_blocked: Option<u64>,
    /// Rutas cuyo borrado en curso (`spawn_document_delete`) fue pedido
    /// directamente por el usuario (botón «Delete», o «Delete» desde la
    /// cabecera de un lienzo de fondo) — NO el borrado que ya ocurre como
    /// consecuencia de deshacer una creación (`pending_delete_from_undo`).
    /// El valor es el sidecar que tenía, si tenía uno — hay que anotarlo
    /// ANTES de borrar, porque una vez borrado ya no queda en disco para
    /// que `canvas_io::find_sidecar` lo encuentre. `AppMsg::DocumentDeleted`
    /// la consulta para decidir si ese borrado se apila como
    /// `GlobalStep::Delete` (deshacible) o no.
    undoable_deletes: HashMap<PathBuf, Option<PathBuf>>,
}

impl App {
    pub(crate) fn new(
        cc: &eframe::CreationContext<'_>,
        initial_path: Option<PathBuf>,
        instance: Option<canvas_shell::InstanceListener>,
    ) -> Result<Self> {
        let rs = cc
            .wgpu_render_state
            .as_ref()
            .context("eframe no ha inicializado wgpu (¿backend glow activo?)")?;
        let renderer = CanvasRenderer::new(&rs.device)?;
        let (tx, rx) = channel();

        // Rutas de segundas instancias: un hilo acepta conexiones del socket
        // local y las convierte en mensajes para la UI.
        if let Some(listener) = instance {
            let tx = tx.clone();
            let ctx = cc.egui_ctx.clone();
            listener.spawn_accept_loop(move |line| {
                let line = line.trim().to_owned();
                if line.is_empty() {
                    let _ = tx.send(AppMsg::FocusWindow);
                } else {
                    let _ = tx.send(AppMsg::OpenPathExternal(PathBuf::from(line)));
                }
                ctx.request_repaint();
            });
        }

        // Menú nativo (Windows): necesita el HWND de la ventana recién creada.
        #[cfg_attr(not(windows), allow(unused_mut))]
        let mut native_menus = None;
        #[cfg(windows)]
        {
            use raw_window_handle::{HasWindowHandle, RawWindowHandle};
            if let Ok(handle) = cc.window_handle() {
                if let RawWindowHandle::Win32(h) = handle.as_raw() {
                    native_menus = menus::AppMenus::install(h.hwnd.get());
                }
            }
        }

        let mut app = Self {
            renderer,
            surface: None,
            view: View::Welcome { error: None },
            tx,
            rx,
            last_title: String::new(),
            pending_save_as: None,
            save_requested: false,
            close_after_save: false,
            allow_close: false,
            thumb_cache: window::thumbnail_cache_dir(),
            deck: deck::Deck::default(),
            pending_deck: None,
            after_save: None,
            settings: settings::AppSettings::load(),
            overwrite_confirmed: false,
            overwrite_prompt: None,
            readonly_prompt: None,
            overwrite_dont_ask: false,
            show_settings: false,
            shell_status: String::new(),
            menus: native_menus,
            show_about: false,
            applied_theme: None,
            menus_editor_open: false,
            menus_can_undo: false,
            menus_can_redo: false,
            watcher: None,
            ignore_fs_events_until: None,
            export_dialog: None,
            pending_export_settings: None,
            pending_export: None,
            save_all_queue: Vec::new(),
            save_all_attempted: false,
            materializing: None,
            materialize_blocked: None,
            undoable_deletes: HashMap::new(),
        };
        if let Some(m) = app.menus.as_mut() {
            m.set_recents(&app.settings.recent_files);
        }
        if let Some(path) = initial_path {
            app.open_path(path, &cc.egui_ctx);
        }
        Ok(app)
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        // Se consume siempre, en cualquier vista, para que no quede pegado
        // de un frame a otro si no había ningún editor abierto para leerlo.
        let paste_requested = paste_hook::take_request();

        // Tema (System/Light/Dark) según los ajustes; solo al cambiar.
        if self.applied_theme != Some(self.settings.theme) {
            ctx.set_theme(self.settings.theme.to_egui());
            self.applied_theme = Some(self.settings.theme);
        }

        // Menú nativo: sondear clics y sincronizar los ítems de editor.
        while let Some(action) = self.menus.as_ref().and_then(|m| m.poll()) {
            self.handle_menu_action(action, &ctx);
        }
        let editor_open = matches!(self.view, View::Editor(_));
        if editor_open != self.menus_editor_open {
            self.menus_editor_open = editor_open;
            if let Some(m) = self.menus.as_mut() {
                m.set_editor_enabled(editor_open);
            }
        }
        // Estado real del historial del editor activo (`false` en cualquier
        // otra vista): sincroniza los ítems Undo/Redo del menú, tanto el
        // nativo como el de respaldo — mismo criterio de "solo llamar al
        // menú nativo cuando cambia" que `menus_editor_open` de arriba.
        let (can_undo, can_redo) = match &self.view {
            View::Editor(state) => (state.can_undo(), state.can_redo()),
            _ => (false, false),
        };
        if (can_undo, can_redo) != (self.menus_can_undo, self.menus_can_redo) {
            self.menus_can_undo = can_undo;
            self.menus_can_redo = can_redo;
            if let Some(m) = self.menus.as_mut() {
                m.set_undo_redo(can_undo, can_redo);
            }
        }

        // Fallback sin menú nativo (macOS/Linux): barra de menús egui.
        #[cfg(not(windows))]
        {
            let recents = self.settings.recent_files.clone();
            let action = egui::Panel::top("menu_bar")
                .show(ui, |ui| {
                    menus::menu_bar_ui(ui, editor_open, can_undo, can_redo, &recents)
                })
                .inner;
            if let Some(action) = action {
                self.handle_menu_action(action, &ctx);
            }
        }

        self.handle_messages(&ctx);
        self.handle_dropped_files(&ctx);
        self.confirm_close(&ctx);

        // Navegación diferida (clic en galería, volver desde el editor).
        let mut open_next: Option<Nav> = None;
        // Acción del menú contextual del lienzo (clic derecho): `state`
        // pide prestado `self.view` mutable durante toda la rama
        // `View::Editor` de más abajo, así que `self.handle_menu_action`
        // (que necesita `&mut self` entero) no se puede llamar ahí dentro
        // — se captura aquí y se resuelve DESPUÉS de este `match`, una vez
        // liberado el préstamo.
        let mut pending_menu_action: Option<menus::MenuAction> = None;

        match &mut self.view {
            View::Welcome { error } => {
                let error = error.clone();
                match welcome::show(
                    ui,
                    error.as_deref(),
                    &self.settings.recent_files,
                    self.settings.last_page_size,
                ) {
                    Some(welcome::WelcomeAction::NewProject) => {
                        open_next = Some(Nav::NewDesign);
                    }
                    Some(welcome::WelcomeAction::OpenFile) => {
                        loader::spawn_pick_file(self.tx.clone(), ctx.clone());
                    }
                    Some(welcome::WelcomeAction::OpenFolder) => {
                        loader::spawn_pick_folder(self.tx.clone(), ctx.clone());
                    }
                    Some(welcome::WelcomeAction::OpenSettings) => {
                        self.show_settings = true;
                    }
                    Some(welcome::WelcomeAction::OpenRecent(path)) => {
                        open_next = Some(Nav::Open(path));
                    }
                    None => {}
                }
            }
            View::Loading { path } => {
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
            View::Gallery(g) => match gallery::show(g, ui) {
                Some(gallery::GalleryAction::CycleFolderPanelSide) => {
                    g.folder_panel_side = gallery::next_folder_panel_side(g.folder_panel_side);
                    self.settings.gallery_folder_panel_side = g.folder_panel_side;
                    self.settings.save_in_background();
                }
                Some(gallery::GalleryAction::Open(path)) => {
                    // Se lleva las miniaturas ya cargadas al editor: si el
                    // archivo resulta tener hermanos, la tira arranca sin
                    // parpadeo de ⏳ (`resolve_deck` la consume al terminar
                    // de cargar).
                    self.pending_deck = Some(deck::DeckSeed::from_gallery(g));
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
                    self.settings.gallery_sort = sort;
                    self.settings.save_in_background();
                }
                Some(gallery::GalleryAction::NewDesign) => {
                    loader::spawn_gallery_op(
                        loader::GalleryOp::NewDesign {
                            folder: g.folder.clone(),
                            page: self.settings.last_page_size,
                            ext: self.settings.new_canvas_format.extension().to_owned(),
                            jpeg_quality: self.settings.jpeg_quality,
                        },
                        true,
                        self.tx.clone(),
                        ctx.clone(),
                    );
                }
                Some(gallery::GalleryAction::Duplicate(path)) => {
                    loader::spawn_gallery_op(
                        loader::GalleryOp::Duplicate { path },
                        false,
                        self.tx.clone(),
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
                        self.tx.clone(),
                        ctx.clone(),
                    );
                }
                Some(gallery::GalleryAction::Rename(path, new_stem)) => {
                    loader::spawn_gallery_op(
                        loader::GalleryOp::Rename { path, new_stem },
                        false,
                        self.tx.clone(),
                        ctx.clone(),
                    );
                }
                Some(gallery::GalleryAction::Delete(path)) => {
                    loader::spawn_gallery_op(
                        loader::GalleryOp::Delete { path },
                        false,
                        self.tx.clone(),
                        ctx.clone(),
                    );
                }
                None => {}
            },
            View::Editor(state) => {
                let Some(rs) = frame.wgpu_render_state().cloned() else {
                    return;
                };
                // El deshacer/rehacer global (`push_undo_step`/`undo`/`redo`
                // en `editor.rs`) etiqueta cada paso con esta id: hay que
                // tenerla al día ANTES de `handle_shortcuts` (que puede
                // disparar un Ctrl+Z ese mismo frame) y de cualquier edición
                // que ocurra más abajo en `canvas_ui`. Barato de refrescar
                // cada frame; más simple que perseguir cada sitio donde
                // `deck.active`/`self.view` pueden cambiar.
                state.active_slot_id = self.deck.slots.get(self.deck.active).map_or(0, |s| s.id);
                state.handle_shortcuts(&ctx, paste_requested, self.deck.rename_edit.is_some());

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
                            // Igual que en confirm_close: en Windows el
                            // resultado llega como Yes/No/Cancel, no Custom.
                            match choice {
                                rfd::MessageDialogResult::Yes => {
                                    self.save_requested = true;
                                    self.after_save = Some(Nav::Open(folder));
                                }
                                rfd::MessageDialogResult::Custom(c) if c == "Save" => {
                                    self.save_requested = true;
                                    self.after_save = Some(Nav::Open(folder));
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

                // Renombrar/borrar el archivo abierto (lápiz junto al
                // nombre / botón «Delete» del panel).
                if let Some(new_stem) = state.file_rename_requested.take() {
                    if let Some(path) = state.doc.source_path.clone() {
                        // Misma ventana de gracia que un guardado
                        // (`Saved` de éxito, más abajo): se abre ANTES de
                        // lanzar la operación, no solo al recibir la
                        // respuesta. Renombrar hace que el watcher (que
                        // sigue mirando la ruta vieja) vea el archivo
                        // "desaparecer" — un evento mucho más inmediato
                        // que el de un guardado en el sitio — y sin esto
                        // el banner de «cambió por fuera» podía saltar en
                        // la carrera entre ese evento y el mensaje
                        // `DocumentRenamed` que actualiza `source_path`.
                        self.ignore_fs_events_until =
                            Some(std::time::Instant::now() + std::time::Duration::from_secs(2));
                        self.watcher = None;
                        loader::spawn_document_rename(path, new_stem, self.tx.clone(), ctx.clone());
                    }
                }
                if std::mem::take(&mut state.delete_requested) {
                    // Si viene de deshacer un `Create` (ver
                    // `pending_delete_from_undo`), este borrado en concreto
                    // no debe poder deshacerse a su vez — se consume aquí,
                    // ANTES de decidir si la ruta entra en
                    // `undoable_deletes`.
                    let from_undo = std::mem::take(&mut state.pending_delete_from_undo);
                    let placeholder_id = self
                        .deck
                        .slots
                        .get(self.deck.active)
                        .filter(|slot| slot.is_placeholder)
                        .map(|slot| slot.id);
                    if let Some(id) = placeholder_id {
                        self.deck.discard_placeholder(id, state);
                    } else if let Some(path) = state.doc.source_path.clone() {
                        self.ignore_fs_events_until =
                            Some(std::time::Instant::now() + std::time::Duration::from_secs(2));
                        self.watcher = None;
                        if !from_undo {
                            let sidecar = canvas_io::find_sidecar(&path);
                            self.undoable_deletes.insert(path.clone(), sidecar);
                        }
                        loader::spawn_document_delete(path, self.tx.clone(), ctx.clone());
                    }
                }
                // Ranura PROVISIONAL que se convierte en archivo de verdad
                // en cuanto el usuario la edita — sin diálogo. El usuario
                // pidió «un lienzo nuevo», no «guardar como»; preguntarle un
                // nombre justo después de su primer trazo rompería el
                // flujo. Va DESPUÉS de `handle_messages` (una respuesta de
                // este mismo frame ya está aplicada) y ANTES del bloque de
                // guardado (el `save_clicked` que la respuesta deja
                // preparado se consume ese mismo frame, más abajo).
                // La extensión a reservar es la del nombre YA asomado en la
                // ranura (`push_placeholder` la fijó al crearla), no la del
                // ajuste actual: si el usuario cambió `new_canvas_format`
                // mientras esta provisional seguía sin editar, el nombre que
                // se ve en la tira («1.png») y el que se reserva de verdad
                // deben seguir siendo el mismo.
                let placeholder = self
                    .deck
                    .slots
                    .get(self.deck.active)
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
                    let has_canvas_content =
                        state.doc.page().is_ok_and(|page| !page.layers.is_empty());
                    if has_canvas_content
                        && state.is_dirty()
                        && !state.saving
                        && self.materializing.is_none()
                        && self.materialize_blocked != Some(id)
                    {
                        if let Some(folder) = self.deck.folder.clone() {
                            self.materializing = Some(id);
                            loader::spawn_reserve_canvas_path(
                                folder,
                                id,
                                ext,
                                self.tx.clone(),
                                ctx.clone(),
                            );
                        }
                    }
                }

                // Guardar / Guardar como: botones del panel o atajos de
                // teclado (el orden importa: Ctrl+Shift+S primero).
                let save_as = std::mem::take(&mut state.save_as_clicked)
                    || ctx.input_mut(|i| {
                        i.consume_shortcut(&egui::KeyboardShortcut::new(
                            egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                            egui::Key::S,
                        ))
                    });
                let mut save = self.save_requested
                    || std::mem::take(&mut state.save_clicked)
                    || ctx.input_mut(|i| {
                        i.consume_shortcut(&egui::KeyboardShortcut::new(
                            egui::Modifiers::COMMAND,
                            egui::Key::S,
                        ))
                    });
                self.save_requested = false;

                if save_as {
                    if state.is_design {
                        loader::spawn_pick_design_path(
                            Some(state.file_name()),
                            self.tx.clone(),
                            ctx.clone(),
                        );
                    } else {
                        loader::spawn_pick_save_path(
                            Some(state.file_name()),
                            self.tx.clone(),
                            ctx.clone(),
                        );
                    }
                    save = false;
                }
                if save {
                    if !state.is_dirty() {
                        // Un guardado sin cambios no reescribe nada: en JPEG,
                        // recomprimir sin motivo costaría calidad. Si veníamos
                        // de un diálogo de cerrar/volver, su flujo continúa.
                        tracing::info!("documento sin cambios: no se reescribe el archivo");
                        if self.close_after_save {
                            self.allow_close = true;
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        } else if let Some(nav) = self.after_save.take() {
                            open_next = Some(nav);
                        }
                    } else if state.is_design {
                        // Un diseño no se rasteriza: no hay nada destructivo
                        // que avisar, ni SVG/GIF que redirigir, así que se
                        // saltan ambos modales.
                        match state.doc.source_path.clone() {
                            Some(path) => start_save_design(
                                state,
                                &mut self.renderer,
                                &rs,
                                &self.tx,
                                &ctx,
                                path,
                                false,
                                &mut self.ignore_fs_events_until,
                            ),
                            None => loader::spawn_pick_design_path(
                                Some(state.file_name()),
                                self.tx.clone(),
                                ctx.clone(),
                            ),
                        }
                    } else {
                        match state.doc.source_path.clone() {
                            // SVG/GIF: no se sobrescriben nunca; se explica y
                            // se redirige a «Save as…».
                            Some(path) if !canvas_io::can_overwrite(&path) => {
                                self.readonly_prompt = Some(path);
                            }
                            Some(path) => {
                                // Aviso de sobrescritura destructiva: la
                                // primera vez de cada sesión (salvo que el
                                // usuario pidiera no volver a preguntar), y
                                // NUNCA para un lienzo `born_blank` — lo creó
                                // la propia app en blanco, no hay píxeles del
                                // usuario que este primer guardado pudiera
                                // destruir.
                                if !state.born_blank
                                    && !self.settings.skip_overwrite_warning
                                    && !self.overwrite_confirmed
                                {
                                    self.overwrite_dont_ask = false;
                                    self.overwrite_prompt = Some(path);
                                } else {
                                    start_save(
                                        state,
                                        &mut self.renderer,
                                        &rs,
                                        &self.tx,
                                        &ctx,
                                        path,
                                        false,
                                        self.settings.jpeg_quality,
                                        &mut self.ignore_fs_events_until,
                                    );
                                }
                            }
                            // Sin origen en disco: cae a «Guardar como…».
                            None => loader::spawn_pick_save_path(
                                Some(state.file_name()),
                                self.tx.clone(),
                                ctx.clone(),
                            ),
                        }
                    }
                }
                if let Some(path) = self.pending_save_as.take() {
                    // La extensión final de la ruta elegida decide la rama,
                    // venga del diálogo de diseño o del de imagen (ambos
                    // acaban en el mismo `SaveAsPicked`).
                    if canvas_io::is_canvas_file(&path) {
                        start_save_design(
                            state,
                            &mut self.renderer,
                            &rs,
                            &self.tx,
                            &ctx,
                            path,
                            true,
                            &mut self.ignore_fs_events_until,
                        );
                    } else {
                        start_save(
                            state,
                            &mut self.renderer,
                            &rs,
                            &self.tx,
                            &ctx,
                            path,
                            true,
                            self.settings.jpeg_quality,
                            &mut self.ignore_fs_events_until,
                        );
                    }
                }

                // Modal de aviso de sobrescritura destructiva.
                if let Some(path) = self.overwrite_prompt.clone() {
                    enum Choice {
                        None,
                        Overwrite,
                        SaveAs,
                        Cancel,
                    }
                    let mut choice = Choice::None;
                    let file_name = path
                        .file_name()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_else(|| path.display().to_string());
                    let jpeg_quality = self.settings.jpeg_quality;
                    let modal =
                        egui::Modal::new(egui::Id::new("overwrite_warning")).show(&ctx, |ui| {
                            ui.set_max_width(400.0);
                            ui.heading("Overwrite the original file?");
                            ui.add_space(6.0);
                            ui.label(format!(
                                "Saving will permanently replace \"{file_name}\" on disk \
                                 with the edited result. This cannot be undone."
                            ));
                            if is_jpeg_path(&path) {
                                ui.label(format!(
                                    "The JPEG will be re-encoded at quality {jpeg_quality}."
                                ));
                            }
                            ui.add_space(8.0);
                            ui.checkbox(&mut self.overwrite_dont_ask, "Don't ask again");
                            ui.add_space(10.0);
                            ui.horizontal(|ui| {
                                if ui.button("Overwrite").clicked() {
                                    choice = Choice::Overwrite;
                                }
                                if ui.button("Save as… instead").clicked() {
                                    choice = Choice::SaveAs;
                                }
                                if ui.button("Cancel").clicked() {
                                    choice = Choice::Cancel;
                                }
                            });
                        });
                    // Clic fuera o Esc equivalen a cancelar.
                    if modal.should_close() && matches!(choice, Choice::None) {
                        choice = Choice::Cancel;
                    }
                    match choice {
                        Choice::None => {}
                        Choice::Overwrite => {
                            self.overwrite_prompt = None;
                            self.overwrite_confirmed = true;
                            if self.overwrite_dont_ask && !self.settings.skip_overwrite_warning {
                                self.settings.skip_overwrite_warning = true;
                                self.settings.save_in_background();
                            }
                            start_save(
                                state,
                                &mut self.renderer,
                                &rs,
                                &self.tx,
                                &ctx,
                                path,
                                false,
                                self.settings.jpeg_quality,
                                &mut self.ignore_fs_events_until,
                            );
                        }
                        Choice::SaveAs => {
                            self.overwrite_prompt = None;
                            if self.overwrite_dont_ask && !self.settings.skip_overwrite_warning {
                                self.settings.skip_overwrite_warning = true;
                                self.settings.save_in_background();
                            }
                            loader::spawn_pick_save_path(
                                Some(state.file_name()),
                                self.tx.clone(),
                                ctx.clone(),
                            );
                        }
                        Choice::Cancel => {
                            self.overwrite_prompt = None;
                            self.close_after_save = false;
                            self.after_save = None;
                        }
                    }
                }

                // Modal para SVG/GIF: no se pueden sobrescribir, se explica
                // por qué y se ofrece «Save as…» en su lugar.
                if let Some(path) = self.readonly_prompt.clone() {
                    let mut save_as_instead = false;
                    let mut cancel = false;
                    let file_name = path
                        .file_name()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_else(|| path.display().to_string());
                    let is_svg = path
                        .extension()
                        .and_then(|e| e.to_str())
                        .is_some_and(|e| e.eq_ignore_ascii_case("svg"));
                    let modal =
                        egui::Modal::new(egui::Id::new("readonly_source")).show(&ctx, |ui| {
                            ui.set_max_width(400.0);
                            ui.heading("This file can't be overwritten");
                            ui.add_space(6.0);
                            if is_svg {
                                ui.label(format!(
                                    "\"{file_name}\" is a vector SVG. Canvas Desktop edits \
                                     raster pixels and can't rewrite vector artwork, so the \
                                     original stays untouched."
                                ));
                            } else {
                                ui.label(format!(
                                    "\"{file_name}\" is a GIF, which may be animated. \
                                     Overwriting it would flatten the animation to a single \
                                     frame, so the original stays untouched."
                                ));
                            }
                            ui.label("Use \"Save as…\" to save the result as a new file.");
                            ui.add_space(10.0);
                            ui.horizontal(|ui| {
                                if ui.button("Save as…").clicked() {
                                    save_as_instead = true;
                                }
                                if ui.button("Cancel").clicked() {
                                    cancel = true;
                                }
                            });
                        });
                    if modal.should_close() && !save_as_instead {
                        cancel = true;
                    }
                    if save_as_instead {
                        self.readonly_prompt = None;
                        loader::spawn_pick_save_path(
                            Some(state.file_name()),
                            self.tx.clone(),
                            ctx.clone(),
                        );
                    } else if cancel {
                        self.readonly_prompt = None;
                        self.close_after_save = false;
                        self.after_save = None;
                    }
                }

                // Diálogo de exportación.
                if let Some(dialog) = &mut self.export_dialog {
                    let page_size = state
                        .doc
                        .page()
                        .map(|p| (p.width, p.height))
                        .unwrap_or((0.0, 0.0));
                    match export::export_modal(dialog, &ctx, page_size) {
                        export::ExportChoice::None => {}
                        export::ExportChoice::Cancel => {
                            self.export_dialog = None;
                        }
                        export::ExportChoice::Pick(settings) => {
                            self.export_dialog = None;
                            let stem = state
                                .doc
                                .source_path
                                .as_deref()
                                .and_then(|p| p.file_stem())
                                .map(|s| s.to_string_lossy().into_owned())
                                .unwrap_or_else(|| "Untitled".to_owned());
                            let suggested = format!("{stem}.{}", settings.format.extension());
                            loader::spawn_pick_export_path(
                                suggested,
                                settings.format,
                                self.tx.clone(),
                                ctx.clone(),
                            );
                            self.pending_export_settings = Some(settings);
                        }
                    }
                }
                if let Some((path, settings)) = self.pending_export.take() {
                    start_export(
                        state,
                        &mut self.renderer,
                        &rs,
                        &self.tx,
                        &ctx,
                        path,
                        settings,
                    );
                }

                // Tira de lienzos de la baraja: solo con más de un archivo en
                // la carpeta de origen. Va antes que "layers" para quedar
                // pegada al borde exterior de la ventana.
                let mut strip_action = None;
                // Acción pedida desde la cabecera de un lienzo del área
                // central (renombrar/duplicar/borrar) — se llena dentro del
                // `CentralPanel` de más abajo, se resuelve junto a
                // `strip_action`.
                let mut canvas_action = None;
                if self.deck.is_visible() {
                    let active_dirty = state.is_dirty();
                    // Ids DISTINTOS por lado (no el mismo panel reetiquetado):
                    // así el tamaño recordado de la tira a la izquierda
                    // (ancho) no se aplica como alto al moverla arriba, y
                    // viceversa — mismo criterio que ya separa "layers" de
                    // "properties". `.resizable(true)` es obligatorio en
                    // Top/Bottom (egui los crea con `resizable(false)` por
                    // defecto) e inofensivo-pero-explícito en Left/Right.
                    // Orden importa: `.default_size` ENSANCHA el rango si se
                    // llama después de `.size_range`, así que va primero.
                    match self.deck.strip_side {
                        deck::StripSide::Left => {
                            egui::Panel::left("deck_strip_left")
                                .default_size(120.0)
                                .size_range(96.0..=280.0)
                                .resizable(true)
                                .show(ui, |ui| {
                                    strip_action =
                                        deck_strip::deck_strip_ui(&mut self.deck, active_dirty, ui);
                                });
                        }
                        deck::StripSide::Right => {
                            egui::Panel::right("deck_strip_right")
                                .default_size(120.0)
                                .size_range(96.0..=280.0)
                                .resizable(true)
                                .show(ui, |ui| {
                                    strip_action =
                                        deck_strip::deck_strip_ui(&mut self.deck, active_dirty, ui);
                                });
                        }
                        deck::StripSide::Top => {
                            egui::Panel::top("deck_strip_top")
                                .default_size(140.0)
                                .size_range(120.0..=280.0)
                                .resizable(true)
                                .show(ui, |ui| {
                                    strip_action =
                                        deck_strip::deck_strip_ui(&mut self.deck, active_dirty, ui);
                                });
                        }
                        deck::StripSide::Bottom => {
                            egui::Panel::bottom("deck_strip_bottom")
                                .default_size(140.0)
                                .size_range(120.0..=280.0)
                                .resizable(true)
                                .show(ui, |ui| {
                                    strip_action =
                                        deck_strip::deck_strip_ui(&mut self.deck, active_dirty, ui);
                                });
                        }
                    }
                }
                // Diseño bloqueado (`Slot::locked`, cabecera del lienzo en el
                // área central): deshabilita también los paneles, no solo
                // los gestos sobre el propio lienzo — "no se puede editar"
                // sin matizar por qué vía.
                let locked = self
                    .deck
                    .slots
                    .get(self.deck.active)
                    .is_some_and(|s| s.locked);
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
                            &mut self.deck,
                            ui,
                            &rs,
                            &mut self.renderer,
                            &mut self.surface,
                            &self.tx,
                            self.settings.new_canvas_format.extension(),
                        );
                    });

                // Saltar a otro lienzo de la baraja: clic en el propio
                // lienzo (ya deja `self.deck.jump_to` listo, dentro de
                // `canvas_ui`), tira lateral, o teclado (PageUp/PageDown/
                // Home/End). El intercambio es SIN PÉRDIDA — el lienzo
                // saliente queda guardado en su propia ranura con su
                // historial de deshacer intacto — así que, a diferencia de
                // «Back to gallery» (que sí sale del editor), no hace falta
                // preguntar por cambios sin guardar para saltar aquí dentro.
                let mut deck_target = state.deck_nav.take().and_then(|nav| match nav {
                    editor::DeckNav::Next => self.deck.next_path(),
                    editor::DeckNav::Prev => self.deck.prev_path(),
                    editor::DeckNav::First => self.deck.first_path(),
                    editor::DeckNav::Last => self.deck.last_path(),
                });
                match strip_action {
                    Some(deck_strip::StripAction::Open(path)) => {
                        deck_target = deck_target.or(Some(path));
                    }
                    // Inline, no `self.toggle_deck_axis()`: `state` de arriba
                    // ya tiene prestado `self.view` mutable (y sigue vivo más
                    // abajo), y el borrow checker no ve que ese método solo
                    // toca `self.deck`/`self.settings` — campos disjuntos.
                    Some(deck_strip::StripAction::ToggleAxis) => {
                        self.deck.axis = self.deck.axis.toggled();
                        self.deck.layout_dirty = true;
                        self.settings.deck_axis = self.deck.axis;
                        self.settings.save_in_background();
                    }
                    Some(deck_strip::StripAction::CycleSide) => {
                        self.deck.strip_side = self.deck.strip_side.cycled();
                        self.settings.deck_strip_side = self.deck.strip_side;
                        self.settings.save_in_background();
                        // Sin `layout_dirty = true`: mover el panel no
                        // cambia la geometría de la baraja, solo el rect del
                        // panel central — que `Viewport::note_size` ya
                        // detecta y reajusta.
                    }
                    Some(deck_strip::StripAction::AddCanvas) => {
                        if let Some(idx) = self.deck.push_placeholder(
                            self.settings.last_page_size,
                            self.settings.new_canvas_format.extension(),
                        ) {
                            self.deck.jump_to = Some(idx);
                            self.deck.jump_center = true;
                        }
                    }
                    None => {}
                }
                // Renombrar/duplicar/borrar desde la cabecera de un lienzo
                // (activo o de fondo) en el área central — mismas
                // operaciones que ya existían para la ranura activa (lápiz
                // junto al nombre, botón «Delete» del panel) o desde la
                // galería (duplicar), generalizadas por id/ruta en vez de
                // asumir "la activa".
                match canvas_action {
                    Some(editor::CanvasAction::Rename(id, new_stem)) => {
                        let is_active =
                            self.deck.slots.get(self.deck.active).map(|s| s.id) == Some(id);
                        let is_placeholder = self
                            .deck
                            .find_by_id(id)
                            .and_then(|index| self.deck.slots.get(index))
                            .is_some_and(|slot| slot.is_placeholder);
                        if is_placeholder {
                            self.deck.discard_placeholder(id, state);
                        } else if is_active {
                            // Reutiliza el camino ya existente (lápiz junto
                            // al nombre): se recoge y se lanza más arriba,
                            // en el próximo frame.
                            state.file_rename_requested = Some(new_stem);
                        } else if let Some(path) = self
                            .deck
                            .find_by_id(id)
                            .and_then(|i| self.deck.slots.get(i))
                            .map(|s| s.path.clone())
                        {
                            self.ignore_fs_events_until =
                                Some(std::time::Instant::now() + std::time::Duration::from_secs(2));
                            self.watcher = None;
                            loader::spawn_document_rename(
                                path,
                                new_stem,
                                self.tx.clone(),
                                ctx.clone(),
                            );
                        }
                    }
                    Some(editor::CanvasAction::Duplicate(id)) => {
                        let source = self
                            .deck
                            .find_by_id(id)
                            .and_then(|i| self.deck.slots.get(i))
                            .map(|slot| (slot.is_placeholder, slot.page, slot.path.clone()));
                        if let Some((true, page, path)) = source {
                            let ext = path
                                .extension()
                                .and_then(|value| value.to_str())
                                .unwrap_or(self.settings.new_canvas_format.extension());
                            self.deck.push_placeholder(
                                page.unwrap_or(self.settings.last_page_size),
                                ext,
                            );
                        } else if let Some((false, _, path)) = source {
                            loader::spawn_gallery_op(
                                loader::GalleryOp::Duplicate { path },
                                false,
                                self.tx.clone(),
                                ctx.clone(),
                            );
                        }
                    }
                    Some(editor::CanvasAction::Delete(id)) => {
                        let is_active =
                            self.deck.slots.get(self.deck.active).map(|s| s.id) == Some(id);
                        let is_placeholder = self
                            .deck
                            .find_by_id(id)
                            .and_then(|index| self.deck.slots.get(index))
                            .is_some_and(|slot| slot.is_placeholder);
                        if is_placeholder {
                            self.deck.discard_placeholder(id, state);
                        } else if is_active {
                            // Reutiliza el camino ya existente (botón
                            // «Delete» del panel de propiedades).
                            state.delete_requested = true;
                        } else if let Some(path) = self
                            .deck
                            .find_by_id(id)
                            .and_then(|i| self.deck.slots.get(i))
                            .map(|s| s.path.clone())
                        {
                            self.ignore_fs_events_until =
                                Some(std::time::Instant::now() + std::time::Duration::from_secs(2));
                            self.watcher = None;
                            let sidecar = canvas_io::find_sidecar(&path);
                            self.undoable_deletes.insert(path.clone(), sidecar);
                            loader::spawn_document_delete(path, self.tx.clone(), ctx.clone());
                        }
                    }
                    Some(editor::CanvasAction::ReplaceFromLocal(layer)) => {
                        loader::spawn_pick_replacement_image(
                            layer,
                            None,
                            self.tx.clone(),
                            ctx.clone(),
                        );
                    }

                    Some(editor::CanvasAction::ReplaceFromUrl(layer, url)) => {
                        loader::spawn_load_replacement_image_from_url(
                            layer,
                            url,
                            self.tx.clone(),
                            ctx.clone(),
                        );
                    }
                    Some(editor::CanvasAction::Menu(action)) => {
                        pending_menu_action = Some(action);
                    }
                    None => {}
                }
                if let Some(target) = deck_target {
                    if let Some(idx) = self.deck.find_by_path(&target) {
                        self.deck.jump_to = Some(idx);
                        // Por la tira o el teclado: el destino puede no
                        // estar a la vista, así que sí hace falta recentrar
                        // (a diferencia de un clic directo sobre el propio
                        // lienzo, que ya deja `jump_to` sin esto).
                        self.deck.jump_center = true;
                    }
                } else if let Some(&next_id) = self.save_all_queue.first() {
                    // «Save all»: sin una navegación más prioritaria este
                    // frame, salta a la próxima ranura pendiente de la cola.
                    if self.deck.slots.get(self.deck.active).map(|s| s.id) != Some(next_id) {
                        match self.deck.find_by_id(next_id) {
                            Some(idx) => {
                                self.deck.jump_to = Some(idx);
                                self.deck.jump_center = true;
                            }
                            // Desapareció (renombrada/borrada) mientras
                            // esperaba turno: se salta sin más.
                            None => {
                                self.save_all_queue.remove(0);
                            }
                        }
                    }
                } else if let Some(id) = state
                    .pending_global_undo
                    .as_ref()
                    .or(state.pending_global_redo.as_ref())
                    .map(editor::GlobalStep::slot_id)
                {
                    // Deshacer/rehacer global: el paso más reciente de toda
                    // la sesión le tocaba a OTRO diseño de la baraja — salta
                    // a él para mostrarlo (mismo patrón que «Save all»,
                    // arriba). `request_loads`, más abajo en `canvas_ui`,
                    // dispara la recarga de disco si esa ranura ya no está
                    // `Ready` (fue descartada por presupuesto).
                    match self.deck.find_by_id(id) {
                        Some(idx) => {
                            self.deck.jump_to = Some(idx);
                            self.deck.jump_center = true;
                        }
                        // El diseño desapareció (archivo borrado) mientras
                        // esperaba turno: se descarta ese paso, sin
                        // encadenar automáticamente con el siguiente.
                        None => {
                            state.discard_pending_global_undo();
                            state.discard_pending_global_redo();
                        }
                    }
                }
                // Aplica el salto si el destino ya está listo y el editor
                // está ocioso; si no, la petición queda pendiente y se
                // reintenta en los próximos frames — llamar aquí siempre,
                // no solo cuando `deck_target` trae algo nuevo, es lo que
                // reintenta un salto que aún esperaba a que su carga
                // terminase. Recentra la vista SOLO si quien pidió el salto
                // lo marcó (`jump_center`): un clic directo sobre el propio
                // lienzo ya se ve, recentrar ahí sería mover la cámara sin
                // que el usuario lo pidiera.
                //
                // NUNCA mientras haya un modal de guardado pendiente
                // (`overwrite_prompt`/`readonly_prompt`): `is_idle()` ya
                // cubre `saving`, pero esos modales aparecen ANTES de que
                // `start_save` los ponga a `true` — sin este freno, saltar
                // en ese hueco dejaría el modal hablando de un archivo
                // mientras `state` pasa a ser otro documento, y al
                // confirmarlo se guardarían los píxeles del documento
                // EQUIVOCADO en la ruta del modal. Igual con
                // `materializing`: la reserva de nombre de una provisional
                // tampoco pone `saving` a `true` todavía, y saltar a mitad
                // de esa reserva dejaría la respuesta actuando sobre el
                // lienzo equivocado.
                let save_modal_pending = self.overwrite_prompt.is_some()
                    || self.readonly_prompt.is_some()
                    || self.materializing.is_some();
                if !save_modal_pending
                    && deck::apply_jump(&mut self.deck, state)
                    && std::mem::take(&mut self.deck.jump_center)
                {
                    state.viewport.request_center(self.deck.active_rect());
                }
                // «Save all»: si la activa ya es la ranura que tocaba,
                // dispara su guardado — mismo camino que Ctrl+S, un frame
                // más tarde (el bloque de guardado de este frame ya corrió
                // antes de que se dibujaran los paneles).
                if let Some(&next_id) = self.save_all_queue.first() {
                    if self.deck.slots.get(self.deck.active).map(|s| s.id) == Some(next_id) {
                        // El aviso de sobrescritura (primer lienzo raster del
                        // lote) o el redirect de SVG/GIF cuentan como "en
                        // curso", no como fallo: sin este freno, el intento
                        // ya marcado se leería como fallido mientras el
                        // usuario todavía no ha respondido al modal.
                        let waiting_on_modal =
                            self.overwrite_prompt.is_some() || self.readonly_prompt.is_some();
                        if !state.is_dirty() {
                            // Ya se guardó (`AppMsg::Saved` la sacó de la
                            // cola) o nunca hizo falta: nada que hacer aquí.
                        } else if state.saving || waiting_on_modal {
                            // En curso, o esperando la respuesta del usuario.
                        } else if self.save_all_attempted {
                            // Se pulsó "Guardar", no hay guardado en curso ni
                            // modal pendiente, y sigue sucia: ese intento
                            // falló de verdad (o el usuario canceló el
                            // modal). Se aborta el lote en vez de reintentar
                            // sin fin sobre el mismo lienzo.
                            tracing::warn!(
                                "Save all: se detiene en un lienzo de fondo (guardado fallido o cancelado)"
                            );
                            self.save_all_queue.clear();
                            self.save_all_attempted = false;
                        } else {
                            self.save_all_attempted = true;
                            state.save_clicked = true;
                        }
                    }
                }
                // Deshacer/rehacer global: si la activa ya es la ranura que
                // le tocaba a la petición pendiente (el salto de arriba se
                // aplicó, esta misma vuelta o en una anterior), ejecuta el
                // paso local ahora que es la activa y limpia la petición.
                if state.pending_global_undo.as_ref().is_some_and(|step| {
                    self.deck.slots.get(self.deck.active).map(|s| s.id) == Some(step.slot_id())
                }) {
                    state.finish_pending_global_undo();
                }
                if state.pending_global_redo.as_ref().is_some_and(|step| {
                    self.deck.slots.get(self.deck.active).map(|s| s.id) == Some(step.slot_id())
                }) {
                    state.finish_pending_global_redo();
                }
                // Deshacer un borrado (`GlobalStep::Delete`): no pertenece a
                // ninguna ranura, así que `undo()` ya lo resolvió sin
                // esperar ningún salto — solo queda lanzar la restauración.
                if let Some(record) = state.pending_restore.take() {
                    loader::spawn_restore_from_trash(
                        record.path,
                        record.sidecar,
                        self.tx.clone(),
                        ctx.clone(),
                    );
                }

                if std::mem::take(&mut state.settings_clicked) {
                    self.show_settings = true;
                }
                // El checkbox del sidecar en el editor ES el valor por defecto
                // persistido: cambiarlo ahí lo recuerda para el futuro. En un
                // diseño el checkbox ni se muestra: no debe tocar el ajuste.
                if !state.is_design && state.sidecar_enabled != self.settings.sidecar_default {
                    self.settings.sidecar_default = state.sidecar_enabled;
                    self.settings.save_in_background();
                }
            }
        }

        // Resuelve la acción del menú contextual del lienzo capturada más
        // arriba (ver el porqué del aplazamiento en su declaración):
        // reutiliza el mismo camino que ya usa la barra de menú, sin
        // duplicar ninguna lógica.
        if let Some(action) = pending_menu_action {
            self.handle_menu_action(action, &ctx);
        }

        // Ventana de ajustes (accesible desde la bienvenida y el editor).
        if self.show_settings {
            let before = self.settings.clone();
            let action = settings::settings_window(
                &ctx,
                &mut self.settings,
                &mut self.show_settings,
                &self.shell_status,
            );
            if self.settings != before {
                self.settings.save_in_background();
            }
            if let Some(action) = action {
                self.shell_status = "Working…".to_owned();
                let tx = self.tx.clone();
                let ctx2 = ctx.clone();
                std::thread::spawn(move || {
                    let shell = canvas_shell::platform();
                    let result = match action {
                        settings::SettingsAction::RegisterShell => std::env::current_exe()
                            .map_err(|e| e.to_string())
                            .and_then(|exe| {
                                shell
                                    .register_file_associations(&exe)
                                    .map(|()| {
                                        "Explorer integration registered. Right-click an \
                                         image → Open with → Canvas Desktop."
                                            .to_owned()
                                    })
                                    .map_err(|e| e.to_string())
                            }),
                        settings::SettingsAction::UnregisterShell => shell
                            .unregister_file_associations()
                            .map(|()| "Explorer integration removed.".to_owned())
                            .map_err(|e| e.to_string()),
                    };
                    let _ = tx.send(AppMsg::ShellIntegrationDone(result));
                    ctx2.request_repaint();
                });
            }
        }

        if let Some(nav) = open_next {
            self.navigate(nav, &ctx);
        }

        // Ventana «About» (menú Help).
        if self.show_about {
            egui::Window::new("About Canvas Desktop")
                .open(&mut self.show_about)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .show(&ctx, |ui| {
                    ui.label(format!("Canvas Desktop {}", env!("CARGO_PKG_VERSION")));
                    ui.weak("A native canvas editor that saves straight to your image files.");
                });
        }

        // Mantén el watcher `notify` apuntando al archivo abierto (si lo hay).
        let desired = match &self.view {
            View::Editor(state) => state.doc.source_path.clone(),
            _ => None,
        };
        if self.watcher.as_ref().map(|w| w.path.as_path()) != desired.as_deref() {
            self.watcher = desired.and_then(|p| watcher::watch(&p, self.tx.clone(), ctx.clone()));
        }

        self.sync_title(&ctx);
    }
}
