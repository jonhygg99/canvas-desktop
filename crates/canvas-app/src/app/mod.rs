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
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

use anyhow::{Context as _, Result};
use canvas_render::CanvasRenderer;
use eframe::egui;

use crate::loader::AppMsg;
use crate::{deck, editor, export, gallery, menus, paste_hook, settings};

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
    /// Ctrl+V con bitmap (hook de mensajes del SO): se consume en la ventana
    /// enfocada.
    pub(crate) paste_this_frame: bool,
    /// Foco pendiente para una ventana recién CREADA: el viewport de la
    /// ventana no existe hasta `spawn_child_viewports` del mismo frame, así
    /// que `focus_workspace` inmediato perdería el comando. Se consume en
    /// `root_frame` justo después de crear las hijas.
    pub(crate) pending_focus: Option<usize>,
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
    /// «Save all»: ids (estables) de las ranuras sucias que faltan por
    /// guardar, el activo excluido.
    pub(super) save_all_queue: Vec<u64>,
    /// Ya se pulsó «Guardar» para la ranura al frente de `save_all_queue`.
    pub(super) save_all_attempted: bool,
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
    pub(crate) fn new(
        cc: &eframe::CreationContext<'_>,
        initial_path: Option<PathBuf>,
        instance: Option<canvas_shell::InstanceListener>,
    ) -> Result<Self> {
        // egui 0.35 solo trae Ubuntu-Light: `RichText::strong()` solo cambia
        // el COLOR, no el grosor. Para títulos de verdad en negrita (secciones
        // del panel de propiedades) registramos la variante Bold de la misma
        // familia Ubuntu y la usamos vía `FontFamily::Name("Ubuntu-Bold")`.
        {
            let mut fonts = egui::FontDefinitions::default();
            fonts.font_data.insert(
                "Ubuntu-Bold".to_owned(),
                egui::FontData::from_static(include_bytes!("../../assets/fonts/Ubuntu-Bold.ttf"))
                    .into(),
            );
            fonts.families.insert(
                egui::FontFamily::Name("Ubuntu-Bold".into()),
                vec!["Ubuntu-Bold".to_owned()],
            );
            cc.egui_ctx.set_fonts(fonts);
        }

        let rs = cc
            .wgpu_render_state
            .as_ref()
            .context("eframe no ha inicializado wgpu (¿backend glow activo?)?")?
            .clone();
        let renderer = CanvasRenderer::new(&rs.device)?;
        let (shell_tx, shell_rx) = channel();

        // Rutas de segundas instancias: un hilo acepta conexiones del socket
        // local y las convierte en mensajes para la UI (canal global, no de
        // ningún workspace: se resuelven contra la ventana ENFOCADA).
        if let Some(listener) = instance {
            let tx = shell_tx.clone();
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
        // Vive en `App` (fuera del `Arc` compartido), no en `AppInner`.
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

        let settings = settings::AppSettings::load();
        if let Some(m) = native_menus.as_mut() {
            m.set_recents(&settings.recent_files);
        }

        let ws0 = Arc::new(Mutex::new(Workspace::new(egui::ViewportId::ROOT)));
        let mut inner = AppInner {
            workspaces: vec![Arc::clone(&ws0)],
            me: None,
            renderer,
            rs,
            settings,
            thumb_cache: window::thumbnail_cache_dir(),
            shell_status: String::new(),
            menu_mirror: MenuMirror::default(),
            applied_theme: None,
            switcher: switcher::SwitcherState::default(),
            focused: 0,
            shell_rx,
            shell_tx,
            next_ws_id: 1,
            last_workspaces_save: None,
            workspaces_dirty: false,
            paste_this_frame: false,
            pending_focus: None,
        };

        // La app arranca SIEMPRE en la home (bienvenida) de la única ventana
        // raíz, con su tamaño por defecto: ni las ventanas hijas ni las
        // carpetas/documentos de la sesión anterior se restauran. La lista
        // sigue persistiéndose en `settings.json` como historial, pero aquí
        // se descarta. Una ruta explícita (argv o «Abrir con» del SO) sí abre.
        let _ = std::mem::take(&mut inner.settings.workspaces);

        let path_to_open = initial_path;
        if let Some(path) = path_to_open {
            if path.exists() {
                let ws0 = Arc::clone(&inner.workspaces[0]);
                let mut ws0 = ws0.lock().unwrap();
                inner.open_path(&mut ws0, path, &cc.egui_ctx);
            } else {
                tracing::info!("ruta inicial inexistente, se ignora: {}", path.display());
            }
        }
        inner.persist_workspaces_now();

        // El Arc que comparten las ventanas; se patcha `me` con el Arc
        // definitivo una vez creado (`None` mientras se construye; nadie lo
        // lee hasta el primer frame, y las ventanas hijas nacen en él).
        let inner_arc = Arc::new(Mutex::new(inner));
        inner_arc.lock().unwrap().me = Some(Arc::clone(&inner_arc));
        Ok(Self {
            inner: inner_arc,
            menus: native_menus,
        })
    }
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
            let mut ws0 = ws0.lock().unwrap();
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
            let ws0 = ws0.lock().unwrap();
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
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let mut inner = self.inner.lock().unwrap();
        App::poll_native_menu(&self.menus, &mut inner, &ctx);
        inner.root_frame(ui, &ctx);
        App::sync_native_menu(&mut self.menus, &mut inner);
    }

    /// Al salir (cierre de la raíz, Alt+F4 del SO) se persiste el estado de
    /// las ventanas como historial en `settings.json`; ya no se restaura al
    /// arrancar: la app siempre abre en la home.
    fn on_exit(&mut self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.persist_workspaces_now();
        }
    }
}

impl AppInner {
    /// Crea un workspace nuevo (ventana) con la bienvenida y lo añade a la
    /// lista. `from_root` indica si el usuario lo pidió desde el menú de una
    /// ventana (se persiste ya) o es uno restaurado del arranque.
    pub(crate) fn new_workspace(&mut self) -> Arc<Mutex<Workspace>> {
        let id = self.next_ws_id;
        self.next_ws_id += 1;
        let viewport = egui::ViewportId::from_hash_of(("canvas-ws", id));
        let ws = Arc::new(Mutex::new(Workspace::new(viewport)));
        self.workspaces.push(Arc::clone(&ws));
        // La persistencia se difiere al final del frame raíz: llamar aquí
        // `persist_workspaces_now` bloquearía TODOS los workspaces, y si
        // este método se invoca con el del frame actual ya lockeado (menú
        // «New Window», Ctrl+N/Ctrl+T) el hilo se bloquearía a sí mismo.
        self.workspaces_dirty = true;
        ws
    }

    /// Persiste la lista de workspaces (documento activo + geometría) en los
    /// ajustes. Se llama al crear/cerrar una ventana, con throttle para la
    /// geometría en vivo, y en `on_exit`.
    pub(crate) fn persist_workspaces_now(&mut self) {
        self.settings.workspaces = self
            .workspaces
            .iter()
            .map(|ws| {
                let ws = ws.lock().unwrap();
                settings::StoredWorkspace {
                    path: ws.persisted_path(),
                    pos: ws.geometry.map(|(p, _)| [p.x, p.y]),
                    size: ws.geometry.map(|(_, s)| [s.x, s.y]),
                }
            })
            .collect();
        self.settings.save_in_background();
        self.last_workspaces_save = Some(std::time::Instant::now());
    }

    /// Persiste la geometría con un throttle corto (no spam de hilos de
    /// escritura por cada píxel de un arrastre de ventana).
    pub(crate) fn maybe_persist_workspaces(&mut self) {
        let due = self
            .last_workspaces_save
            .is_none_or(|t| t.elapsed() >= std::time::Duration::from_secs(2));
        if due {
            self.persist_workspaces_now();
        }
    }

    /// Libera los recursos GPU del workspace que se cierra y lo retira de la
    /// lista. Solo para ventanas hijas (la raíz no se retira nunca).
    pub(crate) fn close_workspace(&mut self, index: usize) {
        debug_assert!(index > 0, "la raíz no se cierra por esta vía");
        // La textura nativa (register_native_texture) vive en el registry del
        // renderer COMPARTIDO: hay que liberarla a mano o la GPU perdería
        // slots con cada ventana cerrada.
        let ws = self.workspaces.remove(index);
        if let Ok(ws) = ws.lock() {
            if let Some(surface) = &ws.surface {
                self.rs.renderer.write().free_texture(&surface.egui_id());
            }
        }
        if self.focused >= self.workspaces.len() {
            self.focused = 0;
        }
        self.persist_workspaces_now();
    }

    /// Enfoca (trae al frente) la ventana de un workspace.
    pub(crate) fn focus_workspace(&mut self, idx: usize, ctx: &egui::Context) {
        let Some(ws_arc) = self.workspaces.get(idx) else {
            return;
        };
        self.focused = idx;
        let viewport = ws_arc.lock().unwrap().viewport;
        ctx.send_viewport_cmd_to(viewport, egui::ViewportCommand::Focus);
        // La ventana objetivo se repinta sola al recibir el foco; la paleta
        // se cierra en el frame siguiente.
        ctx.request_repaint_of(viewport);
    }

    /// Refresca `self.focused` contra el estado real de las ventanas (el SO
    /// manda: Alt+Tab etc. no pasan por aquí).
    fn update_focused(&mut self, ctx: &egui::Context) {
        self.focused = ctx
            .input(|i| {
                i.raw
                    .viewports
                    .iter()
                    .find(|(_, info)| info.focused == Some(true))
                    .and_then(|(id, _)| {
                        self.workspaces
                            .iter()
                            .position(|ws| ws.lock().unwrap().viewport == *id)
                    })
                    .unwrap_or(0)
            })
            .min(self.workspaces.len().saturating_sub(1));
    }

    /// El frame de cada arranque: tema, foco, mensajes, la ventana raíz y
    /// la creación de las hijas.
    fn root_frame(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if self.applied_theme != Some(self.settings.theme) {
            ctx.set_theme(self.settings.theme.to_egui());
            self.applied_theme = Some(self.settings.theme);
        }
        self.update_focused(ctx);
        self.paste_this_frame = paste_hook::take_request();
        self.drain_all(ctx);

        let ws0_arc = Arc::clone(&self.workspaces[0]);
        let mut ws0 = ws0_arc.lock().unwrap();
        let paste = self.paste_this_frame;
        self.ws_frame(ui, ctx, &mut ws0, 0, true, paste);
        drop(ws0);

        self.spawn_child_viewports(ctx);
        if let Some(idx) = self.pending_focus.take() {
            self.focus_workspace(idx, ctx);
        }
        self.finish_root_frame(ctx);
    }

    /// Tras los frames: retira las ventanas que pidieron cerrarse y
    /// persiste (throttleado) la geometría.
    fn finish_root_frame(&mut self, ctx: &egui::Context) {
        let mut changed = false;
        let mut i = 1;
        while i < self.workspaces.len() {
            if self.workspaces[i].lock().unwrap().close_requested {
                self.close_workspace(i);
                changed = true;
            } else {
                i += 1;
            }
        }
        if changed {
            // El conmutador pudo quedar apuntando a una ventana retirada.
            if self.focused >= self.workspaces.len() {
                self.focused = 0;
            }
            self.switcher.selected = self
                .switcher
                .selected
                .min(self.workspaces.len().saturating_sub(1));
            let _ = ctx;
        }
        if self.workspaces_dirty {
            self.workspaces_dirty = false;
            self.persist_workspaces_now();
        }
        self.maybe_persist_workspaces();
    }

    /// Crea (o mantiene) las ventanas hijas con `show_viewport_deferred`.
    /// Hay que llamarla cada frame con los MISMOS `ViewportId`, o la ventana
    /// se cierra.
    fn spawn_child_viewports(&self, ctx: &egui::Context) {
        if self.workspaces.len() <= 1 {
            return;
        }
        let me = Arc::clone(
            self.me
                .as_ref()
                .expect("me se parchea en App::new antes del primer frame"),
        );
        for (i, ws_arc) in self.workspaces.iter().enumerate().skip(1) {
            let ws = ws_arc.lock().unwrap();
            if ws.close_requested {
                continue;
            }
            let viewport = ws.viewport;
            let mut builder = egui::ViewportBuilder::default().with_title(ws.label());
            if let Some((pos, size)) = ws.geometry {
                builder = builder
                    .with_position([pos.x, pos.y])
                    .with_inner_size([size.x, size.y]);
            }
            let ws_arc = Arc::clone(ws_arc);
            let child_ctx = ctx.clone();
            let me = Arc::clone(&me);
            let idx = i;
            ctx.show_viewport_deferred(viewport, builder, move |ui, class| {
                if class == egui::ViewportClass::Deferred {
                    let mut inner = me.lock().unwrap();
                    if let Some(ws_cur) = inner.workspaces.get(idx) {
                        if Arc::ptr_eq(ws_cur, &ws_arc) {
                            let mut ws = ws_arc.lock().unwrap();
                            inner.child_frame(ui, &child_ctx, &mut ws, idx);
                        }
                    }
                }
            });
        }
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
        let paste = paste_hook::take_request();
        self.ws_frame(ui, ctx, ws, idx, false, paste);
    }
}
