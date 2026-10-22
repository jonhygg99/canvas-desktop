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
//! - El fullscreen es el `ViewportCommand::Fullscreen` NATIVO en todas las
//!   plataformas (incluido macOS) y NO hay tabbing nativo: en macOS 26 el
//!   `NSWindowStackController` lanza una excepción asíncrona e incapturable
//!   (`titlebarAccessoryViewControllers not supported for this window style`)
//!   en cualquier sincronización de la barra de pestañas — tanto al entrar en
//!   la transición de fullscreen como al ocultar la barra de título de una
//!   ventana de un grupo — que escapa por el FFI hacia Rust (sin handler de
//!   excepciones ObjC) y aborta el proceso. Por eso `apply_native_tabbing`
//!   se eliminó: Cmd+N/Ctrl+T abren ventanas separadas gestionadas por el
//!   conmutador propio (Ctrl+Tab). Un «simple fullscreen» propio (ocultar la
//!   barra de título con objc2) también se descartó: exigía tocar el
//!   `styleMask`, y ocultar la barra de una ventana en un grupo dispara la
//!   misma excepción.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};

use canvas_render::CanvasRenderer;
use eframe::egui;

use crate::loader::AppMsg;
use crate::lock::LockExt;
use crate::{deck, editor, export, gallery, menus, paste_hook, settings};

mod autosave;
mod bootstrap;
#[cfg(test)]
mod clip_fixture;
mod clip_save;
#[cfg(test)]
#[path = "deck_ops_tests.rs"]
mod deck_ops_tests;
mod frame;
mod menu_actions;
mod messages;
mod navigation;
pub(crate) mod persistence;
mod switcher;
mod ui_menu;
mod ui_modals;
mod video_canvas_names;
mod video_canvases;
mod views;
pub(crate) mod window; // la poda de miniaturas de arranque (`main`) usa `thumbnail_cache_dir`
mod workspace;
mod workspace_lifecycle;
mod write_jobs;
mod write_queue;
mod write_target;
mod ws_frame;

pub(crate) use workspace::Workspace;

pub(crate) enum View {
    /// Pantalla inicial o callejón tras una apertura fallida: `failed_*`
    /// guardan QUÉ archivo falló y su tipo clasificado para ofrecer
    /// reparación/cuarentena en vez de un texto muerto.
    Welcome {
        error: Option<String>,
        failed_path: Option<PathBuf>,
        failed_kind: Option<canvas_io::CorruptionKind>,
    },
    Loading {
        path: PathBuf,
    },
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
    /// Abrir un vídeo del Download en un lienzo nuevo del tamaño elegido,
    /// con trim/blur/zoom aplicados.
    OpenVideo {
        accept: crate::ytdlp::VideoAccept,
    },
    /// Un canvas independiente por cada recorte de la edición.
    OpenVideos {
        accepts: Vec<crate::ytdlp::VideoAccept>,
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
    /// El diálogo de ruta pertenece al canvas activo; impide cambiarlo.
    pub(super) save_path_dialog_open: bool,
    /// Guardar solicitado desde el diálogo de cierre.
    pub(super) save_requested: bool,
    /// Cerrar la ventana en cuanto termine el guardado en curso.
    pub(super) close_after_save: bool,
    /// El usuario ya confirmó el cierre: no volver a preguntar.
    pub(super) allow_close: bool,
    /// Navegación pendiente para cuando termine el guardado en curso.
    pub(super) after_save: Option<Nav>,
    /// Rutas cuya sobrescritura destructiva ya confirmó el usuario. Por
    /// ARCHIVO, no por sesión: confirmar «Overwrite» en `foto.png` no autoriza
    /// a pisar `retrato.jpg`, y el autoguardado escribe sin preguntar, así que
    /// una bandera de sesión se convertiría en un borrado sin aviso.
    pub(super) overwrite_confirmed: HashSet<PathBuf>,
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
    /// Rutas donde el usuario ya confirmó «Save anyway» para un documento sin
    /// capas de imagen (mismo criterio que `overwrite_confirmed`: por archivo).
    pub(super) discard_raster_confirmed: HashSet<PathBuf>,
    /// Archivos que escribió la PROPIA app en esta sesión (una imagen del bulk,
    /// un «Save as», un lienzo nuevo materializado, un duplicado): no hay
    /// píxeles originales del usuario que un guardado en el sitio pueda
    /// destruir, así que el primer guardado no pide la confirmación
    /// destructiva. Por ruta y por sesión: un archivo que ya existía antes no
    /// está aquí y se sigue preguntando una vez.
    pub(super) app_written: HashSet<PathBuf>,
    /// Por qué el autoguardado no está escribiendo ahora mismo (motivo ya en
    /// inglés para la barra de estado). `None` = nada lo bloquea; se limpia
    /// sola cada frame en `autosave::tick`.
    pub(super) autosave_blocked: Option<&'static str>,
    /// Temporizador del autoguardado silencioso del lienzo activo (ver
    /// `app/autosave.rs`): un contador por ventana.
    pub(super) autosave: autosave::Autosave,
    /// La cola ÚNICA de escritura (ver `app/write_queue.rs`): los clips
    /// recién añadidos y cualquier lienzo de fondo que quedó sucio al cambiar
    /// de baraja. Uno a uno, sin cambiar de lienzo ni esperar a `Ctrl+S`.
    pub(super) writes: write_queue::WriteQueue,
    /// Ranuras que fallaron al escribirse de fondo, con la revisión del
    /// historial en la que fallaron: no se reintentan hasta que el usuario
    /// edite algo (`app/write_queue.rs`).
    pub(super) blocked_writes: HashMap<u64, u64>,
    /// Cuándo arrancó la última escritura de la cola: el margen entre dos
    /// escrituras de fondo consecutivas (`write_queue::MIN_GAP`).
    pub(super) last_write: Option<std::time::Instant>,
    /// El guardado en vuelo lo lanzó el autoguardado: su resultado alimenta el
    /// backoff del temporizador. Un `Ctrl+S` manual no toca este estado, para
    /// que un fallo suyo no se anuncie como «fallo del autoguardado».
    pub(super) autosave_in_flight: bool,
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

impl SaveFlow {
    /// ¿El usuario ya autorizó sobrescribir ESTE archivo? Los dos avisos
    /// destructivos se preguntan una vez por ruta: la respuesta no viaja a
    /// otro canvas (el autoguardado la aplicaría sin preguntar).
    pub(super) fn overwrite_ok(&self, path: &Path) -> bool {
        self.overwrite_confirmed.contains(path)
    }

    /// Igual que `overwrite_ok`, para el aviso de «se borró la última foto».
    pub(super) fn discard_raster_ok(&self, path: &Path) -> bool {
        self.discard_raster_confirmed.contains(path)
    }

    pub(super) fn confirm_overwrite(&mut self, path: &Path) {
        self.overwrite_confirmed.insert(path.to_path_buf());
    }
    pub(super) fn confirm_discard_raster(&mut self, path: &Path) {
        self.discard_raster_confirmed.insert(path.to_path_buf());
    }

    /// ¿Este archivo lo escribió la app? Entonces no hay original que
    /// proteger: el autoguardado (y el primer `Ctrl+S`) no pregunta.
    pub(super) fn is_app_written(&self, path: &Path) -> bool {
        self.app_written.contains(path)
    }

    pub(super) fn mark_app_written(&mut self, path: &Path) {
        self.app_written.insert(path.to_path_buf());
    }
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
    /// para apilar `GlobalStep::Delete` con las rutas devueltas por el worker.
    pub(super) undoable_deletes: std::collections::HashSet<PathBuf>,
    /// Primer lienzo creado por un bulk web, pendiente de mostrar: en cuanto
    /// el reescaneo lo incorpora a la baraja se salta a él (si no, el usuario
    /// no vería dónde cayeron).
    pub(super) bulk_jump: Option<PathBuf>,
    /// Tanda entera creada por el último bulk web: el reescaneo final
    /// conserva la marca `thumb_failed` de un archivo que un escaneo
    /// intermedio vio vacío (reserva previa al bake) si el `mtime` no
    /// cambió, así que tras el `merge_scan` se pide la miniatura de cada
    /// creado que siga sin ella — y la ruta NO sale de la tanda hasta que la
    /// baraja la tiene. `bulk_thumb_rounds` acota los reintentos (un archivo
    /// que nunca podrá dar miniatura no debe spawnear un hilo en cada
    /// reescaneo de la carpeta).
    pub(super) bulk_created: Vec<PathBuf>,
    /// Pasadas de reintento gastadas por la tanda actual de `bulk_created`.
    pub(super) bulk_thumb_rounds: u8,
    /// Intentos de miniatura ya gastados por ruta: una miniatura que falló o
    /// que nunca llegó se reintenta sola desde el editor, sin esperar a otro
    /// reescaneo de la carpeta (ver `DeckOps::claim_missing_thumbs`).
    pub(super) thumb_retries: std::collections::HashMap<PathBuf, u8>,
    /// Desde cuándo cada ranura se ve sin miniatura: una petición que sigue en
    /// vuelo no se duplica, pero una que nunca llegó no gira para siempre.
    pub(super) thumb_waiting: std::collections::HashMap<PathBuf, std::time::Instant>,
    /// Momento del próximo reintento de miniaturas (uno por tanda, no uno por
    /// archivo y frame).
    pub(super) thumb_retry_at: Option<std::time::Instant>,
}

/// Cada cuánto se reintenta una miniatura de la baraja que falta. Un archivo
/// que el escaneo pilló a medio escribir (el caso real: el PRIMER lienzo de un
/// bulk, cuya miniatura se pide mientras el worker todavía está guardando) ya
/// está completo para el siguiente intento.
const THUMB_RETRY_EVERY: std::time::Duration = std::time::Duration::from_secs(2);

/// Espera antes de pedir una miniatura que aún no falló: el escaneo de la
/// carpeta la está generando en paralelo y duplicarla no adelanta nada.
const THUMB_WAIT: std::time::Duration = std::time::Duration::from_secs(6);

/// Intentos por archivo: pasado el tope se deja el icono de aviso (o el
/// spinner, si nunca falló) en la tira. Sin tope, un PNG que nunca podrá dar
/// miniatura lanzaría un hilo cada dos segundos durante toda la sesión.
const THUMB_RETRIES: u8 = 3;

impl DeckOps {
    /// Rutas de la baraja que se han quedado sin miniatura y a las que toca
    /// pedir una. Devuelve las rutas (ya contadas) para que el llamador lance
    /// `loader::spawn_single_thumb`, con tope por tanda.
    ///
    /// Existe porque la tira no tenía ninguna forma de recuperar una miniatura
    /// que se perdiera: el reintento del bulk solo corría en un reescaneo de la
    /// carpeta, y con los tres gastados en la misma tanda (los escaneos llegan
    /// seguidos tras crear los lienzos) el primero se quedaba sin preview para
    /// siempre — hasta volver a la galería y reabrir. Aquí el reintento es del
    /// propio editor: reloj acotado, intentos acotados, y los contadores se
    /// olvidan en cuanto la ranura tiene su miniatura.
    ///
    /// Cubre los dos estados rotos de la tira: la que falló (`thumb_failed`) y
    /// la que se quedó esperando una petición que nunca llegó (sin fallo
    /// registrado, tras `THUMB_WAIT`).
    pub(super) fn claim_missing_thumbs(
        &mut self,
        deck: &deck::Deck,
        now: std::time::Instant,
        limit: usize,
    ) -> Vec<PathBuf> {
        let waiting: Vec<(&std::path::Path, bool)> = deck
            .slots
            .iter()
            .filter(|slot| slot.thumb.is_none() && !slot.is_placeholder)
            .map(|slot| (slot.path.as_path(), slot.thumb_failed))
            .collect();
        let still_waiting = |path: &PathBuf| waiting.iter().any(|(w, _)| *w == path.as_path());
        self.thumb_retries.retain(|path, _| still_waiting(path));
        self.thumb_waiting.retain(|path, _| still_waiting(path));
        if waiting.is_empty() {
            self.thumb_retry_at = None;
            return Vec::new();
        }
        if self.thumb_retry_at.is_some_and(|at| now < at) {
            return Vec::new();
        }
        self.thumb_retry_at = Some(now + THUMB_RETRY_EVERY);
        let mut claimed = Vec::new();
        for (path, failed) in waiting {
            if claimed.len() >= limit {
                break;
            }
            let seen = *self.thumb_waiting.entry(path.to_path_buf()).or_insert(now);
            // Sin fallo registrado la petición puede seguir en vuelo (la está
            // generando el escaneo): se espera antes de duplicarla.
            if !failed && now.duration_since(seen) < THUMB_WAIT {
                continue;
            }
            let tries = self.thumb_retries.entry(path.to_path_buf()).or_insert(0);
            if *tries >= THUMB_RETRIES {
                continue;
            }
            *tries += 1;
            claimed.push(path.to_path_buf());
        }
        claimed
    }
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
        menus.set_language(inner.settings.language);
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
        // La persistencia es en segundo plano: esperar la última revisión
        // para no perderla al morir el proceso (A10). Con tope, para no
        // colgar la salida si el disco está atascado.
        crate::settings::flush_settings(std::time::Duration::from_secs(5));
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
        if reg > 0 || reup > 0 || self.atlas_log_frames.is_multiple_of(60) {
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
