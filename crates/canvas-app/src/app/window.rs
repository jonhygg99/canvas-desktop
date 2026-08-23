//! Ventana/SO: título, cierre con confirmación de cambios sin guardar,
//! archivos soltados sobre la ventana, caché de miniaturas, y los
//! "recientes" (ajustes + menú + Jump List de Windows).

use std::path::PathBuf;

use canvas_shell::ShellIntegration as _;
use eframe::egui;

use crate::{deck, loader};

use super::{App, View};

impl App {
    /// Apunta lo abierto en los recientes: ajustes, menú y Jump List del SO.
    pub(super) fn push_recent(&mut self, path: &std::path::Path) {
        if !path.is_dir() {
            return;
        }
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
    /// Recuerda el tamaño de página para el próximo diseño nuevo, sin
    /// escribir ajustes si no cambió (`save_in_background` lanza un hilo).
    pub(super) fn remember_page_size(&mut self, doc: &canvas_core::Document) {
        let Ok(page) = doc.page() else { return };
        let size = (page.width, page.height);
        if self.settings.last_page_size != size {
            self.settings.last_page_size = size;
            self.settings.save_in_background();
        }
    }
    /// Nombres de todos los lienzos con cambios sin guardar — la activa
    /// primero, si lo está, luego el resto de la baraja — para los diálogos
    /// de «cambios sin guardar». Desde que hay N lienzos cargados a la vez
    /// (Fase 14c), un solo `state.is_dirty()` ya no cuenta la historia
    /// entera: una ranura de fondo puede estar sucia sin que el documento
    /// activo lo esté.
    pub(super) fn dirty_canvas_names(&self) -> Vec<String> {
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
    pub(super) fn handle_dropped_files(&mut self, ctx: &egui::Context) {
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
    pub(super) fn confirm_close(&mut self, ctx: &egui::Context) {
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
    pub(super) fn sync_title(&mut self, ctx: &egui::Context) {
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
pub(super) fn thumbnail_cache_dir() -> Option<PathBuf> {
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
