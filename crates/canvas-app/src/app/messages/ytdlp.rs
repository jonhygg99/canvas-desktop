use std::path::PathBuf;

use eframe::egui;

use crate::loader;

use super::super::{View, Workspace};

impl super::AppInner {
    pub(super) fn on_ytdlp_progress(
        &mut self,
        ws: &mut Workspace,
        index: usize,
        total: usize,
        text: String,
        ctx: &egui::Context,
    ) {
        if let View::Editor(state) = &mut ws.view {
            state.ytdlp.downloading = true;
            state.ytdlp.progress = format!("[{}/{total}] {text}", index + 1);
        }
        ctx.request_repaint();
    }

    pub(super) fn on_ytdlp_done(
        &mut self,
        ws: &mut Workspace,
        folder: PathBuf,
        paths: Vec<PathBuf>,
        errors: Vec<String>,
        ctx: &egui::Context,
    ) {
        if let View::Editor(state) = &mut ws.view {
            state.ytdlp.downloading = false;
            state.ytdlp.done = paths.clone();
            state.ytdlp.error = if errors.is_empty() {
                None
            } else {
                Some(errors.join("\n").chars().take(500).collect())
            };
            state.ytdlp.progress = if paths.is_empty() {
                String::new()
            } else {
                format!("{} clip(s) ready", paths.len())
            };
        }
        // Refrescar la galería / miniaturas si la carpeta coincide.
        let in_gallery = matches!(&ws.view, View::Gallery(g) if g.folder == folder);
        if in_gallery {
            self.rescan_gallery(ws, ctx);
        } else {
            for path in &paths {
                loader::spawn_single_thumb(
                    folder.clone(),
                    path.clone(),
                    self.thumb_cache.clone(),
                    ws.tx.clone(),
                    ctx.clone(),
                );
            }
        }
        ctx.request_repaint();
    }
}
