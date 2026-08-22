//! Navegación: abrir algo (archivo/carpeta/segunda instancia/arrastrar y
//! soltar), resolver y sembrar la baraja del editor, saltar entre vistas
//! (con o sin preguntar por cambios sin guardar), y los atajos de la tira
//! (eje de apilado, lado, añadir lienzo).

use std::path::{Path, PathBuf};

use eframe::egui;

use crate::{deck, editor, gallery, loader, settings, App, Nav, View};

use super::persistence::{resolve_canvas_sidecar, seed_gallery_from_deck};

impl App {
    /// Punto único de entrada para abrir algo, venga de argv, diálogo,
    /// arrastrar y soltar, un clic en la galería o una segunda instancia.
    pub(crate) fn open_path(&mut self, path: PathBuf, ctx: &egui::Context) {
        // Un sidecar `foto.png.canvas` se abre como su imagen `foto.png`
        // (que a su vez restaura las capas del sidecar automáticamente).
        let path = resolve_canvas_sidecar(path);
        if path.is_dir() {
            // Si la baraja ya tenía esta misma carpeta (se volvió del
            // editor), siembra la rejilla con sus miniaturas ya en GPU: el
            // reescaneo que sigue solo detecta cambios en disco, no repuebla
            // desde ⏳.
            let gallery_state = seed_gallery_from_deck(
                &self.deck,
                path.clone(),
                self.settings.gallery_sort,
                self.settings.gallery_folder_panel_side,
            );
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
    /// Documento nuevo en blanco (desde la bienvenida o el menú File):
    /// hereda el tamaño de página del último documento abierto o creado, y
    /// nace en el formato elegido en Ajustes (`new_canvas_format`).
    pub(crate) fn new_design(&mut self, ctx: &egui::Context) {
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
    pub(crate) fn resolve_deck(&mut self, path: &Path, ctx: &egui::Context) {
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
        // Limpieza defensiva: si esta carpeta se cerró en falso la vez
        // anterior (crash, apagón) con algo sin deshacer en su papelera
        // propia (`GlobalStep::Delete`), no se queda ahí para siempre.
        if let Some(folder) = self.deck.folder.clone() {
            canvas_io::purge_local_trash(&folder);
        }
    }
    /// Siembra una `Deck` recién construida con las preferencias persistidas
    /// (eje de apilado, visibilidad de la tira) — `Deck::single`/`from_seed`
    /// no conocen `AppSettings`, así que el llamador las aplica justo
    /// después de construirla, antes del primer `relayout`.
    pub(crate) fn apply_deck_prefs(&mut self) {
        self.deck.axis = self.settings.deck_axis;
        self.deck.strip_visible = self.settings.deck_strip_visible;
        self.deck.strip_side = self.settings.deck_strip_side;
    }
    /// Sondea el tamaño real de las ranuras cuyo tamaño aún se desconoce.
    /// No hace nada con una baraja degenerada (`Deck::single`: sin carpeta,
    /// sin hermanos que necesiten sondeo) ni cuando ya se conocen todos.
    pub(crate) fn spawn_deck_probe(&self, ctx: &egui::Context) {
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
    pub(crate) fn toggle_deck_axis(&mut self) {
        self.deck.axis = self.deck.axis.toggled();
        self.deck.layout_dirty = true;
        self.settings.deck_axis = self.deck.axis;
        self.settings.save_in_background();
    }
    /// Mueve la tira al siguiente lado y lo persiste — el botón de la propia
    /// tira y el menú View comparten este único camino.
    pub(crate) fn cycle_strip_side(&mut self) {
        self.deck.strip_side = self.deck.strip_side.cycled();
        self.settings.deck_strip_side = self.deck.strip_side;
        self.settings.save_in_background();
    }

    /// Añade un lienzo en blanco al final de la baraja y salta a él. No hace
    /// nada con una baraja degenerada (`Deck::single`: un archivo suelto no
    /// tiene carpeta donde crear un hermano) — es el único camino a esta
    /// función cuando la tira está oculta (un solo archivo en la carpeta,
    /// donde la celda "+" todavía no existe).
    pub(crate) fn add_canvas(&mut self) {
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
    pub(crate) fn navigate(&mut self, nav: Nav, ctx: &egui::Context) {
        // Se abandona la carpeta activa (galería o baraja del editor), si
        // había una: purga su papelera propia — cualquier `Ctrl+Z` pendiente
        // sobre un borrado ya perdió su ventana, porque el `EditorState` que
        // lo llevaba (`global_undo`) está a punto de descartarse con el
        // cambio de vista de más abajo.
        let leaving_folder = match &self.view {
            View::Gallery(g) => Some(g.folder.clone()),
            View::Editor(_) => self.deck.folder.clone(),
            _ => None,
        };
        if let Some(folder) = leaving_folder {
            canvas_io::purge_local_trash(&folder);
        }
        match nav {
            Nav::Open(path) => self.open_path(path, ctx),
            Nav::OpenGallery { path, navigation } => {
                // Defensivo (crash-recovery): restos de un cierre en falso
                // de la sesión anterior en ESTA carpeta, si los hay.
                canvas_io::purge_local_trash(&path);
                let gallery_state = gallery::GalleryState::with_navigation(
                    path.clone(),
                    self.settings.gallery_sort,
                    navigation,
                    self.settings.gallery_folder_panel_side,
                );
                loader::spawn_gallery_scan(
                    path.clone(),
                    self.thumb_cache.clone(),
                    self.tx.clone(),
                    ctx.clone(),
                );
                self.push_recent(&path);
                self.view = View::Gallery(gallery_state);
                self.sync_title(ctx);
            }
            Nav::CloseProject => {
                self.deck = deck::Deck::default();
                self.pending_deck = None;
                self.watcher = None;
                self.view = View::Welcome { error: None };
                self.sync_title(ctx);
            }
            Nav::NewDesign => self.new_design(ctx),
        }
    }
    /// Navega, pero si hay algún lienzo con cambios sin guardar delante
    /// pregunta primero (Save / Discard / Cancel). «Save» solo guarda el
    /// documento ACTIVO — sigue siendo el único camino de guardado que
    /// funciona fuera del editor (`Ctrl+Alt+S`/«Save all» es una acción del
    /// editor, no de esta navegación); el texto lo dice explícitamente
    /// cuando hay más de un lienzo sucio, para que abrir algo distinto
    /// nunca pierda trabajo en silencio.
    pub(crate) fn request_nav(&mut self, nav: Nav, ctx: &egui::Context) {
        let names = self.dirty_canvas_names();
        if names.is_empty() {
            self.navigate(nav, ctx);
            return;
        }
        let target = match &nav {
            Nav::Open(p) | Nav::OpenGallery { path: p, .. } => format!(
                "\"{}\"",
                p.file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| p.display().to_string())
            ),
            Nav::CloseProject => "the Welcome screen".to_owned(),
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
}
