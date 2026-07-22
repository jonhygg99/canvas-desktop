//! Binario de Canvas Desktop: ventana eframe/egui con el lienzo vello.

mod editor;
mod gallery;
mod loader;
mod menus;
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

    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([640.0, 480.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Canvas Desktop",
        options,
        Box::new(move |cc| Ok(Box::new(App::new(cc, initial_path, instance)?))),
    )
    .map_err(|e| anyhow!("no se pudo arrancar la ventana: {e}"))
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

    /// Documento nuevo en blanco (desde la bienvenida o el menú File).
    fn new_design(&mut self, ctx: &egui::Context) {
        self.gallery_origin = None;
        let mut state = editor::EditorState::new_blank(1920.0, 1080.0);
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
            A::NewDesign => self.request_nav(Nav::NewDesign, ctx),
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
                AppMsg::ImageLoadedForLayer { path, result } => {
                    if let View::Editor(state) = &mut self.view {
                        match result {
                            Ok(img) => state.add_image_layer(path, img),
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
                            g.set_files(files);
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
                        Ok(loader::LoadOutcome::Flat(img)) => {
                            match editor::EditorState::from_image(path.clone(), img) {
                                Ok(mut state) => {
                                    state.from_gallery = self.gallery_origin.clone();
                                    state.sidecar_enabled = self.settings.sidecar_default;
                                    state.source_metadata = metadata;
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

/// ¿La extensión de `path` es JPEG? (para el aviso de calidad de recompresión)
fn is_jpeg_path(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "jpg" | "jpeg"))
}

/// `foto.png.canvas` → `foto.png` si esa imagen existe; cualquier otra ruta
/// se devuelve tal cual.
fn resolve_canvas_sidecar(path: PathBuf) -> PathBuf {
    let is_canvas = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("canvas"));
    if is_canvas {
        // `with_extension("")` quita solo la última extensión: .canvas.
        let inner = path.with_extension("");
        if inner.is_file() {
            return inner;
        }
    }
    path
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

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
                match welcome::show(ui, error.as_deref(), &self.settings.recent_files) {
                    Some(welcome::WelcomeAction::NewProject) => {
                        self.gallery_origin = None;
                        let mut state = editor::EditorState::new_blank(1920.0, 1080.0);
                        state.sidecar_enabled = self.settings.sidecar_default;
                        self.view = View::Editor(Box::new(state));
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
                None => {}
            },
            View::Editor(state) => {
                let Some(rs) = frame.wgpu_render_state().cloned() else {
                    return;
                };
                state.handle_shortcuts(&ctx);

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
                    loader::spawn_pick_save_path(
                        Some(state.file_name()),
                        self.tx.clone(),
                        ctx.clone(),
                    );
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
                // persistido: cambiarlo ahí lo recuerda para el futuro.
                if state.sidecar_enabled != self.settings.sidecar_default {
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
