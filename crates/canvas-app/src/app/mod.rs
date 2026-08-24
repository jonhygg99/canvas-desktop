//! El tipo `App` (estado de la ventana eframe), sus vistas (`View`/`Nav`),
//! el constructor, y el bucle de `impl eframe::App::ui` que ensambla el
//! frame entero: menús, la baraja/lienzo del editor o la galería/bienvenida,
//! y los modales (guardar, sobrescribir, cerrar).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};

use anyhow::{Context, Result};
use canvas_render::CanvasRenderer;
use eframe::egui;

use crate::loader::AppMsg;
use crate::surface::CanvasSurface;
use crate::{deck, editor, export, gallery, menus, paste_hook, settings, watcher};

mod frame;
mod menu_actions;
mod messages;
mod navigation;
pub(crate) mod persistence;
mod ui_menu;
mod ui_modals;
mod views;
mod window;

enum View {
    Welcome { error: Option<String> },
    Loading { path: PathBuf },
    Gallery(Box<gallery::GalleryState>),
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
    NewDesignInFolder {
        seed: deck::DeckSeed,
    },
}

pub(crate) struct App {
    renderer: CanvasRenderer,
    surface: Option<CanvasSurface>,
    view: View,
    tx: Sender<AppMsg>,
    rx: Receiver<AppMsg>,
    last_title: String,
    /// Directorio de caché de miniaturas (si se pudo crear).
    thumb_cache: Option<PathBuf>,
    /// Baraja de lienzos del editor abierto: todos los archivos de su
    /// carpeta de origen, para saltar entre ellos sin volver a la galería.
    /// Sobrevive al cambio de vista (a diferencia de `GalleryState`, que se
    /// destruye), así que también sirve para sembrar la rejilla al volver.
    deck: deck::Deck,
    /// Ajustes persistidos del usuario.
    settings: settings::AppSettings,
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
    /// Watcher `notify` del archivo abierto en el editor, si lo hay.
    watcher: Option<watcher::DocWatcher>,
    /// Ventana de gracia tras un guardado propio: los eventos del watcher
    /// hasta este instante son nuestros y se descartan.
    ignore_fs_events_until: Option<std::time::Instant>,
    /// Estado del camino de guardado.
    save: SaveFlow,
    /// Estado del camino de exportacion.
    export: ExportFlow,
    /// Contabilidad de la baraja que cruza con el disco.
    deck_ops: DeckOps,
    /// Espejo del estado de los menus nativos.
    menu_mirror: MenuMirror,
}

/// Todo lo que hace falta para llevar un guardado a termino: lo pedido, lo
/// diferido, los modales de aviso y el lote de «Save all».
pub(super) struct SaveFlow {
    /// «Guardar como…» elegido, pendiente de hornear (necesita la GPU).
    pending_save_as: Option<PathBuf>,
    /// Guardar solicitado desde el diálogo de cierre.
    save_requested: bool,
    /// Cerrar la ventana en cuanto termine el guardado en curso.
    close_after_save: bool,
    /// El usuario ya confirmó el cierre: no volver a preguntar.
    allow_close: bool,
    /// Navegación pendiente para cuando termine el guardado en curso.
    after_save: Option<Nav>,
    /// El usuario ya confirmó la sobrescritura destructiva en esta sesión.
    overwrite_confirmed: bool,
    /// Sobrescritura pendiente de confirmar en el modal (ruta del original).
    overwrite_prompt: Option<PathBuf>,
    /// Estado del checkbox «Don't ask again» mientras el modal está abierto.
    overwrite_dont_ask: bool,
    /// El original no admite sobrescritura (SVG/GIF): modal que redirige a
    /// «Save as…».
    readonly_prompt: Option<PathBuf>,
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
}

/// El dialogo de exportacion y lo que queda pendiente de el.
pub(super) struct ExportFlow {
    /// Diálogo de exportación visible.
    export_dialog: Option<export::ExportDialog>,
    /// Ajustes ya elegidos en el diálogo, pendientes de la ruta de archivo.
    pending_export_settings: Option<export::ExportSettings>,
    /// Ruta y ajustes de exportación, pendiente de hornear (necesita la GPU).
    pending_export: Option<(PathBuf, export::ExportSettings)>,
}

/// Contabilidad de la baraja que no cabe en `Deck` porque cruza con el disco:
/// la semilla pendiente de la galeria, las reservas de nombre en vuelo y los
/// borrados que todavia se pueden deshacer.
pub(super) struct DeckOps {
    /// Semilla capturada de la galería justo antes de navegar a un lienzo
    /// suyo; `resolve_deck` la consume en cuanto la carga termina.
    pending_deck: Option<deck::DeckSeed>,
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

/// Ultimo estado comunicado a los menus nativos, para no reenviarlo cada frame.
struct MenuMirror {
    /// Último estado «hay editor abierto» comunicado al menú.
    menus_editor_open: bool,
    /// Último estado de `History::can_undo`/`can_redo` comunicado al menú.
    menus_can_undo: bool,
    menus_can_redo: bool,
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
            thumb_cache: window::thumbnail_cache_dir(),
            deck: deck::Deck::default(),
            settings: settings::AppSettings::load(),
            show_settings: false,
            shell_status: String::new(),
            menus: native_menus,
            show_about: false,
            applied_theme: None,
            watcher: None,
            ignore_fs_events_until: None,
            save: SaveFlow {
                pending_save_as: None,
                save_requested: false,
                close_after_save: false,
                allow_close: false,
                after_save: None,
                overwrite_confirmed: false,
                overwrite_prompt: None,
                overwrite_dont_ask: false,
                readonly_prompt: None,
                save_all_queue: Vec::new(),
                save_all_attempted: false,
            },
            export: ExportFlow {
                export_dialog: None,
                pending_export_settings: None,
                pending_export: None,
            },
            deck_ops: DeckOps {
                pending_deck: None,
                materializing: None,
                materialize_blocked: None,
                undoable_deletes: HashMap::new(),
            },
            menu_mirror: MenuMirror {
                menus_editor_open: false,
                menus_can_undo: false,
                menus_can_redo: false,
            },
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

        self.sync_and_show_menu(ui, &ctx);

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
                let before_recent = self.settings.recent_files.len();
                let before_pin = self.settings.pinned_folders.len();
                open_next = views::welcome_view_ui(
                    ui,
                    error.as_deref(),
                    &mut self.settings.recent_files,
                    &mut self.settings.pinned_folders,
                    &mut self.show_settings,
                    &self.tx,
                    &ctx,
                );
                if self.settings.recent_files.len() != before_recent
                    || self.settings.pinned_folders.len() != before_pin
                {
                    self.settings.save_in_background();
                    if let Some(m) = self.menus.as_mut() {
                        m.set_recents(&self.settings.recent_files);
                    }
                }
            }
            View::Loading { path } => {
                views::loading_view_ui(ui, path);
            }
            View::Gallery(g) => {
                open_next = views::gallery_view_ui(
                    g,
                    ui,
                    &mut self.settings,
                    &mut self.deck_ops.pending_deck,
                    &self.tx,
                    &ctx,
                );
            }
            View::Editor(state) => {
                // Si no hay estado de wgpu (backend glow activo), se corta
                // el frame ENTERO aquí — no solo esta vista — igual que
                // antes de este refactor.
                let Some(rs) = frame.wgpu_render_state().cloned() else {
                    return;
                };
                let mut frame = frame::EditorFrame {
                    deck: &mut self.deck,
                    renderer: &mut self.renderer,
                    surface: &mut self.surface,
                    tx: &self.tx,
                    settings: &mut self.settings,
                    show_settings: &mut self.show_settings,
                    watcher: &mut self.watcher,
                    ignore_fs_events_until: &mut self.ignore_fs_events_until,
                    save: &mut self.save,
                    export: &mut self.export,
                    deck_ops: &mut self.deck_ops,
                };
                let (nav, action) =
                    views::editor_view_ui(ui, &ctx, &rs, state, paste_requested, &mut frame);
                open_next = nav;
                pending_menu_action = action;
            }
        }

        // Resuelve la acción del menú contextual del lienzo capturada más
        // arriba (ver el porqué del aplazamiento en su declaración):
        // reutiliza el mismo camino que ya usa la barra de menú, sin
        // duplicar ninguna lógica.
        if let Some(action) = pending_menu_action {
            self.handle_menu_action(action, &ctx);
        }

        self.settings_window_ui(&ctx);

        if let Some(nav) = open_next {
            self.navigate(nav, &ctx);
        }

        self.about_window_ui(&ctx);

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
