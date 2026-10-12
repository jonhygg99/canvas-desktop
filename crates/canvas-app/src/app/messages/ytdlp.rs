use std::path::PathBuf;

use eframe::egui;

use crate::loader;

use super::super::{View, Workspace};

impl super::AppInner {
    pub(super) fn on_ytdlp_item(&mut self, ws: &mut Workspace, outcome: loader::YtdlpItemOutcome) {
        if let View::Editor(state) = &mut ws.view {
            state.ytdlp.finish_download_item(outcome);
        }
    }

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
            state.ytdlp.cancel = None;
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

    // Asociada (sin `self`: solo toca `ws`), como
    // `on_image_loaded_for_layer`: así los tests la ejercen sin GPU.
    pub(super) fn on_ytdlp_frames_ready(
        ws: &mut Workspace,
        done: crate::loader::YtdlpFramesOutcome,
    ) {
        let loader::YtdlpFramesOutcome {
            clip_id,
            files,
            fps,
            source_fps,
            duration,
            video_size,
        } = done;
        // El directorio se calcula una sola vez: lo usa la caché cuando hay
        // consumidor y el borrado de huérfanos cuando no lo hay.
        let dir = files
            .first()
            .and_then(|file| file.parent().map(|dir| dir.to_path_buf()));
        let mut consumed = false;
        if let View::Editor(state) = &mut ws.view {
            if let Some(edit) = state.ytdlp.edit.as_mut() {
                // Solo la extracción vigente: una superseded tardía (reintento
                // o ventana cerrada y reabierta) no pisa el resultado bueno.
                if edit.matches(&clip_id) && edit.waiting_for_frames() {
                    let mtime = std::fs::metadata(edit.clip_path())
                        .and_then(|meta| meta.modified())
                        .ok();
                    edit.set_source_fps(source_fps);
                    edit.set_frames(files.clone(), fps, duration, video_size);
                    if let (Some(mtime), Some(dir)) = (mtime, dir.clone()) {
                        state.ytdlp.frames_cache.put(
                            edit.clip_path().clone(),
                            mtime,
                            dir,
                            crate::ytdlp::CachedFrames {
                                files: files.clone(),
                                fps,
                                source_fps,
                                duration,
                                video_size,
                            },
                        );
                    }
                    consumed = true;
                }
            }
        }
        if !consumed {
            // Sin consumidor: el directorio huérfano se borra para no acumular
            // basura en el temporal.
            if let Some(dir) = dir {
                let _ = std::fs::remove_dir_all(dir);
            }
        }
    }

    // Asociada (sin `self`), igual que la anterior.
    pub(super) fn on_ytdlp_frames_failed(ws: &mut Workspace, clip_id: String, error: String) {
        if let View::Editor(state) = &mut ws.view {
            if let Some(edit) = state.ytdlp.edit.as_mut() {
                // Igual que el camino bueno: un fallo tardío de una
                // extracción superseded no pisa el resultado vigente.
                if edit.matches(&clip_id) && edit.waiting_for_frames() {
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
    use super::super::AppInner;
    use super::*;
    use crate::editor::EditorState;

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

    /// Editor con una sesión de edición viva para `clip_id` (esperando sus
    /// fotogramas, como recién abierto). Sin GPU: `EditorState` y
    /// `VideoEdit::open` son CPU puros.
    fn editor_with_edit(clip_id: &str, clip_path: std::path::PathBuf) -> Workspace {
        let mut ws = Workspace::new(egui::ViewportId::ROOT);
        let mut state = EditorState::new_blank_image(8.0, 8.0);
        let doc = canvas_core::Document::new(8.0, 8.0);
        state.ytdlp.edit = Some(crate::ytdlp::edit::VideoEdit::open(
            clip_id,
            "clip".to_owned(),
            clip_path,
            Some(10.0),
            (1920.0, 1080.0),
            &doc,
        ));
        ws.view = View::Editor(Box::new(state));
        ws
    }

    fn frames_outcome(
        clip_id: &str,
        dir: &std::path::Path,
        name: &str,
    ) -> loader::YtdlpFramesOutcome {
        let file = dir.join(name);
        std::fs::write(&file, [0u8; 4]).expect("frame");
        loader::YtdlpFramesOutcome {
            clip_id: clip_id.to_owned(),
            files: vec![file],
            fps: 1.0,
            source_fps: 30.0,
            duration: 10.0,
            video_size: None,
        }
    }

    fn edit_of(ws: &Workspace) -> &crate::ytdlp::edit::VideoEdit {
        let View::Editor(state) = &ws.view else {
            unreachable!("el editor sigue abierto");
        };
        state.ytdlp.edit.as_ref().expect("sesión viva")
    }

    #[test]
    fn stale_frames_are_dropped_and_orphan_dirs_removed() {
        let tmp = tempfile::tempdir().expect("temp");
        // Clip real para la sesión viva (su mtime alimenta la caché).
        let clip = tmp.path().join("live.mp4");
        std::fs::write(&clip, [0u8; 4]).expect("clip");
        let mut ws = editor_with_edit("live", clip);

        // 1. Extracción ajena (otra sesión): no toca la viva y su
        // directorio se borra en vez de acumularse en el temporal.
        let stale_dir = tmp.path().join("stale");
        std::fs::create_dir(&stale_dir).expect("stale dir");
        AppInner::on_ytdlp_frames_ready(&mut ws, frames_outcome("stale", &stale_dir, "f0.png"));
        assert!(!stale_dir.exists(), "el directorio huérfano se borra");
        assert!(
            edit_of(&ws).waiting_for_frames(),
            "la sesión viva sigue esperando"
        );

        // 2. La extracción vigente se consume: su directorio sobrevive
        // (entra en la caché de fotogramas, no se borra).
        let live_dir = tmp.path().join("live_frames");
        std::fs::create_dir(&live_dir).expect("live dir");
        AppInner::on_ytdlp_frames_ready(&mut ws, frames_outcome("live", &live_dir, "f0.png"));
        assert!(live_dir.is_dir(), "el resultado vigente se conserva");
        assert!(!edit_of(&ws).waiting_for_frames());

        // 3. Duplicado tardío de la misma extracción (ya servida): se ignora
        // y su directorio también se borra.
        let late_dir = tmp.path().join("late");
        std::fs::create_dir(&late_dir).expect("late dir");
        AppInner::on_ytdlp_frames_ready(&mut ws, frames_outcome("live", &late_dir, "f0.png"));
        assert!(!late_dir.exists(), "el duplicado tardío se borra");
    }

    #[test]
    fn late_failure_does_not_clobber_a_good_result() {
        let tmp = tempfile::tempdir().expect("temp");
        let clip = tmp.path().join("live.mp4");
        std::fs::write(&clip, [0u8; 4]).expect("clip");
        let mut ws = editor_with_edit("live", clip);

        // Fallo de otra extracción: se ignora (la viva sigue esperando, sin
        // error).
        AppInner::on_ytdlp_frames_failed(&mut ws, "stale".to_owned(), "boom".into());
        assert!(edit_of(&ws).waiting_for_frames());
        assert_eq!(edit_of(&ws).frames_error(), None);

        // Fallo de la extracción vigente: sí se registra.
        AppInner::on_ytdlp_frames_failed(&mut ws, "live".to_owned(), "boom".into());
        assert!(!edit_of(&ws).waiting_for_frames());
        assert_eq!(edit_of(&ws).frames_error(), Some("boom"));
    }

    #[test]
    fn failure_after_success_is_ignored() {
        let tmp = tempfile::tempdir().expect("temp");
        let clip = tmp.path().join("live.mp4");
        std::fs::write(&clip, [0u8; 4]).expect("clip");
        let mut ws = editor_with_edit("live", clip);
        let live_dir = tmp.path().join("live_frames");
        std::fs::create_dir(&live_dir).expect("live dir");
        AppInner::on_ytdlp_frames_ready(&mut ws, frames_outcome("live", &live_dir, "f0.png"));
        assert!(!edit_of(&ws).waiting_for_frames());
        // El fallo tardío de la extracción ya servida no pisa el resultado
        // bueno ni marca error.
        AppInner::on_ytdlp_frames_failed(&mut ws, "live".to_owned(), "tardío".into());
        assert_eq!(edit_of(&ws).frames_error(), None);
    }
}
