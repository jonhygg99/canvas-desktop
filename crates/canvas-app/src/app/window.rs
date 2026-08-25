//! Ventana/SO: título, cierre con confirmación de cambios sin guardar,
//! archivos soltados sobre la ventana, caché de miniaturas, y los
//! "recientes" (ajustes + menú + Jump List de Windows).

use std::path::PathBuf;

use canvas_shell::ShellIntegration as _;
use eframe::egui;

use crate::loader;

use super::{AppInner, View, Workspace};

impl AppInner {
    /// Apunta lo abierto en los recientes: ajustes, menú y Jump List del SO.
    pub(crate) fn push_recent(&mut self, path: &std::path::Path) {
        if !path.is_dir() {
            return;
        }
        let path = path.to_owned();
        self.settings.recent_files.retain(|p| p != &path);
        self.settings.recent_files.insert(0, path);
        self.settings.recent_files.truncate(10);
        self.settings.save_in_background();
        // El menú nativo (si existe) se entera del cambio vía el espejo de
        // `App::sync_native_menu` al final del frame raíz.
        let recents = self.settings.recent_files.clone();
        std::thread::spawn(move || {
            if let Err(e) = canvas_shell::platform().update_jump_list(&recents) {
                tracing::debug!("jump list no actualizada: {e}");
            }
        });
    }

    /// Recuerda el tamaño de página para el próximo diseño nuevo.
    pub(crate) fn remember_page_size(&mut self, ws: &Workspace, doc: &canvas_core::Document) {
        let Ok(page) = doc.page() else { return };
        let size = (page.width, page.height);
        if self.settings.last_page_size != size {
            self.settings.last_page_size = size;
            self.settings.save_in_background();
        }
        let _ = ws;
    }

    /// Archivos soltados sobre la ventana.
    pub(crate) fn handle_dropped_files(&mut self, ws: &mut Workspace, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });
        if let Some(path) = dropped.into_iter().next() {
            if matches!(ws.view, View::Editor(_))
                && path.is_file()
                && canvas_io::is_image_file(&path)
            {
                loader::spawn_load_image_as_layer(path, ws.tx.clone(), ctx.clone());
            } else {
                self.open_path(ws, path, ctx);
            }
        }
    }

    /// Mantiene el título de la ventana (con asterisco de cambios sin
    /// guardar) al día; solo envía el comando cuando cambia.
    pub(crate) fn sync_title(&mut self, ctx: &egui::Context, ws: &mut Workspace) {
        let title = match &ws.view {
            View::Editor(state) => {
                let dirty = if state.is_dirty() { "*" } else { "" };
                let position = if ws.deck.slots.len() > 1 {
                    format!(" ({}/{})", ws.deck.active + 1, ws.deck.slots.len())
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
        if title != ws.last_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            ws.last_title = title;
        }
    }

    /// Cierre de una ventana (X de su barra de título o `Quit` de su menú):
    /// pregunta por cambios sin guardar y, si procede (Save en curso o
    /// Discard), deja que el cierre continúe. Para la raíz reenvía el
    /// `Close` (el `CancelClose` lo canceló); para una hija marca
    /// `close_requested` para que el frame raíz la retire.
    pub(crate) fn confirm_window_close(
        &mut self,
        ws: &mut Workspace,
        ctx: &egui::Context,
        is_root: bool,
    ) {
        if ws.save.allow_close {
            return;
        }
        if !matches!(ws.view, View::Editor(_)) {
            ws.save.allow_close = true;
            if !is_root {
                ws.close_requested = true;
            }
            return;
        }
        let names = ws.dirty_canvas_names();
        if names.is_empty() {
            ws.save.allow_close = true;
            if !is_root {
                ws.close_requested = true;
            }
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
        let finish_close = |ws: &mut Workspace| {
            ws.save.allow_close = true;
            if is_root {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else {
                ws.close_requested = true;
            }
        };
        match choice {
            rfd::MessageDialogResult::Yes => {
                ws.save.save_requested = true;
                ws.save.close_after_save = true;
            }
            rfd::MessageDialogResult::Custom(c) if c == "Save" => {
                ws.save.save_requested = true;
                ws.save.close_after_save = true;
            }
            rfd::MessageDialogResult::No => finish_close(ws),
            rfd::MessageDialogResult::Custom(c) if c == "Discard" => finish_close(ws),
            _ => {}
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
