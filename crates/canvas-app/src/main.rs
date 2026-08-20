//! Binario de Canvas Desktop: ventana eframe/egui con el lienzo vello.

// Subsistema GUI solo en release: evita la consola negra que Windows abre
// detrás de la ventana al lanzar la app instalada desde el Explorador. En
// debug se mantiene la consola para seguir viendo los logs de `tracing`
// con `cargo run`.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod clipboard;
mod deck;
mod deck_strip;
mod editor;
mod export;
mod gallery;
mod layers_panel;
mod loader;
mod menus;
mod paste_hook;
mod settings;
mod surface;
mod watcher;
mod welcome;

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};

use anyhow::{anyhow, Context, Result};
use canvas_render::CanvasRenderer;
use canvas_shell::ShellIntegration as _;
use eframe::egui;
use eframe::egui_wgpu::RenderState;
use loader::AppMsg;
use surface::CanvasSurface;

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,wgpu_core=warn,wgpu_hal=warn".into()),
        )
        .init();

    // Flags headless para el instalador: registran/quitan la integración con
    // el Explorador sin abrir ventana, sin tocar la instancia única. Deben
    // interceptarse antes que cualquier otra cosa en main.
    if let Some(register) = shell_registration_flag(std::env::args()) {
        let shell = canvas_shell::platform();
        let exe =
            std::env::current_exe().context("no se pudo resolver la ruta del ejecutable actual")?;
        if register {
            shell
                .register_file_associations(&exe)
                .map_err(|e| anyhow!("registro de integración con el Explorador fallido: {e}"))?;
            println!("Explorer integration registered.");
        } else {
            shell.unregister_file_associations().map_err(|e| {
                anyhow!("desregistro de integración con el Explorador fallido: {e}")
            })?;
            println!("Explorer integration removed.");
        }
        return Ok(());
    }

    // Identidad ante la barra de tareas (Jump List); antes de crear la ventana.
    canvas_shell::set_app_identity();

    let initial_paths = canvas_shell::open_paths_from_args(std::env::args());

    // Instancia única: si ya hay una app viva, se le envían las rutas por el
    // socket local y este proceso sale con código 0.
    let instance = match canvas_shell::acquire_instance(&initial_paths) {
        canvas_shell::InstanceRole::Secondary => {
            tracing::info!("instancia ya abierta: rutas reenviadas, saliendo");
            return Ok(());
        }
        canvas_shell::InstanceRole::Primary(listener) => Some(listener),
        canvas_shell::InstanceRole::Standalone => None,
    };
    let initial_path = initial_paths.into_iter().next();

    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([1280.0, 800.0])
        .with_min_inner_size([640.0, 480.0]);
    if let Some(icon) = load_app_icon() {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport,
        event_loop_builder: Some(Box::new(paste_hook::install)),
        ..Default::default()
    };

    eframe::run_native(
        "Canvas Desktop",
        options,
        Box::new(move |cc| Ok(Box::new(App::new(cc, initial_path, instance)?))),
    )
    .map_err(|e| anyhow!("no se pudo arrancar la ventana: {e}"))
}

/// Icono de la ventana (barra de título/alt-tab), generado desde
/// `assets/icon.svg` por `cargo run -p canvas-render --example gen_icons`.
const APP_ICON_PNG: &[u8] =
    include_bytes!("../../../assets/linux/hicolor/256x256/apps/canvas-desktop.png");

fn load_app_icon() -> Option<egui::IconData> {
    let img = image::load_from_memory(APP_ICON_PNG).ok()?.into_rgba8();
    let (width, height) = img.dimensions();
    Some(egui::IconData {
        rgba: img.into_raw(),
        width,
        height,
    })
}

/// Busca `--register-shell`/`--unregister-shell` en argv; usado por el
/// instalador NSIS (`nsExec`) para escribir/limpiar el registro sin abrir la
/// app. `Some(true)` = registrar, `Some(false)` = quitar, `None` = ninguno.
fn shell_registration_flag(args: impl Iterator<Item = String>) -> Option<bool> {
    for arg in args {
        match arg.as_str() {
            "--register-shell" => return Some(true),
            "--unregister-shell" => return Some(false),
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::shell_registration_flag;

    fn args(items: &[&str]) -> impl Iterator<Item = String> {
        items
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
            .into_iter()
    }

    #[test]
    fn detects_register_flag() {
        assert_eq!(
            shell_registration_flag(args(&["canvas-desktop.exe", "--register-shell"])),
            Some(true)
        );
    }

    #[test]
    fn detects_unregister_flag() {
        assert_eq!(
            shell_registration_flag(args(&["canvas-desktop.exe", "--unregister-shell"])),
            Some(false)
        );
    }

    #[test]
    fn ignores_unrelated_args() {
        assert_eq!(
            shell_registration_flag(args(&["canvas-desktop.exe", "C:\\photo.png"])),
            None
        );
    }
}

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
    NewDesign,
}

struct App {
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
}

impl App {
    fn new(
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
            thumb_cache: thumbnail_cache_dir(),
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
        };
        if let Some(m) = app.menus.as_mut() {
            m.set_recents(&app.settings.recent_files);
        }
        if let Some(path) = initial_path {
            app.open_path(path, &cc.egui_ctx);
        }
        Ok(app)
    }

    /// Punto único de entrada para abrir algo, venga de argv, diálogo,
    /// arrastrar y soltar, un clic en la galería o una segunda instancia.
    fn open_path(&mut self, path: PathBuf, ctx: &egui::Context) {
        // Un sidecar `foto.png.canvas` se abre como su imagen `foto.png`
        // (que a su vez restaura las capas del sidecar automáticamente).
        let path = resolve_canvas_sidecar(path);
        if path.is_dir() {
            // Si la baraja ya tenía esta misma carpeta (se volvió del
            // editor), siembra la rejilla con sus miniaturas ya en GPU: el
            // reescaneo que sigue solo detecta cambios en disco, no repuebla
            // desde ⏳.
            let gallery_state =
                seed_gallery_from_deck(&self.deck, path.clone(), self.settings.gallery_sort);
            loader::spawn_gallery_scan(
                path.clone(),
                self.thumb_cache.clone(),
                self.tx.clone(),
                ctx.clone(),
            );
            self.push_recent(&path);
            self.view = View::Gallery(gallery_state);
        } else if canvas_io::is_canvas_file(&path) {
            // Diseño autónomo: el `.canvas` ES el documento. Qué baraja usar
            // (semilla de galería, la ya activa, o una degenerada de una
            // ranura) se decide en `resolve_deck` cuando la carga termine.
            loader::spawn_load_design(path.clone(), self.tx.clone(), ctx.clone());
            self.push_recent(&path);
            self.view = View::Loading { path };
        } else if canvas_io::is_image_file(&path) {
            loader::spawn_load_image(path.clone(), true, self.tx.clone(), ctx.clone());
            self.push_recent(&path);
            self.view = View::Loading { path };
        } else {
            self.view = View::Welcome {
                error: Some(format!(
                    "\"{}\" is not a supported image format.",
                    path.display()
                )),
            };
        }
        self.sync_title(ctx);
    }

    /// Apunta lo abierto en los recientes: ajustes, menú y Jump List del SO.
    fn push_recent(&mut self, path: &std::path::Path) {
        let path = path.to_owned();
        self.settings.recent_files.retain(|p| p != &path);
        self.settings.recent_files.insert(0, path);
        self.settings.recent_files.truncate(10);
        self.settings.save_in_background();
        if let Some(m) = self.menus.as_mut() {
            m.set_recents(&self.settings.recent_files);
        }
        // La Jump List usa COM: hilo aparte, mejor esfuerzo.
        let recents = self.settings.recent_files.clone();
        std::thread::spawn(move || {
            if let Err(e) = canvas_shell::platform().update_jump_list(&recents) {
                tracing::debug!("jump list no actualizada: {e}");
            }
        });
    }

    /// Relanza el escaneo de la carpeta actualmente abierta en la galería
    /// (tras crear/duplicar/pegar un archivo). `GalleryState::merge_files`
    /// conserva las miniaturas ya cargadas, así que esto es casi gratis.
    fn rescan_gallery(&mut self, ctx: &egui::Context) {
        if let View::Gallery(g) = &self.view {
            loader::spawn_gallery_scan(
                g.folder.clone(),
                self.thumb_cache.clone(),
                self.tx.clone(),
                ctx.clone(),
            );
        }
    }

    /// Recuerda el tamaño de página para el próximo diseño nuevo, sin
    /// escribir ajustes si no cambió (`save_in_background` lanza un hilo).
    fn remember_page_size(&mut self, doc: &canvas_core::Document) {
        let Ok(page) = doc.page() else { return };
        let size = (page.width, page.height);
        if self.settings.last_page_size != size {
            self.settings.last_page_size = size;
            self.settings.save_in_background();
        }
    }

    /// Documento nuevo en blanco (desde la bienvenida o el menú File):
    /// hereda el tamaño de página del último documento abierto o creado, y
    /// nace en el formato elegido en Ajustes (`new_canvas_format`).
    fn new_design(&mut self, ctx: &egui::Context) {
        self.deck = deck::Deck::default();
        self.apply_deck_prefs();
        let (w, h) = self.settings.last_page_size;
        let state = if self.settings.new_canvas_format == settings::NewCanvasFormat::Canvas {
            let mut state = editor::EditorState::new_blank(w, h);
            // Sin efecto real (`is_design` ignora `sidecar_enabled`), pero
            // deja el checkbox del panel en el valor que el usuario espera
            // si en algún momento deja de ser un diseño autónomo.
            state.sidecar_enabled = self.settings.sidecar_default;
            state
        } else {
            // `new_blank_image` fuerza `sidecar_enabled = true` — NO se
            // sobrescribe con `sidecar_default` aquí: un raster en blanco sin
            // sidecar perdería sus capas en el primer guardado.
            editor::EditorState::new_blank_image(w, h)
        };
        self.view = View::Editor(Box::new(state));
        self.sync_title(ctx);
    }

    /// Decide qué baraja usar al terminar de cargar `path` en el editor: la
    /// semilla que dejó un clic de galería (`pending_deck`), la baraja ya
    /// activa si `path` es uno de sus lienzos (navegación por la tira o el
    /// teclado dentro del propio editor, que no toca `pending_deck`), o una
    /// baraja degenerada de una sola ranura en cualquier otro caso (CLI,
    /// recientes, arrastrar y soltar, segunda instancia).
    fn resolve_deck(&mut self, path: &Path, ctx: &egui::Context) {
        if let Some(seed) = self.pending_deck.take() {
            self.deck = deck::Deck::from_seed(seed, path);
            self.apply_deck_prefs();
            // La baraja acaba de nacer con `folder` ya puesto: a diferencia
            // del sondeo que lanza la galería (demasiado pronto, antes de
            // que esta `Deck` existiera), este llega a tiempo.
            self.spawn_deck_probe(ctx);
        } else if let Some(idx) = self.deck.find_by_path(path) {
            self.deck.active = idx;
        } else {
            self.deck = deck::Deck::single(path.to_path_buf());
            self.apply_deck_prefs();
        }
    }

    /// Siembra una `Deck` recién construida con las preferencias persistidas
    /// (eje de apilado, visibilidad de la tira) — `Deck::single`/`from_seed`
    /// no conocen `AppSettings`, así que el llamador las aplica justo
    /// después de construirla, antes del primer `relayout`.
    fn apply_deck_prefs(&mut self) {
        self.deck.axis = self.settings.deck_axis;
        self.deck.strip_visible = self.settings.deck_strip_visible;
        self.deck.strip_side = self.settings.deck_strip_side;
    }

    /// Sondea el tamaño real de las ranuras cuyo tamaño aún se desconoce.
    /// No hace nada con una baraja degenerada (`Deck::single`: sin carpeta,
    /// sin hermanos que necesiten sondeo) ni cuando ya se conocen todos.
    fn spawn_deck_probe(&self, ctx: &egui::Context) {
        let Some(folder) = self.deck.folder.clone() else {
            return;
        };
        let paths: Vec<PathBuf> = self
            .deck
            .slots
            .iter()
            .filter(|s| s.page.is_none())
            .map(|s| s.path.clone())
            .collect();
        if paths.is_empty() {
            return;
        }
        loader::spawn_deck_probe(folder, paths, self.tx.clone(), ctx.clone());
    }

    /// Alterna el eje de apilado de la baraja activa y lo persiste — la tira
    /// (botón ⇅/⇆) y el menú View (Fase 14e) comparten este único camino.
    fn toggle_deck_axis(&mut self) {
        self.deck.axis = self.deck.axis.toggled();
        self.deck.layout_dirty = true;
        self.settings.deck_axis = self.deck.axis;
        self.settings.save_in_background();
    }

    /// Mueve la tira al siguiente lado y lo persiste — el botón de la propia
    /// tira y el menú View comparten este único camino.
    fn cycle_strip_side(&mut self) {
        self.deck.strip_side = self.deck.strip_side.cycled();
        self.settings.deck_strip_side = self.deck.strip_side;
        self.settings.save_in_background();
    }

    /// Añade un lienzo en blanco al final de la baraja y salta a él. No hace
    /// nada con una baraja degenerada (`Deck::single`: un archivo suelto no
    /// tiene carpeta donde crear un hermano) — es el único camino a esta
    /// función cuando la tira está oculta (un solo archivo en la carpeta,
    /// donde la celda "+" todavía no existe).
    fn add_canvas(&mut self) {
        let ext = self.settings.new_canvas_format.extension();
        match self
            .deck
            .push_placeholder(self.settings.last_page_size, ext)
        {
            Some(idx) => {
                self.deck.jump_to = Some(idx);
                self.deck.jump_center = true;
            }
            None => tracing::info!("«Add canvas» sin efecto: la baraja no tiene carpeta"),
        }
    }

    fn navigate(&mut self, nav: Nav, ctx: &egui::Context) {
        match nav {
            Nav::Open(path) => self.open_path(path, ctx),
            Nav::NewDesign => self.new_design(ctx),
        }
    }

    /// Nombres de todos los lienzos con cambios sin guardar — la activa
    /// primero, si lo está, luego el resto de la baraja — para los diálogos
    /// de «cambios sin guardar». Desde que hay N lienzos cargados a la vez
    /// (Fase 14c), un solo `state.is_dirty()` ya no cuenta la historia
    /// entera: una ranura de fondo puede estar sucia sin que el documento
    /// activo lo esté.
    fn dirty_canvas_names(&self) -> Vec<String> {
        let View::Editor(state) = &self.view else {
            return Vec::new();
        };
        let mut names = Vec::new();
        if state.is_dirty() {
            names.push(state.file_name());
        }
        for slot in &self.deck.slots {
            if matches!(&slot.content, deck::SlotContent::Ready(d) if d.history.is_dirty()) {
                names.push(slot.name.clone());
            }
        }
        names
    }

    /// Navega, pero si hay algún lienzo con cambios sin guardar delante
    /// pregunta primero (Save / Discard / Cancel). «Save» solo guarda el
    /// documento ACTIVO — sigue siendo el único camino de guardado que
    /// funciona fuera del editor (`Ctrl+Alt+S`/«Save all» es una acción del
    /// editor, no de esta navegación); el texto lo dice explícitamente
    /// cuando hay más de un lienzo sucio, para que abrir algo distinto
    /// nunca pierda trabajo en silencio.
    fn request_nav(&mut self, nav: Nav, ctx: &egui::Context) {
        let names = self.dirty_canvas_names();
        if names.is_empty() {
            self.navigate(nav, ctx);
            return;
        }
        let target = match &nav {
            Nav::Open(p) => format!(
                "\"{}\"",
                p.file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| p.display().to_string())
            ),
            Nav::NewDesign => "a new design".to_owned(),
        };
        let description = if names.len() == 1 {
            format!(
                "\"{}\" has unsaved changes.\nSave them before opening {target}? (\"No\" discards them.)",
                names[0]
            )
        } else {
            format!(
                "{} canvases have unsaved changes:\n\u{2022} {}\n\nOpening {target} only saves \
                 the active one — the rest will be lost. Cancel and switch to them first if you \
                 want to keep their changes.",
                names.len(),
                names.join("\n\u{2022} ")
            )
        };
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
        match choice {
            rfd::MessageDialogResult::Yes => {
                self.save_requested = true;
                self.after_save = Some(nav);
            }
            rfd::MessageDialogResult::Custom(c) if c == "Save" => {
                self.save_requested = true;
                self.after_save = Some(nav);
            }
            rfd::MessageDialogResult::No => self.navigate(nav, ctx),
            rfd::MessageDialogResult::Custom(c) if c == "Discard" => self.navigate(nav, ctx),
            _ => {}
        }
    }

    /// «Save all»: encola las ranuras de fondo sucias (id estable, no
    /// índice — el orden puede cambiar entre frames). El documento ACTIVO,
    /// si está sucio, se guarda aparte y de inmediato (no necesita saltar);
    /// la cola solo lleva lo demás. Deja fuera SVG/GIF: no se pueden
    /// sobrescribir y un lote no tiene un destino automático razonable para
    /// ellos sin preguntar archivo por archivo — el usuario los guarda
    /// individualmente activándolos, donde `Ctrl+S` ya redirige a «Save as…».
    fn start_save_all(&mut self) {
        let View::Editor(state) = &mut self.view else {
            return;
        };
        if state.is_dirty()
            && state
                .doc
                .source_path
                .as_deref()
                .is_some_and(canvas_io::can_overwrite)
        {
            state.save_clicked = true;
        }
        self.save_all_queue = self
            .deck
            .slots
            .iter()
            .filter(|s| {
                // Una provisional sucia la escribe su propio camino de
                // materialización (que además le reserva un nombre antes de
                // guardar); dejarla entrar aquí sería una segunda escritura
                // sobre una ruta solo «asomada», nunca reservada.
                !s.is_placeholder
                    && matches!(&s.content, deck::SlotContent::Ready(d) if d.history.is_dirty())
                    && canvas_io::can_overwrite(&s.path)
            })
            .map(|s| s.id)
            .collect();
        self.save_all_attempted = false;
    }

    /// Traduce un clic de menú a la acción correspondiente.
    fn handle_menu_action(&mut self, action: menus::MenuAction, ctx: &egui::Context) {
        use menus::MenuAction as A;
        match action {
            // Desde una galería, Ctrl+N crea el diseño DENTRO de esa carpeta
            // en vez de un documento fantasma sin sitio en disco.
            A::NewDesign => {
                if let View::Gallery(g) = &self.view {
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
                } else {
                    self.request_nav(Nav::NewDesign, ctx);
                }
            }
            A::OpenFile => loader::spawn_pick_file(self.tx.clone(), ctx.clone()),
            A::OpenFolder => loader::spawn_pick_folder(self.tx.clone(), ctx.clone()),
            A::OpenRecent(path) => self.request_nav(Nav::Open(path), ctx),
            A::Save => {
                if let View::Editor(state) = &mut self.view {
                    state.save_clicked = true;
                }
            }
            A::SaveAs => {
                if let View::Editor(state) = &mut self.view {
                    state.save_as_clicked = true;
                }
            }
            A::SaveAll => self.start_save_all(),
            A::Export => {
                if let View::Editor(_) = &self.view {
                    self.export_dialog = Some(export::ExportDialog::default());
                }
            }
            A::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            A::Undo => {
                if let View::Editor(state) = &mut self.view {
                    state.undo();
                }
            }
            A::Redo => {
                if let View::Editor(state) = &mut self.view {
                    state.redo();
                }
            }
            A::ZoomIn => {
                if let View::Editor(state) = &mut self.view {
                    state.pending_zoom_factor = Some(1.25);
                }
            }
            A::ZoomOut => {
                if let View::Editor(state) = &mut self.view {
                    state.pending_zoom_factor = Some(0.8);
                }
            }
            A::FitToWindow => {
                if let View::Editor(state) = &mut self.view {
                    state.viewport.request_fit();
                }
            }
            A::ToggleGrid => {
                if let View::Editor(state) = &mut self.view {
                    state.show_grid = !state.show_grid;
                }
            }
            A::ToggleRulers => {
                if let View::Editor(state) = &mut self.view {
                    state.show_rulers = !state.show_rulers;
                }
            }
            A::NextCanvas => {
                if let View::Editor(state) = &mut self.view {
                    state.deck_nav = Some(editor::DeckNav::Next);
                }
            }
            A::PrevCanvas => {
                if let View::Editor(state) = &mut self.view {
                    state.deck_nav = Some(editor::DeckNav::Prev);
                }
            }
            A::ToggleCanvasesPanel => {
                self.deck.strip_visible = !self.deck.strip_visible;
                self.settings.deck_strip_visible = self.deck.strip_visible;
                self.settings.save_in_background();
            }
            A::ToggleCanvasesAxis => self.toggle_deck_axis(),
            A::CycleCanvasesSide => self.cycle_strip_side(),
            A::AddCanvas => self.add_canvas(),
            A::FullScreen => {
                let fullscreen = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!fullscreen));
            }
            A::Settings => self.show_settings = true,
            A::About => self.show_about = true,
            A::Cut => {
                if let View::Editor(state) = &mut self.view {
                    clipboard::cut(state);
                }
            }
            A::Copy => {
                if let View::Editor(state) = &self.view {
                    clipboard::copy(state);
                }
            }
            A::Paste => {
                if let View::Editor(state) = &mut self.view {
                    if !clipboard::paste(state) {
                        state.save_error = Some(clipboard::PASTE_EMPTY_MSG.to_owned());
                    }
                }
            }
            A::Duplicate => {
                if let View::Editor(state) = &mut self.view {
                    clipboard::duplicate(state);
                }
            }
            A::Delete => {
                if let View::Editor(state) = &mut self.view {
                    clipboard::delete_selected(state);
                }
            }
            A::SelectAll => {
                if let View::Editor(state) = &mut self.view {
                    clipboard::select_all(state);
                }
            }
            A::Group => {
                if let View::Editor(state) = &mut self.view {
                    layers_panel::group_selection(state);
                }
            }
            A::Ungroup => {
                if let View::Editor(state) = &mut self.view {
                    layers_panel::ungroup_selection(state);
                }
            }
        }
    }

    fn handle_messages(&mut self, ctx: &egui::Context) {
        // Aperturas diferidas para no pelear con el préstamo de self.view.
        let mut open_after: Option<Nav> = None;
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                AppMsg::FilePicked(Some(path)) | AppMsg::FolderPicked(Some(path)) => {
                    self.open_path(path, ctx);
                }
                AppMsg::FilePicked(None) | AppMsg::FolderPicked(None) => {}
                AppMsg::SaveAsPicked(path) => {
                    self.pending_save_as = path;
                }
                AppMsg::Saved {
                    path,
                    result,
                    new_source,
                } => {
                    if let View::Editor(state) = &mut self.view {
                        state.saving = false;
                        match result {
                            Ok(()) => {
                                tracing::info!("guardado OK: {}", path.display());
                                state.history.mark_saved();
                                // A partir de este guardado ya hay píxeles
                                // del usuario en disco: el próximo `Ctrl+S`
                                // vuelve a pedir confirmación si sobrescribe.
                                state.born_blank = false;
                                // Los eventos de disco inminentes son de este
                                // guardado: ventana de gracia y watcher nuevo
                                // (la sustitución atómica puede invalidarlo).
                                self.ignore_fs_events_until = Some(
                                    std::time::Instant::now() + std::time::Duration::from_secs(2),
                                );
                                self.watcher = None;
                                if new_source {
                                    state.doc.source_path = Some(path);
                                }
                                // «Save all»: si lo que se acaba de guardar
                                // era el frente de la cola, avanza. Se
                                // comprueba por id de ranura, no por ruta:
                                // más robusto ante un renombrado en vuelo.
                                if self.save_all_queue.first().is_some_and(|&id| {
                                    self.deck.slots.get(self.deck.active).map(|s| s.id) == Some(id)
                                }) {
                                    self.save_all_queue.remove(0);
                                    self.save_all_attempted = false;
                                }
                                if self.close_after_save {
                                    self.allow_close = true;
                                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                                } else if let Some(nav) = self.after_save.take() {
                                    open_after = Some(nav);
                                }
                            }
                            Err(e) => {
                                self.close_after_save = false;
                                self.after_save = None;
                                // No hace falta esperar al frame siguiente
                                // para que el chequeo de la cola detecte el
                                // fallo: se aborta el lote aquí mismo si era
                                // su frente el que acaba de fallar.
                                if self.save_all_queue.first().is_some_and(|&id| {
                                    self.deck.slots.get(self.deck.active).map(|s| s.id) == Some(id)
                                }) {
                                    self.save_all_queue.clear();
                                    self.save_all_attempted = false;
                                }
                                state.save_error = Some(e);
                            }
                        }
                    }
                }
                AppMsg::ExportPathPicked(path) => {
                    if let (Some(path), Some(settings)) =
                        (path, self.pending_export_settings.take())
                    {
                        self.pending_export = Some((path, settings));
                    } else {
                        self.pending_export_settings = None;
                    }
                }
                AppMsg::Exported { path, result } => {
                    if let View::Editor(state) = &mut self.view {
                        state.exporting = false;
                        match result {
                            Ok(()) => tracing::info!("exportado OK: {}", path.display()),
                            Err(e) => {
                                state.save_error = Some(format!("Could not export: {e}"));
                            }
                        }
                    }
                }
                AppMsg::ImageLoadedForLayer { path, result } => {
                    if let View::Editor(state) = &mut self.view {
                        match result {
                            Ok(img) => {
                                let name = path
                                    .file_stem()
                                    .map(|s| s.to_string_lossy().into_owned())
                                    .unwrap_or_else(|| "Image".to_owned());
                                state.add_image_layer(name, Some(path), img);
                            }
                            Err(e) => {
                                state.save_error =
                                    Some(format!("Could not add \"{}\": {e}", path.display()));
                            }
                        }
                    }
                }
                AppMsg::GalleryScanned { folder, files } => {
                    // La baraja del editor (si es la misma carpeta) y la
                    // rejilla (si está abierta ahí) pueden querer el mismo
                    // reescaneo a la vez — típicamente al volver de un
                    // editor recién abierto desde esa galería.
                    let want_deck = self.deck.folder.as_deref() == Some(folder.as_path());
                    let want_gallery = matches!(&self.view, View::Gallery(g) if g.folder == folder);
                    match (want_deck, want_gallery) {
                        (true, true) => {
                            self.deck.merge_scan(files.clone());
                            if let View::Gallery(g) = &mut self.view {
                                g.merge_files(files);
                            }
                        }
                        (true, false) => self.deck.merge_scan(files),
                        (false, true) => {
                            if let View::Gallery(g) = &mut self.view {
                                g.merge_files(files);
                            }
                        }
                        (false, false) => {}
                    }
                    // Archivos nuevos en `merge_scan` nacen con `page: None`
                    // (`idle_slot`): sondearlos cubre el caso de añadir
                    // archivos a la carpeta mientras el editor ya está
                    // abierto en ella, no solo la apertura inicial.
                    if want_deck {
                        self.spawn_deck_probe(ctx);
                    }
                }
                AppMsg::GalleryThumb {
                    folder,
                    path,
                    result,
                } => {
                    // Igual que arriba: se sube la textura UNA vez y se
                    // reparte el handle (barato de clonar) a quien la quiera,
                    // para no duplicar la subida a GPU cuando ambas coinciden.
                    let want_deck = self.deck.folder.as_deref() == Some(folder.as_path());
                    let want_gallery = matches!(&self.view, View::Gallery(g) if g.folder == folder);
                    if want_deck || want_gallery {
                        match result {
                            Ok(img) => {
                                let color = egui::ColorImage::from_rgba_unmultiplied(
                                    [img.width as usize, img.height as usize],
                                    &img.rgba,
                                );
                                let tex = ctx.load_texture(
                                    path.to_string_lossy().into_owned(),
                                    color,
                                    egui::TextureOptions::LINEAR,
                                );
                                if want_deck {
                                    self.deck.set_thumb(&path, Some(tex.clone()));
                                }
                                if want_gallery {
                                    if let View::Gallery(g) = &mut self.view {
                                        g.set_thumb(&path, Some(tex));
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::warn!("miniatura de {} falló: {e}", path.display());
                                if want_deck {
                                    self.deck.set_thumb(&path, None);
                                }
                                if want_gallery {
                                    if let View::Gallery(g) = &mut self.view {
                                        g.set_thumb(&path, None);
                                    }
                                }
                            }
                        }
                    }
                }
                AppMsg::DeckProbed { folder, sizes } => {
                    if self.deck.folder.as_deref() == Some(folder.as_path()) {
                        self.deck.set_probes(sizes);
                    }
                }
                AppMsg::SlotLoaded {
                    folder,
                    path,
                    result,
                    metadata,
                } => {
                    // Guarda de obsolescencia: si la baraja ya no es esta
                    // carpeta (el usuario abrió otra cosa mientras cargaba),
                    // el mensaje se descarta entero — el `inflight` de la
                    // baraja NUEVA no tiene nada que ver con esta carga.
                    if self.deck.folder.as_deref() == Some(folder.as_path()) {
                        self.deck.loading_finished();
                        if let Some(idx) = self.deck.find_by_path(&path) {
                            // Si mientras tanto la ranura dejó de estar
                            // `Loading` (se activó por otra vía, o ya se
                            // descartó), no se pisa: esta carga ya no pinta
                            // nada.
                            let still_loading =
                                self.deck.slots.get(idx).is_some_and(|s| {
                                    matches!(s.content, deck::SlotContent::Loading)
                                });
                            if still_loading {
                                let metadata = (!metadata.is_empty()).then_some(metadata);
                                let new_content = match result {
                                    Ok(outcome) => build_slot_doc(
                                        path.clone(),
                                        outcome,
                                        metadata,
                                        self.settings.sidecar_default,
                                    )
                                    .map_or_else(
                                        || {
                                            deck::SlotContent::Failed(
                                                "could not build the document".to_owned(),
                                            )
                                        },
                                        |doc| deck::SlotContent::Ready(Box::new(doc)),
                                    ),
                                    Err(e) => {
                                        tracing::warn!(
                                            "carga de fondo de {} falló: {e}",
                                            path.display()
                                        );
                                        deck::SlotContent::Failed(e)
                                    }
                                };
                                if let Some(slot) = self.deck.slots.get_mut(idx) {
                                    slot.content = new_content;
                                }
                            }
                        }
                    }
                }
                AppMsg::CanvasPathReserved {
                    folder,
                    slot,
                    result,
                } => {
                    // Libera el cerrojo PRIMERO y siempre, para que un
                    // guardián de carpeta obsoleta (más abajo) no lo deje
                    // atascado.
                    if self.materializing == Some(slot) {
                        self.materializing = None;
                    }
                    // Guarda de obsolescencia, igual que `SlotLoaded`: si la
                    // baraja ya no es esta carpeta, el archivo reservado (0
                    // bytes) queda huérfano — se registra, no se limpia
                    // (borrarlo de fondo podría chocar con un usuario que
                    // reabrió justo esa carpeta).
                    if self.deck.folder.as_deref() != Some(folder.as_path()) {
                        tracing::warn!(
                            "baraja: reserva de nombre para «{}» llegó tras cambiar de carpeta; \
                             el archivo reservado queda huérfano",
                            folder.display()
                        );
                        continue;
                    }
                    match result {
                        Ok(path) => {
                            let Some(idx) = self.deck.find_by_id(slot) else {
                                tracing::warn!(
                                    "baraja: la ranura provisional ya no existe al reservar su nombre"
                                );
                                continue;
                            };
                            // Mismo patrón que `DocumentRenamed`: la tira lee
                            // ruta y nombre de la RANURA, no del documento.
                            if let Some(s) = self.deck.slots.get_mut(idx) {
                                s.name = path
                                    .file_name()
                                    .map(|n| n.to_string_lossy().into_owned())
                                    .unwrap_or_default();
                                s.path = path.clone();
                                s.is_placeholder = false;
                            }
                            if idx == self.deck.active {
                                if let View::Editor(state) = &mut self.view {
                                    // `state.is_design` refleja la extensión
                                    // REAL reservada (`settings.new_canvas_format`
                                    // en el momento de crear la ranura), no un
                                    // `true` fijo: la mayoría de lienzos nuevos
                                    // hoy son un raster, no un diseño autónomo.
                                    state.is_design = canvas_io::is_canvas_file(&path);
                                    state.doc.source_path = Some(path);
                                    // El bloque de guardado normal, más abajo
                                    // en este mismo frame, toma la rama de
                                    // diseño y llama a `start_save_design`
                                    // con horneado de miniatura, ventana de
                                    // gracia y `mark_saved()` de siempre —
                                    // gratis, sin duplicar nada de eso aquí.
                                    state.save_clicked = true;
                                }
                            } else if let deck::SlotContent::Ready(d) =
                                &mut self.deck.slots[idx].content
                            {
                                // El usuario saltó a otro lienzo mientras la
                                // reserva estaba en vuelo: se deja lista para
                                // guardarse la próxima vez (Ctrl+S al volver
                                // a ella, o Save All), sin forzarlo ahora.
                                d.doc.source_path = Some(path);
                            }
                            // Relleno automático: siempre queda una
                            // provisional lista al final, con o sin éxito
                            // arriba.
                            self.deck.push_placeholder(
                                self.settings.last_page_size,
                                self.settings.new_canvas_format.extension(),
                            );
                        }
                        Err(e) => {
                            self.materialize_blocked = Some(slot);
                            tracing::warn!("no se pudo crear el archivo del nuevo lienzo: {e}");
                            if let View::Editor(state) = &mut self.view {
                                state.save_error = Some(format!(
                                    "Could not create a file for the new canvas: {e}"
                                ));
                            }
                        }
                    }
                }
                AppMsg::GalleryOpDone {
                    folder,
                    created,
                    result,
                    open,
                } => {
                    // Lo que vamos a abrir lo acabamos de escribir nosotros:
                    // ventana de gracia para que el watcher no cante «cambió
                    // en disco» si el usuario ya estaba en el editor.
                    self.ignore_fs_events_until =
                        Some(std::time::Instant::now() + std::time::Duration::from_secs(2));
                    match result {
                        Ok(()) if open => {
                            if let Some(path) = created {
                                open_after = Some(Nav::Open(path));
                            }
                        }
                        Ok(()) => {
                            // Solo rescanea si el usuario sigue en esa galería:
                            // pudo haber navegado mientras corría la copia.
                            if matches!(&self.view, View::Gallery(g) if g.folder == folder) {
                                // El resultado de la operación queda
                                // seleccionado (borde azul): la copia recién
                                // duplicada/pegada, el archivo recién
                                // renombrado, o nada tras un borrado
                                // (`created` es `None`, limpia la marca).
                                if let View::Gallery(g) = &mut self.view {
                                    g.selected = created.clone();
                                }
                                self.rescan_gallery(ctx);
                            }
                            // Igual, pero para la baraja del editor (p.ej. el
                            // botón «⧉» de la cabecera de un lienzo, que
                            // dispara esta misma operación aunque la vista
                            // actual sea el editor, no la galería) — la
                            // reconciliación (`merge_scan`, incluido
                            // `order_hint`) llega sola al recibir
                            // `GalleryScanned`, aquí solo hace falta pedirla.
                            if self.deck.folder.as_deref() == Some(folder.as_path()) {
                                loader::spawn_gallery_scan(
                                    folder.clone(),
                                    self.thumb_cache.clone(),
                                    self.tx.clone(),
                                    ctx.clone(),
                                );
                            }
                        }
                        Err(e) => {
                            // No hay nada destructivo que deshacer (la copia
                            // fallida ya se revirtió en el hilo de trabajo).
                            // Se registra y, si el usuario sigue en esa
                            // galería, también se le muestra: antes solo
                            // quedaba en el log, invisible en la UI.
                            tracing::warn!("operación de galería fallida: {e}");
                            if let View::Gallery(g) = &mut self.view {
                                if g.folder == folder {
                                    g.op_error = Some(e);
                                }
                            }
                        }
                    }
                }
                AppMsg::DocumentRenamed { old_path, result } => {
                    let is_active = matches!(&self.view, View::Editor(state)
                        if state.doc.source_path.as_deref() == Some(old_path.as_path()));
                    if is_active {
                        if let View::Editor(state) = &mut self.view {
                            match result {
                                Ok(new_path) => {
                                    // La ranura activa de la baraja lleva su
                                    // propia copia de la ruta/nombre (la
                                    // tira los lee de ahí, no del documento):
                                    // sin esto, renombrar dejaría la tira con
                                    // el nombre viejo hasta el próximo
                                    // reescaneo.
                                    if let Some(slot) = self.deck.slots.get_mut(self.deck.active) {
                                        slot.path = new_path.clone();
                                        slot.name = new_path
                                            .file_name()
                                            .map(|n| n.to_string_lossy().into_owned())
                                            .unwrap_or_default();
                                    }
                                    state.doc.source_path = Some(new_path);
                                }
                                // Reutiliza el banner de error que ya existe
                                // en el panel: no hace falta un campo nuevo.
                                Err(e) => state.save_error = Some(e),
                            }
                        }
                    } else {
                        // Ranura de FONDO (cabecera del lienzo en el área
                        // central, no la activa): sin `state.doc` que
                        // actualizar, solo la propia ranura de la baraja —
                        // mismo campo que arriba, generalizado por ruta en
                        // vez de "la activa". Sin banner de error propio
                        // para una ranura que no se está mirando: se
                        // registra y ya.
                        match result {
                            Ok(new_path) => {
                                if let Some(slot) =
                                    self.deck.slots.iter_mut().find(|s| s.path == old_path)
                                {
                                    slot.path = new_path.clone();
                                    slot.name = new_path
                                        .file_name()
                                        .map(|n| n.to_string_lossy().into_owned())
                                        .unwrap_or_default();
                                }
                            }
                            Err(e) => tracing::warn!(
                                "no se pudo renombrar {} en segundo plano: {e}",
                                old_path.display()
                            ),
                        }
                    }
                }
                AppMsg::DocumentDeleted { path, result } => {
                    // `state` toma prestado `self.view`; no se puede
                    // reasignar `self.view` mientras siga vivo, así que la
                    // decisión se guarda en una variable local y se aplica
                    // después de que el préstamo termine.
                    let mut go_to_welcome = false;
                    let is_active = matches!(&self.view, View::Editor(state)
                        if state.doc.source_path.as_deref() == Some(path.as_path()));
                    if is_active {
                        if let View::Editor(state) = &mut self.view {
                            match result {
                                Ok(()) => {
                                    // El archivo ya no existe: no tiene
                                    // sentido preguntar por cambios sin
                                    // guardar (no hay dónde guardarlos). Si
                                    // la baraja tiene más ranuras y la
                                    // vecina ya está cargada, se salta a
                                    // ella en vez de salir del editor entero
                                    // — el archivo desapareció, pero el
                                    // resto de la carpeta sigue teniendo
                                    // sentido en pantalla.
                                    let mut jumped = false;
                                    if self.deck.slots.len() > 1 {
                                        let removed = self.deck.active;
                                        self.deck.slots.remove(removed);
                                        // Sin esto los supervivientes se
                                        // quedan con el `rect` viejo
                                        // (calculado con la borrada
                                        // todavía en la pila) hasta el
                                        // próximo cambio que sí encienda
                                        // el flag — se ve como un hueco
                                        // vacío que nadie ocupa.
                                        self.deck.layout_dirty = true;
                                        let neighbor =
                                            removed.min(self.deck.slots.len().saturating_sub(1));
                                        if let Some(slot) = self.deck.slots.get_mut(neighbor) {
                                            if matches!(slot.content, deck::SlotContent::Ready(_)) {
                                                let deck::SlotContent::Ready(incoming) =
                                                    std::mem::replace(
                                                        &mut slot.content,
                                                        deck::SlotContent::Active,
                                                    )
                                                else {
                                                    unreachable!("comprobado justo arriba");
                                                };
                                                state.put_slot(*incoming);
                                                self.deck.active = neighbor;
                                                jumped = true;
                                            }
                                        }
                                    }
                                    if !jumped {
                                        match state.from_gallery.clone() {
                                            Some(folder) => open_after = Some(Nav::Open(folder)),
                                            None => go_to_welcome = true,
                                        }
                                    }
                                }
                                Err(e) => state.save_error = Some(e),
                            }
                        }
                    } else {
                        // Ranura de FONDO (cabecera del lienzo en el área
                        // central, no la activa): borrar generaliza el mismo
                        // bloque de arriba que YA quita la ranura activa de
                        // `self.deck.slots` — sin salto ni pantalla de
                        // bienvenida, porque el usuario no estaba mirando
                        // este lienzo.
                        match result {
                            Ok(()) => {
                                if let Some(idx) =
                                    self.deck.slots.iter().position(|s| s.path == path)
                                {
                                    self.deck.slots.remove(idx);
                                    // Si la borrada estaba ANTES de la
                                    // activa en el `Vec`, todo lo posterior
                                    // se desplaza un puesto — sin este
                                    // ajuste `deck.active` (un índice, no un
                                    // id) pasaría a apuntar a la ranura
                                    // equivocada, y la que de verdad sigue
                                    // activa dejaría de encajar en ninguna
                                    // rama del render (ni "es la activa" ni
                                    // "tiene contenido `Ready`", porque su
                                    // contenido es el marcador `Active`) —
                                    // su cuerpo desaparecía aunque la
                                    // cabecera se siguiera pintando.
                                    if idx < self.deck.active {
                                        self.deck.active -= 1;
                                    }
                                    self.deck.layout_dirty = true;
                                }
                            }
                            Err(e) => tracing::warn!(
                                "no se pudo borrar {} en segundo plano: {e}",
                                path.display()
                            ),
                        }
                    }
                    if go_to_welcome {
                        self.view = View::Welcome { error: None };
                    }
                }
                AppMsg::FocusWindow => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                AppMsg::ShellIntegrationDone(result) => {
                    self.shell_status = match result {
                        Ok(msg) => msg,
                        Err(e) => format!("Failed: {e}"),
                    };
                }
                AppMsg::OpenPathExternal(path) => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    // Pregunta si hay un editor con cambios sin guardar.
                    self.request_nav(Nav::Open(path), ctx);
                }
                AppMsg::SourceChangedOnDisk { path } => {
                    let own_save = self
                        .ignore_fs_events_until
                        .is_some_and(|t| std::time::Instant::now() < t);
                    if !own_save {
                        if let View::Editor(state) = &mut self.view {
                            if state.doc.source_path.as_deref() == Some(path.as_path()) {
                                state.external_change = true;
                            }
                        }
                    }
                }
                AppMsg::ImageLoaded {
                    path,
                    result,
                    metadata,
                } => {
                    // Ignora cargas que ya no corresponden a la vista actual.
                    let expected = matches!(&self.view, View::Loading { path: p } if *p == path);
                    if !expected {
                        continue;
                    }
                    let metadata = (!metadata.is_empty()).then_some(metadata);
                    match result {
                        Ok(loader::LoadOutcome::Restored(restored)) => {
                            // Si la imagen cambió por fuera desde el último
                            // guardado con capas, avisa y deja elegir.
                            let use_layers = restored.hash_matches
                                || {
                                    let choice = rfd::MessageDialog::new()
                                    .set_level(rfd::MessageLevel::Warning)
                                    .set_title("Image changed outside Canvas Desktop")
                                    .set_description(format!(
                                        "\"{}\" was modified by another program after the last save with layers.\nRestore the editable layers anyway? (\"No\" opens the image as it is now.)",
                                        path.file_name().map(|s| s.to_string_lossy()).unwrap_or_default()
                                    ))
                                    .set_buttons(rfd::MessageButtons::YesNo)
                                    .show();
                                    matches!(choice, rfd::MessageDialogResult::Yes)
                                };
                            if use_layers {
                                self.resolve_deck(&path, ctx);
                                let mut state =
                                    editor::EditorState::from_restored(path.clone(), restored);
                                state.from_gallery = self.deck.folder.clone();
                                state.sidecar_enabled = self.settings.sidecar_default;
                                state.source_metadata = metadata;
                                self.remember_page_size(&state.doc);
                                self.view = View::Editor(Box::new(state));
                            } else {
                                // Recarga plana, ignorando el sidecar.
                                loader::spawn_load_image(
                                    path.clone(),
                                    false,
                                    self.tx.clone(),
                                    ctx.clone(),
                                );
                                self.view = View::Loading { path: path.clone() };
                            }
                        }
                        Ok(loader::LoadOutcome::Design(restored)) => {
                            // Diseño autónomo: `hash_matches` siempre es
                            // `true` (no hay nada que contrastar), así que no
                            // hace falta el diálogo de «cambió por fuera».
                            self.resolve_deck(&path, ctx);
                            let mut state =
                                editor::EditorState::from_design(path.clone(), restored);
                            state.from_gallery = self.deck.folder.clone();
                            self.remember_page_size(&state.doc);
                            self.view = View::Editor(Box::new(state));
                        }
                        Ok(loader::LoadOutcome::Flat(img)) => {
                            match editor::EditorState::from_image(path.clone(), img) {
                                Ok(mut state) => {
                                    self.resolve_deck(&path, ctx);
                                    state.from_gallery = self.deck.folder.clone();
                                    state.sidecar_enabled = self.settings.sidecar_default;
                                    state.source_metadata = metadata;
                                    self.remember_page_size(&state.doc);
                                    self.view = View::Editor(Box::new(state));
                                }
                                Err(e) => {
                                    self.view = View::Welcome {
                                        error: Some(format!(
                                            "Could not open \"{}\": {e}",
                                            path.display()
                                        )),
                                    };
                                }
                            }
                        }
                        Err(e) => {
                            self.view = View::Welcome {
                                error: Some(format!("Could not open \"{}\": {e}", path.display())),
                            };
                        }
                    }
                    self.sync_title(ctx);
                }
            }
        }
        if let Some(nav) = open_after {
            self.navigate(nav, ctx);
        }
    }

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });
        if let Some(path) = dropped.into_iter().next() {
            // Con un documento abierto, soltar una imagen la AÑADE como capa;
            // en cualquier otra vista (o si es carpeta), abre como siempre.
            if matches!(self.view, View::Editor(_))
                && path.is_file()
                && canvas_io::is_image_file(&path)
            {
                loader::spawn_load_image_as_layer(path, self.tx.clone(), ctx.clone());
            } else {
                self.open_path(path, ctx);
            }
        }
    }

    /// Si el usuario intenta cerrar con cambios sin guardar, cancela el
    /// cierre y pregunta con un diálogo nativo Guardar / Descartar / Cancelar.
    fn confirm_close(&mut self, ctx: &egui::Context) {
        if self.allow_close || !ctx.input(|i| i.viewport().close_requested()) {
            return;
        }
        if !matches!(self.view, View::Editor(_)) {
            return;
        }
        // Otras ranuras de la baraja pueden tener cambios sin guardar aunque
        // la activa esté limpia: cerrar la app las perdería en silencio si
        // no se avisa aquí también. «Save» aquí solo guarda la activa —
        // «Save all» es una acción del editor (`Ctrl+Alt+S`), no de este
        // diálogo — así que con más de un lienzo sucio el texto lo dice
        // explícitamente en vez de fingir que un único «Save» los cubre.
        let names = self.dirty_canvas_names();
        if names.is_empty() {
            return;
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        let description = if names.len() == 1 {
            format!(
                "\"{}\" has unsaved changes.\nSave them before closing? (\"No\" discards them.)",
                names[0]
            )
        } else {
            format!(
                "{} canvases have unsaved changes:\n\u{2022} {}\n\n\"Save\" only saves the \
                 active one — the rest will be lost when you close. Cancel and switch to them \
                 first if you want to keep their changes.",
                names.len(),
                names.join("\n\u{2022} ")
            )
        };
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
        // OJO: en Windows, sin la feature `common-controls-v6` de rfd los
        // botones custom degradan a un MessageBox Sí/No/Cancelar que devuelve
        // Yes/No/Cancel, nunca Custom. Hay que aceptar ambas familias.
        match choice {
            rfd::MessageDialogResult::Yes => {
                self.save_requested = true;
                self.close_after_save = true;
            }
            rfd::MessageDialogResult::Custom(c) if c == "Save" => {
                self.save_requested = true;
                self.close_after_save = true;
            }
            rfd::MessageDialogResult::No => {
                self.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            rfd::MessageDialogResult::Custom(c) if c == "Discard" => {
                self.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            _ => {}
        }
    }

    /// Mantiene el título de la ventana (con asterisco de cambios sin
    /// guardar) al día; solo envía el comando cuando cambia.
    fn sync_title(&mut self, ctx: &egui::Context) {
        let title = match &self.view {
            View::Editor(state) => {
                let dirty = if state.is_dirty() { "*" } else { "" };
                let position = if self.deck.slots.len() > 1 {
                    format!(" ({}/{})", self.deck.active + 1, self.deck.slots.len())
                } else {
                    String::new()
                };
                format!("{dirty}{}{position} — Canvas Desktop", state.file_name())
            }
            View::Loading { path } => format!(
                "Loading {}… — Canvas Desktop",
                path.file_name()
                    .map(|s| s.to_string_lossy())
                    .unwrap_or_default()
            ),
            View::Gallery(g) => format!(
                "{} — Canvas Desktop",
                g.folder
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| g.folder.display().to_string())
            ),
            View::Welcome { .. } => "Canvas Desktop".to_owned(),
        };
        if title != self.last_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.last_title = title;
        }
    }
}

/// Directorio de caché de miniaturas del usuario (mejor esfuerzo).
fn thumbnail_cache_dir() -> Option<PathBuf> {
    let dirs = directories::ProjectDirs::from("com", "canvas-desktop", "Canvas Desktop")?;
    let dir = dirs.cache_dir().join("thumbnails");
    match std::fs::create_dir_all(&dir) {
        Ok(()) => Some(dir),
        Err(e) => {
            tracing::warn!("sin caché de miniaturas ({}): {e}", dir.display());
            None
        }
    }
}

/// Al volver a la galería desde el editor, siembra la rejilla con lo que ya
/// tenía la baraja (miniaturas ya subidas a GPU): evita el parpadeo de ⏳ que
/// antes hacía falta esperar a que el reescaneo (que se lanza de todas
/// formas, para detectar archivos nuevos o borrados por fuera) volviera a
/// decodificarlo todo. Si la baraja pertenece a otra carpeta, la rejilla
/// arranca vacía como siempre.
fn seed_gallery_from_deck(
    deck: &deck::Deck,
    folder: PathBuf,
    sort: settings::GallerySort,
) -> gallery::GalleryState {
    let mut g = gallery::GalleryState::new(folder.clone(), sort);
    if deck.folder.as_deref() == Some(folder.as_path()) {
        g.items = deck
            .slots
            .iter()
            // Una ranura provisional no tiene archivo detrás todavía: una
            // miniatura suya en la rejilla sería una casilla que nunca
            // termina de cargar y que no se puede abrir.
            .filter(|s| !s.is_placeholder)
            .map(|s| gallery::GalleryItem {
                path: s.path.clone(),
                name: s.name.clone(),
                mtime: s.mtime,
                kind: s.kind,
                tex: s.thumb.clone(),
                failed: s.thumb_failed,
            })
            .collect();
        g.scanned = !g.items.is_empty();
        g.apply_sort();
    }
    g
}

/// Construye el `SlotDoc` de una carga de fondo de la baraja, reutilizando
/// los constructores de `EditorState` (evita duplicar la lógica de
/// restaurar capas desde el sidecar): se arma un `EditorState` efímero y se
/// cosecha con `take_slot()` — sus campos de sesión (viewport, gestos…) se
/// tiran, solo interesaban los del documento. `None` si el documento no
/// pudo construirse (p. ej. una imagen sin píxeles válidos).
///
/// A diferencia de `AppMsg::ImageLoaded`, un sidecar cuyo hash no coincide
/// con la imagen NUNCA abre el diálogo interactivo aquí (sería un modal
/// disparado por hacer scroll): las capas restauradas se usan de todas
/// formas y `external_change` queda encendido, para que el banner normal de
/// «cambió por fuera» aparezca en cuanto el usuario active esa ranura.
fn build_slot_doc(
    path: PathBuf,
    outcome: loader::LoadOutcome,
    metadata: Option<canvas_io::ImageMetadata>,
    sidecar_default: bool,
) -> Option<deck::SlotDoc> {
    match outcome {
        loader::LoadOutcome::Restored(restored) => {
            let external_change = !restored.hash_matches;
            let mut state = editor::EditorState::from_restored(path, restored);
            state.sidecar_enabled = sidecar_default;
            state.source_metadata = metadata;
            state.external_change = external_change;
            Some(state.take_slot())
        }
        loader::LoadOutcome::Design(restored) => {
            let mut state = editor::EditorState::from_design(path, restored);
            Some(state.take_slot())
        }
        loader::LoadOutcome::Flat(img) => match editor::EditorState::from_image(path, img) {
            Ok(mut state) => {
                state.sidecar_enabled = sidecar_default;
                state.source_metadata = metadata;
                Some(state.take_slot())
            }
            Err(e) => {
                tracing::warn!("carga de fondo: no se pudo construir el documento: {e}");
                None
            }
        },
    }
}

/// Hornea la página en la GPU (hilo de UI) y delega codificar+escribir a un
/// hilo de trabajo. Si el horneado falla, el error queda visible en el panel
/// y el documento intacto.
#[allow(clippy::too_many_arguments)]
fn start_save(
    state: &mut editor::EditorState,
    renderer: &mut CanvasRenderer,
    rs: &RenderState,
    tx: &std::sync::mpsc::Sender<AppMsg>,
    ctx: &egui::Context,
    path: PathBuf,
    new_source: bool,
    jpeg_quality: u8,
    ignore_fs_events_until: &mut Option<std::time::Instant>,
) {
    if state.saving {
        return;
    }
    tracing::info!("guardando en {}", path.display());
    match renderer.bake_page(
        &rs.device,
        &rs.queue,
        canvas_render::FxScope::default(),
        &state.doc,
        &state.images,
        1.0,
    ) {
        Ok((rgba, width, height)) => {
            state.saving = true;
            state.save_error = None;
            // Arranca la ventana de gracia YA, no cuando llegue `Saved`: el
            // watcher corre en otro hilo y puede notificar el cambio en disco
            // (la escritura empieza aquí mismo, en `spawn_save`) antes de que
            // el hilo de guardado termine y mande `Saved` — si la ventana se
            // abriera solo al recibir ese mensaje, ese evento adelantado
            // llegaría sin filtro y dispararía el banner de "cambió en disco".
            *ignore_fs_events_until =
                Some(std::time::Instant::now() + std::time::Duration::from_secs(5));
            let sidecar = state.sidecar_enabled.then(|| state.sidecar_payload());
            loader::spawn_save(
                path,
                rgba,
                width,
                height,
                jpeg_quality,
                state.source_metadata.clone(),
                new_source,
                sidecar,
                tx.clone(),
                ctx.clone(),
            );
        }
        Err(e) => {
            tracing::error!("horneado falló: {e}");
            state.save_error = Some(format!("Could not prepare the save: {e}"));
        }
    }
}

/// Guarda un diseño autónomo. La GPU solo interviene para hornear la
/// MINIATURA embebida (a escala reducida: nadie necesita 4K en una celda de
/// 156 px). Si el horneado falla, el diseño se guarda igual sin miniatura:
/// no es motivo para bloquear el guardado real.
#[allow(clippy::too_many_arguments)]
fn start_save_design(
    state: &mut editor::EditorState,
    renderer: &mut CanvasRenderer,
    rs: &RenderState,
    tx: &std::sync::mpsc::Sender<AppMsg>,
    ctx: &egui::Context,
    path: PathBuf,
    new_source: bool,
    ignore_fs_events_until: &mut Option<std::time::Instant>,
) {
    if state.saving {
        return;
    }
    tracing::info!("guardando diseño en {}", path.display());
    state.is_design = true; // «Save as… → .canvas» convierte el documento.
    let mut payload = state.sidecar_payload();
    let (pw, ph) = state
        .doc
        .page()
        .map(|p| (p.width, p.height))
        .unwrap_or((0.0, 0.0));
    let scale = canvas_io::preview_scale(pw, ph);
    match renderer.bake_page(
        &rs.device,
        &rs.queue,
        canvas_render::FxScope::default(),
        &state.doc,
        &state.images,
        scale,
    ) {
        Ok((rgba, w, h)) => payload.preview = canvas_io::make_preview(&rgba, w, h),
        Err(e) => tracing::warn!("miniatura del diseño no horneada: {e}"),
    }
    state.saving = true;
    state.save_error = None;
    // Ver el comentario en `start_save`: la ventana de gracia debe abrirse
    // antes de lanzar la escritura, no al recibir `Saved`.
    *ignore_fs_events_until = Some(std::time::Instant::now() + std::time::Duration::from_secs(5));
    loader::spawn_save_design(path, payload, new_source, tx.clone(), ctx.clone());
}

/// PNG/JPEG hornean en la GPU igual que al guardar. SVG/PDF se generan a
/// mano a partir del documento, pero primero hay que sincronizar los
/// efectos GPU (desenfoque, ajustes de color) y tomar las texturas ya
/// procesadas — lo mismo que hace `bake_page` por dentro — para que el SVG
/// lleve los píxeles TAL Y COMO se ven en el lienzo, sin reimplementar los
/// efectos como filtros SVG.
fn start_export(
    state: &mut editor::EditorState,
    renderer: &mut CanvasRenderer,
    rs: &RenderState,
    tx: &std::sync::mpsc::Sender<AppMsg>,
    ctx: &egui::Context,
    path: PathBuf,
    settings: export::ExportSettings,
) {
    if state.exporting {
        return;
    }
    tracing::info!("exportando a {}", path.display());
    let scale = f64::from(settings.scale);

    if settings.format.needs_bake() {
        match renderer.bake_page(
            &rs.device,
            &rs.queue,
            canvas_render::FxScope::default(),
            &state.doc,
            &state.images,
            scale,
        ) {
            Ok((rgba, width, height)) => {
                state.exporting = true;
                state.save_error = None;
                loader::spawn_export_raster(
                    path,
                    rgba,
                    width,
                    height,
                    settings.jpeg_quality,
                    tx.clone(),
                    ctx.clone(),
                );
            }
            Err(e) => {
                tracing::error!("horneado falló: {e}");
                state.save_error = Some(format!("Could not prepare the export: {e}"));
            }
        }
        return;
    }

    if let Ok(page) = state.doc.page() {
        for layer in &page.layers {
            if let Some(source) = state.images.get(&layer.id) {
                renderer.sync_layer_effects(
                    &rs.device,
                    &rs.queue,
                    canvas_render::FxScope::default(),
                    layer.id,
                    source,
                    &layer.effects,
                );
            }
        }
    }
    let blurred = renderer.blur_overrides(canvas_render::FxScope::default());
    let mut images: Vec<canvas_io::LayerPixels> = Vec::new();
    if let Ok(page) = state.doc.page() {
        for layer in &page.layers {
            let Some(data) = blurred
                .get(&layer.id)
                .or_else(|| state.images.get(&layer.id))
            else {
                continue;
            };
            images.push((
                layer.id.raw(),
                data.data.data().to_vec(),
                data.width,
                data.height,
            ));
        }
    }
    state.exporting = true;
    state.save_error = None;
    loader::spawn_export_vector(
        path,
        state.doc.clone(),
        images,
        settings.format,
        scale,
        tx.clone(),
        ctx.clone(),
    );
}

/// ¿La extensión de `path` es JPEG? (para el aviso de calidad de recompresión)
fn is_jpeg_path(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "jpg" | "jpeg"))
}

/// `foto.png.canvas` (hermano legacy) o `.canvas/foto.png.canvas` (ubicación
/// actual) → `foto.png` si esa imagen existe; cualquier otra ruta se
/// devuelve tal cual. Punto de entrada para abrir un sidecar directamente
/// desde el Explorador (doble clic, "Abrir con"). El guard exige que `inner`
/// sea además una imagen (no solo un archivo cualquiera) para que un diseño
/// autónomo con nombre `Untitled.canvas` (cuyo `inner` es `Untitled`, sin
/// extensión) nunca se confunda con el sidecar de otra cosa.
fn resolve_canvas_sidecar(path: PathBuf) -> PathBuf {
    if !canvas_io::is_canvas_file(&path) {
        return path;
    }
    // Ubicación actual: `<carpeta>/.canvas/foto.png.canvas`. `file_stem()`
    // quita solo la extensión `.canvas` y deja `foto.png`; el abuelo de
    // `path` es la carpeta real de la imagen.
    let in_sidecar_dir = path
        .parent()
        .and_then(|p| p.file_name())
        .is_some_and(|n| n == canvas_io::SIDECAR_DIR);
    if in_sidecar_dir {
        if let Some(grandparent) = path.parent().and_then(|p| p.parent()) {
            if let Some(stem) = path.file_stem() {
                let inner = grandparent.join(stem);
                if canvas_io::is_image_file(&inner) && inner.is_file() {
                    return inner;
                }
            }
        }
        return path;
    }
    // Hermano legacy: `with_extension("")` quita solo la última extensión.
    let inner = path.with_extension("");
    if canvas_io::is_image_file(&inner) && inner.is_file() {
        return inner;
    }
    path
}

#[cfg(test)]
mod resolve_canvas_sidecar_tests {
    use super::resolve_canvas_sidecar;

    #[test]
    fn resolves_a_sidecar_inside_the_dot_canvas_folder_to_its_image() {
        let dir = tempfile::tempdir().expect("tempdir");
        let image = dir.path().join("foto.png");
        std::fs::write(&image, b"x").unwrap();
        let sidecar_dir = dir.path().join(canvas_io::SIDECAR_DIR);
        std::fs::create_dir_all(&sidecar_dir).unwrap();
        let sidecar = sidecar_dir.join("foto.png.canvas");
        std::fs::write(&sidecar, b"{}").unwrap();

        assert_eq!(resolve_canvas_sidecar(sidecar), image);
    }

    #[test]
    fn resolves_a_legacy_sibling_sidecar_to_its_image() {
        let dir = tempfile::tempdir().expect("tempdir");
        let image = dir.path().join("foto.png");
        std::fs::write(&image, b"x").unwrap();
        let sidecar = dir.path().join("foto.png.canvas");
        std::fs::write(&sidecar, b"{}").unwrap();

        assert_eq!(resolve_canvas_sidecar(sidecar), image);
    }

    #[test]
    fn a_standalone_design_is_returned_as_is() {
        let dir = tempfile::tempdir().expect("tempdir");
        let design = dir.path().join("Untitled.canvas");
        std::fs::write(&design, b"{}").unwrap();

        assert_eq!(resolve_canvas_sidecar(design.clone()), design);
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
            View::Editor(state) => (state.history.can_undo(), state.history.can_redo()),
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
                Some(gallery::GalleryAction::Open(path)) => {
                    // Se lleva las miniaturas ya cargadas al editor: si el
                    // archivo resulta tener hermanos, la tira arranca sin
                    // parpadeo de ⏳ (`resolve_deck` la consume al terminar
                    // de cargar).
                    self.pending_deck = Some(deck::DeckSeed::from_gallery(g));
                    open_next = Some(Nav::Open(path));
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
                    if let Some(path) = state.doc.source_path.clone() {
                        self.ignore_fs_events_until =
                            Some(std::time::Instant::now() + std::time::Duration::from_secs(2));
                        self.watcher = None;
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
                // se ve en la tira («Untitled.png») y el que se reserva de
                // verdad deben seguir siendo el mismo.
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
                    if state.is_dirty()
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
                        if is_active {
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
                        if let Some(path) = self
                            .deck
                            .find_by_id(id)
                            .and_then(|i| self.deck.slots.get(i))
                            .map(|s| s.path.clone())
                        {
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
                        if is_active {
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
                            loader::spawn_document_delete(path, self.tx.clone(), ctx.clone());
                        }
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
