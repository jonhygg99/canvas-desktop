//! Binario de Canvas Desktop: ventana eframe/egui con el lienzo vello.

mod editor;
mod gallery;
mod loader;
mod surface;
mod welcome;

use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};

use anyhow::{anyhow, Context, Result};
use canvas_render::CanvasRenderer;
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

    let initial_path = canvas_shell::open_paths_from_args(std::env::args())
        .into_iter()
        .next();

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
        Box::new(move |cc| Ok(Box::new(App::new(cc, initial_path)?))),
    )
    .map_err(|e| anyhow!("no se pudo arrancar la ventana: {e}"))
}

enum View {
    Welcome { error: Option<String> },
    Loading { path: PathBuf },
    Gallery(gallery::GalleryState),
    Editor(Box<editor::EditorState>),
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
    /// Volver a esta galería en cuanto termine el guardado en curso.
    return_to: Option<PathBuf>,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>, initial_path: Option<PathBuf>) -> Result<Self> {
        let rs = cc
            .wgpu_render_state
            .as_ref()
            .context("eframe no ha inicializado wgpu (¿backend glow activo?)")?;
        let renderer = CanvasRenderer::new(&rs.device)?;
        let (tx, rx) = channel();

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
            return_to: None,
        };
        if let Some(path) = initial_path {
            app.open_path(path, &cc.egui_ctx);
        }
        Ok(app)
    }

    /// Punto único de entrada para abrir algo, venga de argv, diálogo,
    /// arrastrar y soltar o un clic en la galería.
    fn open_path(&mut self, path: PathBuf, ctx: &egui::Context) {
        if path.is_dir() {
            self.gallery_origin = Some(path.clone());
            loader::spawn_gallery_scan(
                path.clone(),
                self.thumb_cache.clone(),
                self.tx.clone(),
                ctx.clone(),
            );
            self.view = View::Gallery(gallery::GalleryState::new(path));
        } else if canvas_io::is_image_file(&path) {
            // Abrir un archivo que no viene de la galería actual rompe el
            // vínculo con ella (el botón «volver» desaparece).
            if self.gallery_origin.as_deref() != path.parent() {
                self.gallery_origin = None;
            }
            loader::spawn_load_image(path.clone(), self.tx.clone(), ctx.clone());
            self.view = View::Loading { path };
        } else {
            self.view = View::Welcome {
                error: Some(format!(
                    "«{}» no es un formato de imagen compatible.",
                    path.display()
                )),
            };
        }
        self.sync_title(ctx);
    }

    fn handle_messages(&mut self, ctx: &egui::Context) {
        // Aperturas diferidas para no pelear con el préstamo de self.view.
        let mut open_after: Option<PathBuf> = None;
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
                                if new_source {
                                    state.doc.source_path = Some(path);
                                }
                                if self.close_after_save {
                                    self.allow_close = true;
                                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                                } else if let Some(folder) = self.return_to.take() {
                                    open_after = Some(folder);
                                }
                            }
                            Err(e) => {
                                self.close_after_save = false;
                                self.return_to = None;
                                state.save_error = Some(e);
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
                    index,
                    result,
                } => {
                    if let View::Gallery(g) = &mut self.view {
                        if g.folder == folder {
                            if let Some(item) = g.items.get_mut(index) {
                                match result {
                                    Ok(img) => {
                                        let color = egui::ColorImage::from_rgba_unmultiplied(
                                            [img.width as usize, img.height as usize],
                                            &img.rgba,
                                        );
                                        item.tex = Some(ctx.load_texture(
                                            item.name.clone(),
                                            color,
                                            egui::TextureOptions::LINEAR,
                                        ));
                                    }
                                    Err(e) => {
                                        item.failed = true;
                                        tracing::warn!(
                                            "miniatura de {} falló: {e}",
                                            item.path.display()
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
                AppMsg::ImageLoaded { path, result } => {
                    // Ignora cargas que ya no corresponden a la vista actual.
                    let expected = matches!(&self.view, View::Loading { path: p } if *p == path);
                    if !expected {
                        continue;
                    }
                    match result.and_then(|img| {
                        editor::EditorState::from_image(path.clone(), img)
                            .map_err(|e| e.to_string())
                    }) {
                        Ok(mut state) => {
                            state.from_gallery = self.gallery_origin.clone();
                            self.view = View::Editor(Box::new(state));
                        }
                        Err(e) => {
                            self.view = View::Welcome {
                                error: Some(format!("No se pudo abrir «{}»: {e}", path.display())),
                            };
                        }
                    }
                    self.sync_title(ctx);
                }
            }
        }
        if let Some(path) = open_after {
            self.open_path(path, ctx);
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
            self.open_path(path, ctx);
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
            .set_title("Cambios sin guardar")
            .set_description(format!(
                "«{}» tiene cambios sin guardar.\n¿Quieres guardarlos antes de cerrar? («No» los descarta.)",
                state.file_name()
            ))
            .set_buttons(rfd::MessageButtons::YesNoCancelCustom(
                "Guardar".to_owned(),
                "Descartar".to_owned(),
                "Cancelar".to_owned(),
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
            rfd::MessageDialogResult::Custom(c) if c == "Guardar" => {
                self.save_requested = true;
                self.close_after_save = true;
            }
            rfd::MessageDialogResult::No => {
                self.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            rfd::MessageDialogResult::Custom(c) if c == "Descartar" => {
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
                "Cargando {}… — Canvas Desktop",
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
fn start_save(
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
    tracing::info!("guardando en {}", path.display());
    match renderer.bake_page(&rs.device, &rs.queue, &state.doc, &state.images, 1.0) {
        Ok((rgba, width, height)) => {
            state.saving = true;
            state.save_error = None;
            loader::spawn_save(
                path,
                rgba,
                width,
                height,
                new_source,
                tx.clone(),
                ctx.clone(),
            );
        }
        Err(e) => {
            tracing::error!("horneado falló: {e}");
            state.save_error = Some(format!("No se pudo preparar el guardado: {e}"));
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.handle_messages(&ctx);
        self.handle_dropped_files(&ctx);
        self.confirm_close(&ctx);

        // Navegación diferida (clic en galería, volver desde el editor).
        let mut open_next: Option<PathBuf> = None;

        match &mut self.view {
            View::Welcome { error } => {
                let error = error.clone();
                match welcome::show(ui, error.as_deref()) {
                    Some(welcome::WelcomeAction::OpenFile) => {
                        loader::spawn_pick_file(self.tx.clone(), ctx.clone());
                    }
                    Some(welcome::WelcomeAction::OpenFolder) => {
                        loader::spawn_pick_folder(self.tx.clone(), ctx.clone());
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
                        ui.label(format!("Cargando {name}…"));
                    });
                });
            }
            View::Gallery(g) => {
                if let Some(gallery::GalleryAction::Open(path)) = gallery::show(g, ui) {
                    open_next = Some(path);
                }
            }
            View::Editor(state) => {
                let Some(rs) = frame.wgpu_render_state().cloned() else {
                    return;
                };
                state.handle_shortcuts(&ctx);

                // Volver a la galería (preguntando si hay cambios sin guardar).
                if state.return_requested {
                    state.return_requested = false;
                    if let Some(folder) = state.from_gallery.clone() {
                        if !state.is_dirty() {
                            open_next = Some(folder);
                        } else {
                            let choice = rfd::MessageDialog::new()
                                .set_level(rfd::MessageLevel::Warning)
                                .set_title("Cambios sin guardar")
                                .set_description(format!(
                                    "«{}» tiene cambios sin guardar.\n¿Quieres guardarlos antes de volver a la galería? («No» los descarta.)",
                                    state.file_name()
                                ))
                                .set_buttons(rfd::MessageButtons::YesNoCancelCustom(
                                    "Guardar".to_owned(),
                                    "Descartar".to_owned(),
                                    "Cancelar".to_owned(),
                                ))
                                .show();
                            // Igual que en confirm_close: en Windows el
                            // resultado llega como Yes/No/Cancel, no Custom.
                            match choice {
                                rfd::MessageDialogResult::Yes => {
                                    self.save_requested = true;
                                    self.return_to = Some(folder);
                                }
                                rfd::MessageDialogResult::Custom(c) if c == "Guardar" => {
                                    self.save_requested = true;
                                    self.return_to = Some(folder);
                                }
                                rfd::MessageDialogResult::No => {
                                    open_next = Some(folder);
                                }
                                rfd::MessageDialogResult::Custom(c) if c == "Descartar" => {
                                    open_next = Some(folder);
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
                    match state.doc.source_path.clone() {
                        Some(path) => {
                            start_save(state, &mut self.renderer, &rs, &self.tx, &ctx, path, false)
                        }
                        // Sin origen en disco: cae a «Guardar como…».
                        None => loader::spawn_pick_save_path(
                            Some(state.file_name()),
                            self.tx.clone(),
                            ctx.clone(),
                        ),
                    }
                }
                if let Some(path) = self.pending_save_as.take() {
                    start_save(state, &mut self.renderer, &rs, &self.tx, &ctx, path, true);
                }

                egui::Panel::right("properties")
                    .default_size(260.0)
                    .show(ui, |ui| editor::properties_ui(state, ui));
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ui, |ui| {
                        editor::canvas_ui(state, ui, &rs, &mut self.renderer, &mut self.surface);
                    });
            }
        }
        if let Some(path) = open_next {
            self.open_path(path, &ctx);
        }
        self.sync_title(&ctx);
    }
}
