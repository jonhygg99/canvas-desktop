//! Binario de Canvas Desktop: ventana eframe/egui con el lienzo vello.

// Subsistema GUI solo en release: evita la consola negra que Windows abre
// detrás de la ventana al lanzar la app instalada desde el Explorador. En
// debug se mantiene la consola para seguir viendo los logs de `tracing`
// con `cargo run`.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod clipboard;
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

use std::path::PathBuf;
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
    /// Carpeta de galería de la que procede lo que está abierto.
    gallery_origin: Option<PathBuf>,
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
            gallery_origin: None,
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
            watcher: None,
            ignore_fs_events_until: None,
            export_dialog: None,
            pending_export_settings: None,
            pending_export: None,
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
            self.gallery_origin = Some(path.clone());
            loader::spawn_gallery_scan(
                path.clone(),
                self.thumb_cache.clone(),
                self.tx.clone(),
                ctx.clone(),
            );
            self.push_recent(&path);
            self.view = View::Gallery(gallery::GalleryState::new(path, self.settings.gallery_sort));
        } else if canvas_io::is_canvas_file(&path) {
            // Diseño autónomo: el `.canvas` ES el documento.
            if self.gallery_origin.as_deref() != path.parent() {
                self.gallery_origin = None;
            }
            loader::spawn_load_design(path.clone(), self.tx.clone(), ctx.clone());
            self.push_recent(&path);
            self.view = View::Loading { path };
        } else if canvas_io::is_image_file(&path) {
            // Abrir un archivo que no viene de la galería actual rompe el
            // vínculo con ella (el botón «volver» desaparece).
            if self.gallery_origin.as_deref() != path.parent() {
                self.gallery_origin = None;
            }
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
    /// hereda el tamaño de página del último documento abierto o creado.
    fn new_design(&mut self, ctx: &egui::Context) {
        self.gallery_origin = None;
        let (w, h) = self.settings.last_page_size;
        let mut state = editor::EditorState::new_blank(w, h);
        state.sidecar_enabled = self.settings.sidecar_default;
        self.view = View::Editor(Box::new(state));
        self.sync_title(ctx);
    }

    fn navigate(&mut self, nav: Nav, ctx: &egui::Context) {
        match nav {
            Nav::Open(path) => self.open_path(path, ctx),
            Nav::NewDesign => self.new_design(ctx),
        }
    }

    /// Navega, pero si hay un editor con cambios sin guardar delante pregunta
    /// primero (Save / Discard / Cancel).
    fn request_nav(&mut self, nav: Nav, ctx: &egui::Context) {
        let dirty_name = match &self.view {
            View::Editor(state) if state.is_dirty() => Some(state.file_name()),
            _ => None,
        };
        let Some(name) = dirty_name else {
            self.navigate(nav, ctx);
            return;
        };
        let target = match &nav {
            Nav::Open(p) => format!(
                "\"{}\"",
                p.file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| p.display().to_string())
            ),
            Nav::NewDesign => "a new design".to_owned(),
        };
        let choice = rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Warning)
            .set_title("Unsaved changes")
            .set_description(format!(
                "\"{name}\" has unsaved changes.\nSave them before opening {target}? (\"No\" discards them.)"
            ))
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
                    if let View::Gallery(g) = &mut self.view {
                        if g.folder == folder {
                            g.merge_files(files);
                        }
                    }
                }
                AppMsg::GalleryThumb {
                    folder,
                    path,
                    result,
                } => {
                    if let View::Gallery(g) = &mut self.view {
                        if g.folder == folder {
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
                                    g.set_thumb(&path, Some(tex));
                                }
                                Err(e) => {
                                    tracing::warn!("miniatura de {} falló: {e}", path.display());
                                    g.set_thumb(&path, None);
                                }
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
                    if let View::Editor(state) = &mut self.view {
                        if state.doc.source_path.as_deref() == Some(old_path.as_path()) {
                            match result {
                                Ok(new_path) => state.doc.source_path = Some(new_path),
                                // Reutiliza el banner de error que ya existe
                                // en el panel: no hace falta un campo nuevo.
                                Err(e) => state.save_error = Some(e),
                            }
                        }
                    }
                }
                AppMsg::DocumentDeleted { path, result } => {
                    // `state` toma prestado `self.view`; no se puede
                    // reasignar `self.view` mientras siga vivo, así que la
                    // decisión se guarda en una variable local y se aplica
                    // después de que el préstamo termine.
                    let mut go_to_welcome = false;
                    if let View::Editor(state) = &mut self.view {
                        if state.doc.source_path.as_deref() == Some(path.as_path()) {
                            match result {
                                // El archivo ya no existe: no tiene sentido
                                // preguntar por cambios sin guardar, se
                                // navega directo.
                                Ok(()) => match state.from_gallery.clone() {
                                    Some(folder) => open_after = Some(Nav::Open(folder)),
                                    None => go_to_welcome = true,
                                },
                                Err(e) => state.save_error = Some(e),
                            }
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
                                let mut state =
                                    editor::EditorState::from_restored(path.clone(), restored);
                                state.from_gallery = self.gallery_origin.clone();
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
                            let mut state =
                                editor::EditorState::from_design(path.clone(), restored);
                            state.from_gallery = self.gallery_origin.clone();
                            self.remember_page_size(&state.doc);
                            self.view = View::Editor(Box::new(state));
                        }
                        Ok(loader::LoadOutcome::Flat(img)) => {
                            match editor::EditorState::from_image(path.clone(), img) {
                                Ok(mut state) => {
                                    state.from_gallery = self.gallery_origin.clone();
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
        let View::Editor(state) = &mut self.view else {
            return;
        };
        if !state.is_dirty() {
            return;
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        let choice = rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Warning)
            .set_title("Unsaved changes")
            .set_description(format!(
                "\"{}\" has unsaved changes.\nSave them before closing? (\"No\" discards them.)",
                state.file_name()
            ))
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
                format!("{dirty}{} — Canvas Desktop", state.file_name())
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
) {
    if state.saving {
        return;
    }
    tracing::info!("guardando en {}", path.display());
    match renderer.bake_page(&rs.device, &rs.queue, &state.doc, &state.images, 1.0) {
        Ok((rgba, width, height)) => {
            state.saving = true;
            state.save_error = None;
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
    match renderer.bake_page(&rs.device, &rs.queue, &state.doc, &state.images, scale) {
        Ok((rgba, w, h)) => payload.preview = canvas_io::make_preview(&rgba, w, h),
        Err(e) => tracing::warn!("miniatura del diseño no horneada: {e}"),
    }
    state.saving = true;
    state.save_error = None;
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
        match renderer.bake_page(&rs.device, &rs.queue, &state.doc, &state.images, scale) {
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
                    layer.id,
                    source,
                    &layer.effects,
                );
            }
        }
    }
    let blurred = renderer.blur_overrides();
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

/// `foto.png.canvas` → `foto.png` si esa imagen existe; cualquier otra ruta
/// se devuelve tal cual. El guard exige que `inner` sea además una imagen
/// (no solo un archivo cualquiera) para que un diseño autónomo con nombre
/// `Untitled.canvas` (cuyo `inner` es `Untitled`, sin extensión) nunca se
/// confunda con el sidecar de otra cosa.
fn resolve_canvas_sidecar(path: PathBuf) -> PathBuf {
    let is_canvas = canvas_io::is_canvas_file(&path);
    if is_canvas {
        // `with_extension("")` quita solo la última extensión: .canvas.
        let inner = path.with_extension("");
        if canvas_io::is_image_file(&inner) && inner.is_file() {
            return inner;
        }
    }
    path
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

        // Fallback sin menú nativo (macOS/Linux): barra de menús egui.
        #[cfg(not(windows))]
        {
            let recents = self.settings.recent_files.clone();
            let action = egui::Panel::top("menu_bar")
                .show(ui, |ui| menus::menu_bar_ui(ui, editor_open, &recents))
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
                state.handle_shortcuts(&ctx, paste_requested);

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
                                // usuario pidiera no volver a preguntar).
                                if !self.settings.skip_overwrite_warning
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

                egui::Panel::left("layers")
                    .default_size(220.0)
                    .show(ui, |ui| layers_panel::layers_panel_ui(state, ui));
                egui::Panel::right("properties")
                    .default_size(260.0)
                    .show(ui, |ui| editor::properties_ui(state, ui));
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ui, |ui| {
                        editor::canvas_ui(state, ui, &rs, &mut self.renderer, &mut self.surface);
                    });

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
