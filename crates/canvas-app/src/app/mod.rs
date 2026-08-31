//! La app: `App` es la cáscara eframe (una por proceso), y `AppInner` es el
//! estado real — compartido entre las ventanas nativas del conmutador por
//! `Arc<Mutex<…>>`. Cada ventana es un `Workspace` (ver `workspace.rs`).
//!
//! - La ventana raíz se pinta desde `impl eframe::App::ui`; el resto se
//!   crean con `Context::show_viewport_deferred`, cuyos callbacks corren en
//!   sus propias pasadas del bucle de eventos (mismo hilo, nunca anidados),
//!   así que los `Mutex` nunca compiten de verdad — solo existen para que
//!   los callbacks `'static` compilen.
//! - El `RenderState` de wgpu es UNO para todo el proceso (eframe crea un
//!   único device/renderer y cada ventana solo es una surface más); se clona
//!   al arrancar y lo usan también las ventanas hijas.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};

use canvas_render::CanvasRenderer;
use eframe::egui;

use crate::loader::AppMsg;
use crate::lock::LockExt;
use crate::{deck, editor, export, gallery, menus, paste_hook, settings};

mod bootstrap;
mod frame;
mod menu_actions;
mod messages;
mod navigation;
pub(crate) mod persistence;
mod switcher;
mod ui_menu;
mod ui_modals;
mod views;
mod window;
mod workspace;
mod workspace_lifecycle;
mod ws_frame;

pub(crate) use workspace::Workspace;

pub(crate) enum View {
    Welcome { error: Option<String> },
    Loading { path: PathBuf },
    Gallery(Box<gallery::GalleryState>),
    Editor(Box<editor::EditorState>),
}

/// Navegación diferida: qué hacer cuando termine el guardado en curso o al
/// final del frame (para no pelear con el préstamo de `ws.view`).
#[derive(Clone)]
pub(crate) enum Nav {
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

/// Qué hay pendiente de decidir detrás de un diálogo «¿guardar los
/// cambios?» que corre en un hilo aparte (ver `AppMsg::UnsavedDialogAnswer`):
/// cerrar LA VENTANA, o aplicar una navegación.
pub(crate) enum UnsavedDialog {
    WindowClose,
    Navigate(Nav),
}

/// La shell eframe; `inner` se comparte con los callbacks de las ventanas
/// hijas. Los menús nativos (muda, Windows) viven AQUÍ, fuera del `Arc`:
/// sus `Rc` internos romperían `Send`/`Sync` en el closure de
/// `show_viewport_deferred` de las ventanas hijas. Solo la ventana raíz los
/// tiene; las hijas dibujan la barra de respaldo egui.
pub(crate) struct App {
    inner: Arc<Mutex<AppInner>>,
    /// Menús nativos (muda, Windows); `None` si no se pudieron instalar o en
    /// plataformas sin ellos.
    pub(crate) menus: Option<menus::AppMenus>,
}

/// Estado de la app completa, accesible desde cualquier ventana. Aunque hay
/// un solo hilo de UI, los campos guardan lo que era de `App` pero ahora es
/// compartido por `Arc`.
pub(crate) struct AppInner {
    /// Todas las ventanas; la 0 es la raíz (no se puede cerrar sin salir).
    pub(crate) workspaces: Vec<Arc<Mutex<Workspace>>>,
    /// Manejar compartido con el que los callbacks de las ventanas hijas
    /// recuperan el `AppInner` original (un ciclo `Arc` que se rompe en
    /// `on_exit`).
    pub(crate) me: Option<Arc<Mutex<AppInner>>>,
    /// Renderer de vello, compartido (el device wgpu es único).
    pub(crate) renderer: CanvasRenderer,
    /// `RenderState` de egui-wgpu clonado: las ventanas hijas no reciben
    /// `Frame`, pero registran sus texturas nativas en este mismo renderer.
    pub(crate) rs: eframe::egui_wgpu::RenderState,
    pub(crate) settings: settings::AppSettings,
    /// Directorio de caché de miniaturas (si se pudo crear).
    pub(crate) thumb_cache: Option<PathBuf>,
    /// Resultado del último registro/desregistro del Explorador.
    pub(crate) shell_status: String,
    /// Espejo del estado de los menús nativos.
    pub(crate) menu_mirror: MenuMirror,
    /// Último tema aplicado (para no reaplicar cada frame).
    pub(crate) applied_theme: Option<settings::ThemeChoice>,
    /// Conmutador rápido (Ctrl+Tab / Ctrl+`).
    pub(crate) switcher: switcher::SwitcherState,
    /// Índice del workspace enfocado, refrescado cada frame raíz.
    pub(crate) focused: usize,
    /// Canal global: mensajes que no pertenecen a ninguna ventana (segunda
    /// instancia, registro de shell).
    pub(crate) shell_rx: Receiver<AppMsg>,
    pub(crate) shell_tx: Sender<AppMsg>,
    /// Siguiente id de workspace a repartir.
    pub(crate) next_ws_id: u64,
    /// Último instante en que se persistieron los workspaces (throttle de
    /// geometría; también se fuerza al crear/cerrar y en `on_exit`).
    pub(crate) last_workspaces_save: Option<std::time::Instant>,
    /// Se creó/cerró un workspace y hay que persistir la lista al final del
    /// frame raíz. No se persiste en el momento: `new_workspace` puede
    /// llamarse con el lock del workspace actual tomado, y persistir
    /// bloquea TODOS los workspaces (`std::sync::Mutex` no reentrante →
    /// deadlock, el «File ▸ New Window se congela»).
    pub(crate) workspaces_dirty: bool,
    /// Foco pendiente para una ventana recién CREADA: el viewport de la
    /// ventana no existe hasta `spawn_child_viewports` del mismo frame, así
    /// que `focus_workspace` inmediato perdería el comando. Se consume en
    /// `root_frame` justo después de crear las hijas.
    pub(crate) pending_focus: Option<usize>,
    /// Diagnóstico del atlas de vello (solo con `CANVAS_DEBUG_ATLAS=1`):
    /// últimos totales vistos y contador de frames para el log periódico.
    pub(crate) atlas_regs: u64,
    pub(crate) atlas_reups: u64,
    pub(crate) atlas_log_frames: u64,
}

/// Todo lo que hace falta para llevar un guardado a término: lo pedido, lo
/// diferido, los modales de aviso y el lote de «Save all». Uno por workspace.
#[derive(Default)]
pub(super) struct SaveFlow {
    /// «Guardar como…» elegido, pendiente de hornear (necesita la GPU).
    pub(super) pending_save_as: Option<PathBuf>,
    /// Guardar solicitado desde el diálogo de cierre.
    pub(super) save_requested: bool,
    /// Cerrar la ventana en cuanto termine el guardado en curso.
    pub(super) close_after_save: bool,
    /// El usuario ya confirmó el cierre: no volver a preguntar.
    pub(super) allow_close: bool,
    /// Navegación pendiente para cuando termine el guardado en curso.
    pub(super) after_save: Option<Nav>,
    /// El usuario ya confirmó la sobrescritura destructiva en esta sesión.
    pub(super) overwrite_confirmed: bool,
    /// Sobrescritura pendiente de confirmar en el modal (ruta del original).
    pub(super) overwrite_prompt: Option<PathBuf>,
    /// Estado del checkbox «Don't ask again» mientras el modal está abierto.
    pub(super) overwrite_dont_ask: bool,
    /// El original no admite sobrescritura (SVG/GIF): modal que redirige a
    /// «Save as…».
    pub(super) readonly_prompt: Option<PathBuf>,
    /// Guardado raster (imagen) a punto de sobrescribir un archivo cuyo
    /// documento ya no tiene NINGUNA capa de imagen/SVG (p. ej. tras borrar
    /// la última foto): pide confirmación antes de aplanar y descartar la
    /// copia editable. `Some(ruta)` mientras el modal está abierto.
    pub(super) discard_raster_prompt: Option<PathBuf>,
    /// El usuario ya confirmó «Save anyway» para un documento sin capas de
    /// imagen en esta sesión (mismo criterio que `overwrite_confirmed`): no
    /// volver a avisar en guardados posteriores.
    pub(super) discard_raster_confirmed: bool,
    /// «Save all»: ids (estables) de las ranuras sucias que faltan por
    /// guardar, el activo excluido.
    pub(super) save_all_queue: Vec<u64>,
    /// Ya se pulsó «Guardar» para la ranura al frente de `save_all_queue`.
    pub(super) save_all_attempted: bool,
    /// Aviso de poca RAM libre antes de un «Save all» masivo: cuántos
    /// documentos se escribirían. `Some` mientras el modal está abierto;
    /// «Save all anyway» lo limpia y arranca la cola, «Cancel» lo limpia y
    /// descarta el lote.
    pub(super) low_memory_prompt: Option<usize>,
}

/// El diálogo de exportación y lo que queda pendiente de él. Uno por
/// ventana.
#[derive(Default)]
pub(super) struct ExportFlow {
    /// Diálogo de exportación visible.
    pub(super) export_dialog: Option<export::ExportDialog>,
    /// Ajustes ya elegidos en el diálogo, pendientes de la ruta de archivo.
    pub(super) pending_export_settings: Option<export::ExportSettings>,
    /// Ruta y ajustes de exportación, pendientes de hornear (necesita la GPU).
    pub(super) pending_export: Option<(PathBuf, export::ExportSettings)>,
}

/// Contabilidad de la baraja que no cabe en `Deck` porque cruza con el
/// disco. Uno por ventana.
#[derive(Default)]
pub(super) struct DeckOps {
    /// Semilla capturada de la galería justo antes de navegar a un lienzo
    /// suyo; `resolve_deck` la consume en cuanto la carga termina.
    pub(super) pending_deck: Option<deck::DeckSeed>,
    /// Id de la ranura provisional cuya reserva de nombre está en vuelo.
    pub(super) materializing: Option<u64>,
    /// Id de una ranura provisional cuya reserva FALLÓ.
    pub(super) materialize_blocked: Option<u64>,
    /// Rutas cuyo borrado en curso se pidió directamente por el usuario,
    /// con el sidecar que tenían (para apilar `GlobalStep::Delete`).
    pub(super) undoable_deletes: std::collections::HashMap<PathBuf, Option<PathBuf>>,
}

/// Último estado comunicado a los menús nativos, para no reenviarlo cada
/// frame.
#[derive(Default)]
pub(crate) struct MenuMirror {
    /// Último estado «hay editor abierto» comunicado al menú.
    pub(crate) menus_editor_open: bool,
    /// Último estado de `History::can_undo`/`can_redo` comunicado al menú.
    pub(crate) menus_can_undo: bool,
    pub(crate) menus_can_redo: bool,
    /// Última lista de recientes comunicada al menú (para no llamar
    /// `set_recents` en cada frame).
    pub(crate) menus_recents: Vec<PathBuf>,
}

impl App {
    /// Sondear clics del menú nativo (solo existe en la raíz de Windows) y
    /// aplicarlos sobre el workspace raíz. Se ejecuta antes del frame raíz;
    /// el resto de ventanas no tiene menú nativo. `menus` se pasa aparte (no
    /// `&mut self`) porque `inner` ya presta `self.inner` — borrows disjuntos
    /// de campos.
    fn poll_native_menu(
        menus: &Option<menus::AppMenus>,
        inner: &mut AppInner,
        ctx: &egui::Context,
    ) {
        while let Some(action) = menus.as_ref().and_then(|m| m.poll()) {
            let Some(ws0) = inner.workspaces.first().cloned() else {
                continue;
            };
            let mut ws0 = ws0.lock_ok();
            inner.handle_menu_action(&mut ws0, action, ctx);
        }
    }

    /// Refleja el estado del editor de la RAÍZ en el menú nativo (ítems
    /// habilitados y recientes), solo cuando cambió (espejo `MenuMirror`).
    /// Mismo patrón de borrows disjuntos que `poll_native_menu`.
    fn sync_native_menu(menus: &mut Option<menus::AppMenus>, inner: &mut AppInner) {
        let Some(menus) = menus.as_mut() else {
            return;
        };
        let Some(ws0) = inner.workspaces.first().cloned() else {
            return;
        };
        let (editor_open, can_undo, can_redo) = {
            let ws0 = ws0.lock_ok();
            match &ws0.view {
                View::Editor(state) => (true, state.can_undo(), state.can_redo()),
                _ => (false, false, false),
            }
        };
        if editor_open != inner.menu_mirror.menus_editor_open {
            inner.menu_mirror.menus_editor_open = editor_open;
            menus.set_editor_enabled(editor_open);
        }
        if (can_undo, can_redo)
            != (
                inner.menu_mirror.menus_can_undo,
                inner.menu_mirror.menus_can_redo,
            )
        {
            inner.menu_mirror.menus_can_undo = can_undo;
            inner.menu_mirror.menus_can_redo = can_redo;
            menus.set_undo_redo(can_undo, can_redo);
        }
        if inner.menu_mirror.menus_recents != inner.settings.recent_files {
            inner.menu_mirror.menus_recents = inner.settings.recent_files.clone();
            menus.set_recents(&inner.settings.recent_files);
        }
    }
}
impl eframe::App for App {
    /// eframe ejecuta `App::logic` en TODOS los passes de la raíz — también
    /// en los Ocultos (fullscreen, minimizada), donde `App::ui` se salta.
    /// Las hijas se re-registran aquí, no en `App::ui`: si solo se
    /// registraran en `App::ui`, un pass oculto las eliminaría
    /// («never used this pass») y la ventana se destruiría — el bug que
    /// hacía que el fullscreen se quitara solo con una segunda ventana.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let inner = self.inner.lock_ok();
        inner.spawn_child_viewports(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let mut inner = self.inner.lock_ok();
        App::poll_native_menu(&self.menus, &mut inner, &ctx);
        inner.root_frame(ui, &ctx);
        App::sync_native_menu(&mut self.menus, &mut inner);
    }

    /// Al salir (cierre de la raíz, Alt+F4 del SO) se persiste el estado de
    /// las ventanas como historial en `settings.json`; ya no se restaura al
    /// arrancar: la app siempre abre en la home.
    fn on_exit(&mut self) {
        // lock_ok y no `if let Ok`: aunque un workspace haya envenenado su
        // lock con un pánico, la persistencia de geometría sigue siendo
        // segura (y sí se quiere al salir).
        self.inner.lock_ok().persist_workspaces_now();
    }
}

impl AppInner {
    /// El frame de cada arranque: tema, foco, mensajes, la ventana raíz y
    /// la creación de las hijas.
    fn root_frame(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if self.applied_theme != Some(self.settings.theme) {
            ctx.set_theme(self.settings.theme.to_egui());
            self.applied_theme = Some(self.settings.theme);
        }
        self.update_focused(ctx);
        // El flag global de pegado (paste_hook) NO lo roba la raíz: lo
        // consume la ventana ENFOCADA en su PROPIO pase. La raíz no sabe en
        // qué orden correrá respecto del pase de la hija dentro de la misma
        // pasada del event loop, así que cualquier reparto con estado entre
        // frames tiene una carrera; «solo la enfocada toma el flag» no la
        // tiene — el resto de ventanas ni lo mira.
        self.drain_all(ctx);

        let ws0_arc = Arc::clone(&self.workspaces[0]);
        let mut ws0 = ws0_arc.lock_ok();
        let paste = self.focused == 0 && paste_hook::take_request();
        self.ws_frame(ui, ctx, &mut ws0, 0, true, paste);
        drop(ws0);

        // Se re-registran las hijas TAMBIÉN aquí, al final del frame de la
        // raíz: `App::logic` (donde viven habitualmente) corre ANTES que
        // `App::ui`, así que en el pase que crea un workspace nuevo con
        // Cmd+N/T o el menú la hija no existía aún cuando `logic` corrió.
        // Sin esta segunda llamada, `focus_workspace` crearía un viewport
        // fantasma (vía `request_repaint_of`) que `end_pass` eliminaría por
        // no estar registrado («never used this pass») y la ventana se
        // destruiría y recrearía — el ciclo que se veía como parpadeo.
        // `logic` sigue encargándose de los pases Ocultos (fullscreen).
        self.spawn_child_viewports(ctx);
        if let Some(idx) = self.pending_focus.take() {
            self.focus_workspace(idx, ctx);
        }
        self.finish_root_frame(ctx);
        self.maybe_log_atlas(ctx);
    }

    /// Log de diagnóstico del atlas de vello, activado con
    /// `CANVAS_DEBUG_ATLAS=1`. Cada frame imprime cuántas texturas NUEVAS se
    /// registraron y cuántas se re-subieron (re-horneados) en el renderer
    /// compartido. El estado estacionario de dos ventanas de editor debe ser
    /// 0/0: cualquier re-subida por frame sin que nadie edite es un scope
    /// colisionando entre ventanas. Además fuerza un repintado por frame
    /// mientras está activo, para poder observar los contadores en vivo
    /// aunque la UI esté en reposo (egui se detiene si nada cambia).
    fn maybe_log_atlas(&mut self, ctx: &egui::Context) {
        if std::env::var_os("CANVAS_DEBUG_ATLAS").is_none() {
            return;
        }
        let stats = self.renderer.atlas_stats();
        let reg = stats.registrations.saturating_sub(self.atlas_regs);
        let reup = stats.reuploads.saturating_sub(self.atlas_reups);
        self.atlas_regs = stats.registrations;
        self.atlas_reups = stats.reuploads;
        self.atlas_log_frames += 1;
        if reg > 0 || reup > 0 || self.atlas_log_frames % 60 == 0 {
            tracing::info!(
                "atlas de vello: frame={} registros_nuevos={} re_subidas={} | acumulado: {} registros, {} re-subidas",
                self.atlas_log_frames,
                reg,
                reup,
                stats.registrations,
                stats.reuploads,
            );
        }
        ctx.request_repaint();
    }

    /// El frame de una ventana hija: drena su propio canal (#por si se repintó
    /// sin que el UI pasara antes) y pinta lo mismo que la raíz.
    fn child_frame(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        ws: &mut Workspace,
        idx: usize,
    ) {
        let mut open_after = None;
        self.drain_ws(ws, ctx, &mut open_after);
        if let Some(nav) = open_after {
            self.navigate(ws, nav, ctx);
        }
        // El pegado lo consume SOLO la ventana enfocada (ver `root_frame`).
        let paste = self.focused == idx && paste_hook::take_request();
        self.ws_frame(ui, ctx, ws, idx, false, paste);
        // Con el diagnóstico del atlas activo (`CANVAS_DEBUG_ATLAS=1`), la
        // ventana hija también repinta cada frame: solo la raíz forzaba el
        // repintado continuo, así que el modo estrés de dos ventanas era
        // asimétrico (la hija apenas editaba).
        if std::env::var_os("CANVAS_DEBUG_ATLAS").is_some() {
            ctx.request_repaint_of(ws.viewport);
        }
    }
}
