//! Navegación: abrir algo (archivo/carpeta/segunda instancia/arrastrar y
//! soltar), resolver y sembrar la baraja del editor, saltar entre vistas
//! (con o sin preguntar por cambios sin guardar), y los atajos de la tira
//! (eje de apilado, lado, añadir lienzo).

use std::path::{Path, PathBuf};

use eframe::egui;

use crate::{deck, editor, gallery, loader, settings};

use super::{AppInner, Nav, View, Workspace};

use super::persistence::{resolve_canvas_sidecar, seed_gallery_from_deck};

const DEFAULT_NEW_CANVAS_SIZE: (f64, f64) = (1920.0, 1080.0);

impl AppInner {
    /// Punto único de entrada para abrir algo en UN workspace, venga de
    /// argv, diálogo, arrastrar y soltar, un clic en la galería o una
    /// segunda instancia.
    pub(crate) fn open_path(&mut self, ws: &mut Workspace, path: PathBuf, ctx: &egui::Context) {
        let path = resolve_canvas_sidecar(path);
        if path.is_dir() {
            let path = gallery::normalize_folder(path);
            let gallery_state = seed_gallery_from_deck(
                &ws.deck,
                path.clone(),
                self.settings.gallery_sort,
                self.settings.gallery_folder_panel_side,
            );
            loader::spawn_gallery_scan(
                path.clone(),
                self.thumb_cache.clone(),
                ws.tx.clone(),
                ctx.clone(),
            );
            self.push_recent(&path);
            ws.view = View::Gallery(Box::new(gallery_state));
        } else if canvas_io::is_canvas_file(&path) {
            loader::spawn_load_design(path.clone(), ws.tx.clone(), ctx.clone());
            self.push_recent(&path);
            ws.view = View::Loading { path };
        } else if canvas_io::is_image_file(&path) {
            loader::spawn_load_image(path.clone(), true, ws.tx.clone(), ctx.clone());
            self.push_recent(&path);
            ws.view = View::Loading { path };
        } else {
            ws.view = View::Welcome {
                error: Some(format!(
                    "\"{}\" is not a supported image format.",
                    path.display()
                )),
            };
        }
        self.sync_title(ctx, ws);
    }

    /// Documento nuevo en blanco en un workspace.
    pub(crate) fn new_design(&mut self, ws: &mut Workspace, ctx: &egui::Context) {
        let (w, h) = DEFAULT_NEW_CANVAS_SIZE;
        ws.deck = deck::Deck::single(PathBuf::new());
        self.apply_deck_prefs(ws);
        if let Some(slot) = ws.deck.slots.first_mut() {
            slot.page = Some((w, h));
            slot.is_placeholder = true;
            slot.name = "Untitled".to_owned();
        }
        let state = if self.settings.new_canvas_format == settings::NewCanvasFormat::Canvas {
            let mut state = editor::EditorState::new_blank(w, h);
            state.sidecar_enabled = self.settings.sidecar_default;
            state
        } else {
            editor::EditorState::new_blank_image(w, h)
        };
        ws.view = View::Editor(Box::new(state));
        self.sync_title(ctx, ws);
    }

    /// Decide qué baraja usar al terminar de cargar `path` en el editor.
    pub(crate) fn resolve_deck(&mut self, ws: &mut Workspace, path: &Path, ctx: &egui::Context) {
        if let Some(seed) = ws.deck_ops.pending_deck.take() {
            self.initialize_seeded_deck(ws, seed, path);
            self.start_seeded_deck_preload(ws, ctx);
        } else if let Some(idx) = ws.deck.find_by_path(path) {
            ws.deck.active = idx;
        } else {
            ws.deck = deck::Deck::single(path.to_path_buf());
            self.apply_deck_prefs(ws);
        }
        if let Some(folder) = ws.deck.folder.clone() {
            canvas_io::purge_local_trash(&folder);
        }
    }

    fn initialize_seeded_deck(
        &mut self,
        ws: &mut Workspace,
        seed: deck::DeckSeed,
        active_path: &Path,
    ) {
        ws.deck = deck::Deck::from_seed(seed, active_path);
        self.apply_deck_prefs(ws);
    }

    fn start_seeded_deck_preload(&mut self, ws: &mut Workspace, ctx: &egui::Context) {
        self.spawn_deck_probe(ctx, ws);
        self.preload_nearby_slots(ws, ctx);
    }

    fn preload_nearby_slots(&mut self, ws: &mut Workspace, ctx: &egui::Context) {
        let Some(folder) = ws.deck.folder.clone() else {
            return;
        };
        for path in ws.deck.request_loads(&[]) {
            loader::spawn_load_slot(
                folder.clone(),
                path,
                ws.deck.generation(),
                self.settings.sidecar_default,
                ws.tx.clone(),
                ctx.clone(),
            );
        }
    }

    /// Aplica preferencias persistidas a una baraja recién construida.
    pub(crate) fn apply_deck_prefs(&mut self, ws: &mut Workspace) {
        ws.deck.axis = self.settings.deck_axis;
        ws.deck.strip_visible = self.settings.deck_strip_visible;
        ws.deck.strip_side = self.settings.deck_strip_side;
    }

    /// Sondea el tamaño real de las ranuras cuyo tamaño aún se desconoce.
    pub(crate) fn spawn_deck_probe(&self, ctx: &egui::Context, ws: &Workspace) {
        let Some(folder) = ws.deck.folder.clone() else {
            return;
        };
        let paths: Vec<PathBuf> = ws
            .deck
            .slots
            .iter()
            .filter(|s| s.page.is_none())
            .map(|s| s.path.clone())
            .collect();
        if paths.is_empty() {
            return;
        }
        loader::spawn_deck_probe(
            folder,
            ws.deck.generation(),
            paths,
            ws.tx.clone(),
            ctx.clone(),
        );
    }

    /// Alterna el eje de apilado de la baraja activa y lo persiste.
    pub(crate) fn toggle_deck_axis(&mut self, ws: &mut Workspace) {
        ws.deck.axis = ws.deck.axis.toggled();
        ws.deck.layout_dirty = true;
        self.settings.deck_axis = ws.deck.axis;
        self.settings.save_in_background();
    }

    /// Mueve la tira al siguiente lado y lo persiste.
    pub(crate) fn cycle_strip_side(&mut self, ws: &mut Workspace) {
        ws.deck.strip_side = ws.deck.strip_side.cycled();
        self.settings.deck_strip_side = ws.deck.strip_side;
        self.settings.save_in_background();
    }

    /// Añade un lienzo en blanco al final de la baraja activa y salta a él.
    pub(crate) fn add_canvas(&mut self, ws: &mut Workspace) {
        let ext = self.settings.new_canvas_format.extension();
        match ws.deck.push_placeholder(self.settings.last_page_size, ext) {
            Some(idx) => {
                ws.deck.jump_to = Some(idx);
                ws.deck.jump_reframe = true;
            }
            None => tracing::info!("«Add canvas» sin efecto: la baraja no tiene carpeta"),
        }
    }

    fn new_design_in_folder(
        &mut self,
        ws: &mut Workspace,
        seed: deck::DeckSeed,
        ctx: &egui::Context,
    ) {
        let page = self.settings.last_page_size;
        let ext = self.settings.new_canvas_format.extension();
        self.initialize_seeded_deck(ws, seed, Path::new(""));
        let Some(idx) = ws.deck.push_placeholder(page, ext) else {
            self.new_design(ws, ctx);
            return;
        };
        for (slot_index, slot) in ws.deck.slots.iter_mut().enumerate() {
            if slot_index != idx && matches!(slot.content, deck::SlotContent::Active) {
                slot.content = deck::SlotContent::Idle;
            }
        }
        let mut state = if ext == canvas_io::CANVAS_EXTENSION {
            editor::EditorState::new_blank(page.0, page.1)
        } else {
            editor::EditorState::new_blank_image(page.0, page.1)
        };
        state.from_gallery = ws.deck.folder.clone();
        ws.deck.active = idx;
        if let Some(slot) = ws.deck.slots.get_mut(idx) {
            slot.content = deck::SlotContent::Active;
        }
        self.start_seeded_deck_preload(ws, ctx);
        ws.view = View::Editor(Box::new(state));
        ws.deck.layout_dirty = true;
        self.sync_title(ctx, ws);
    }

    /// Aplica una navegación diferida sobre un workspace.
    pub(crate) fn navigate(&mut self, ws: &mut Workspace, nav: Nav, ctx: &egui::Context) {
        let leaving_folder = match &ws.view {
            View::Gallery(g) => Some(g.folder.clone()),
            View::Editor(_) => ws.deck.folder.clone(),
            _ => None,
        };
        if let Some(folder) = leaving_folder {
            canvas_io::purge_local_trash(&folder);
        }
        match nav {
            Nav::Open(path) => self.open_path(ws, path, ctx),
            Nav::OpenGallery { path, navigation } => {
                let path = gallery::normalize_folder(path);
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
                    ws.tx.clone(),
                    ctx.clone(),
                );
                self.push_recent(&path);
                ws.view = View::Gallery(Box::new(gallery_state));
                self.sync_title(ctx, ws);
            }
            Nav::CloseProject => {
                ws.deck = deck::Deck::default();
                ws.deck_ops.pending_deck = None;
                ws.watcher = None;
                ws.view = View::Welcome { error: None };
                self.sync_title(ctx, ws);
            }
            Nav::NewDesign => self.new_design(ws, ctx),
            Nav::NewDesignInFolder { seed } => self.new_design_in_folder(ws, seed, ctx),
        }
    }

    /// Navega un workspace, pero si hay algún lienzo con cambios sin guardar
    /// delante pregunta primero (Save / Discard / Cancel). El modal corre en
    /// un hilo aparte y responde por `AppMsg::UnsavedDialogAnswer`: un
    /// `rfd::…::show()` sincrónico dentro del pase de un viewport diferido
    /// congela todo el event loop multi-ventana.
    pub(crate) fn request_nav(&mut self, ws: &mut Workspace, nav: Nav, ctx: &egui::Context) {
        let names = ws.dirty_canvas_names();
        if names.is_empty() {
            self.navigate(ws, nav, ctx);
            return;
        }
        if ws.unsaved_dialog.is_some() {
            return; // ya hay un diálogo en vuelo para esta ventana
        }
        ws.unsaved_dialog = Some(super::UnsavedDialog::Navigate(nav.clone()));
        let target = match &nav {
            Nav::Open(p) | Nav::OpenGallery { path: p, .. } => format!(
                "\"{}\"",
                p.file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| p.display().to_string())
            ),
            Nav::CloseProject => "the Welcome screen".to_owned(),
            Nav::NewDesign => "a new design".to_owned(),
            Nav::NewDesignInFolder { .. } => "a new design".to_owned(),
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
        let tx = ws.tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
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
            use crate::loader::{AppMsg, DialogDecision};
            let decision = match choice {
                rfd::MessageDialogResult::Yes => DialogDecision::Save,
                rfd::MessageDialogResult::Custom(ref c) if c == "Save" => DialogDecision::Save,
                rfd::MessageDialogResult::No => DialogDecision::Discard,
                rfd::MessageDialogResult::Custom(ref c) if c == "Discard" => {
                    DialogDecision::Discard
                }
                _ => DialogDecision::Cancel,
            };
            let _ = tx.send(AppMsg::UnsavedDialogAnswer(decision));
            ctx.request_repaint();
        });
    }
}
