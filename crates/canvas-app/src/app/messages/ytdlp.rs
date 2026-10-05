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
            // La lista acumula: lo descargado se queda hasta que se borre
            // (los que ya no existen en disco se podan).
            state.ytdlp.done = merge_done(&state.ytdlp.done, paths.clone());
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

    pub(super) fn on_ytdlp_files_deleted(
        &mut self,
        ws: &mut Workspace,
        removed: Vec<PathBuf>,
        errors: Vec<String>,
    ) {
        if let View::Editor(state) = &mut ws.view {
            state.ytdlp.done.retain(|p| !removed.contains(p));
            if !errors.is_empty() {
                state.ytdlp.error = Some(errors.join("\n").chars().take(500).collect());
            }
        }
    }

    pub(super) fn on_ytdlp_frames_ready(
        &mut self,
        ws: &mut Workspace,
        done: crate::loader::YtdlpFramesOutcome,
    ) {
        if let View::Editor(state) = &mut ws.view {
            if let Some(edit) = state.ytdlp.edit.as_mut() {
                if edit.matches(&done.clip_id) {
                    edit.set_source_fps(done.source_fps);
                    edit.set_frames(done.files, done.fps, done.duration, done.video_size);
                }
            }
        }
    }

    pub(super) fn on_ytdlp_frames_failed(
        &mut self,
        ws: &mut Workspace,
        clip_id: String,
        error: String,
    ) {
        if let View::Editor(state) = &mut ws.view {
            if let Some(edit) = state.ytdlp.edit.as_mut() {
                if edit.matches(&clip_id) {
                    edit.set_frames_error(error);
                }
            }
        }
    }

    pub(super) fn on_ytdlp_edit_accepted(
        &mut self,
        ws: &mut Workspace,
        accept: crate::ytdlp::VideoAccept,
        ctx: &egui::Context,
    ) {
        // Con guard de cambios sin guardar: la creación viaja como Nav.
        self.request_nav(ws, super::super::Nav::OpenVideo { accept }, ctx);
    }
}

/// Junta la tanda nueva con la lista visible: conserva lo anterior que siga
/// en disco y añade lo nuevo sin duplicar. Pura (testeable).
fn merge_done(old: &[PathBuf], paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = old.iter().filter(|p| p.is_file()).cloned().collect();
    for p in paths {
        if !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::merge_done;

    #[test]
    fn done_accumulates_and_prunes_missing() {
        let dir = tempfile::tempdir().expect("temp");
        let a = dir.path().join("a.mp4");
        let gone = dir.path().join("gone.mp4");
        std::fs::write(&a, [0u8; 2]).expect("a");
        let b = dir.path().join("b.mp4");
        std::fs::write(&b, [0u8; 2]).expect("b");
        // `a` sigue, `gone` ya no existe, `b` es nuevo (y `a` no duplica).
        assert_eq!(
            merge_done(&[a.clone(), gone], vec![a.clone(), b.clone()]),
            vec![a, b]
        );
    }
}
