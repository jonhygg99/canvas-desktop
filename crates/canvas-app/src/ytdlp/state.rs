//! Estado del panel de descargas yt-dlp (pestaña «Download»).

use super::api;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct Panel {
    /// URLs pegadas (una por línea o separadas por espacios).
    pub urls: String,
    /// Trim por tiempos: inicio/fin en `HH:MM:SS`, `MM:SS` o segundos.
    pub start: String,
    pub end: String,
    pub download_segment: bool,
    /// Descargar sin audio (solo video).
    pub mute: bool,
    /// Descarga en curso en un hilo de trabajo.
    pub downloading: bool,
    /// Progreso textual (`[2/5] 42% …`) para la UI.
    pub progress: String,
    pub cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    pub failed: Vec<(String, String)>,
    pub retry_requests: std::collections::HashMap<String, crate::loader::YtdlpDownloadRequest>,
    pub(crate) clip_previews: super::clip_preview::ClipPreviews,
    pub clip_info: std::collections::HashMap<PathBuf, crate::loader::DownloadedClip>,
    /// Último error visible.
    pub error: Option<String>,
    /// Clips visibles, el más reciente primero (así se pintan). Dueño de
    /// TODA la carpeta staging: lo de sesiones anteriores sigue en disco
    /// hasta borrarse desde aquí y entra con `sync_download_folder`.
    pub done: Vec<PathBuf>,
    /// Escaneo de la carpeta pendiente: el listado arranca con lo que hay
    /// en el disco, no vacío, pero solo se lee UNA vez por sesión (IO en
    /// el hilo de UI no rueda cada frame).
    pub needs_folder_rescan: bool,
    /// Clip pendiente de insertar al canvas (lo pide el botón Insertar; lo
    /// lanza `layers_panel` con el canal a mano).
    pub pending_insert: Option<PathBuf>,
    /// Clip pendiente de editar: abre la ventana de edición.
    pub pending_edit: Option<PathBuf>,
    /// Sesión de edición abierta (`None` = ventana cerrada).
    pub edit: Option<super::edit::VideoEdit>,
    /// Fotogramas ya extraídos por clip: abrir «Edit video» sobre el mismo
    /// archivo no repite el pase de ffmpeg (ver `frames_cache`).
    pub(crate) frames_cache: super::frames_cache::FramesCache,
}

impl Default for Panel {
    fn default() -> Self {
        Self {
            urls: String::new(),
            start: String::new(),
            end: String::new(),
            download_segment: false,
            mute: true,
            downloading: false,
            progress: String::new(),
            cancel: None,
            failed: Vec::new(),
            retry_requests: std::collections::HashMap::new(),
            clip_previews: Default::default(),
            clip_info: std::collections::HashMap::new(),
            error: None,
            done: Vec::new(),
            needs_folder_rescan: true,
            pending_insert: None,
            pending_edit: None,
            edit: None,
            frames_cache: Default::default(),
        }
    }
}

impl Drop for Panel {
    fn drop(&mut self) {
        if let Some(cancel) = &self.cancel {
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }
}

impl Panel {
    /// Refresca la lista con TODA la carpeta de descargas: lo que sigue en
    /// disco de sesiones anteriores entra también, no solo lo bajado en
    /// esta sesión. El IO rueda una vez por sesión (`needs_folder_rescan`);
    /// los cambios posteriores los traen los mensajes de la tanda
    /// (`merge_done`) y del borrado.
    pub(crate) fn sync_download_folder(&mut self, folder: &Path) {
        if !self.needs_folder_rescan {
            return;
        }
        self.needs_folder_rescan = false;
        self.done = merge_disk_and_session(disk_downloads(folder), &self.done);
        self.clip_info.retain(|path, _| self.done.contains(path));
    }

    pub(crate) fn finish_download_item(&mut self, outcome: crate::loader::YtdlpItemOutcome) {
        self.failed.retain(|(url, _)| *url != outcome.url);
        if let Some(error) = outcome.error {
            if let Some(target) = outcome.target {
                if let Some(request) = self.retry_requests.get_mut(&outcome.url) {
                    request.target = Some(target);
                }
            }
            self.failed.push((outcome.url, error));
        } else {
            self.retry_requests.remove(&outcome.url);
        }
        for clip in outcome.clips {
            // Cada clip aparece al terminar, sin esperar al resto de la tanda,
            // y arriba del todo: lo más reciente primero.
            if !self.done.contains(&clip.path) {
                self.done.insert(0, clip.path.clone());
            }
            self.clip_info.insert(clip.path.clone(), clip);
        }
    }
}

/// Lo que hay en la carpeta de descargas: clips TERMINADOS (la tabla
/// compartida `api::is_clip_name`: `.mp4`/`.webm`/`.m4a`) que sigan en el
/// disco, el más nuevo primero (mtime). Pura salvo el IO de `read_dir`.
fn disk_downloads(folder: &Path) -> Vec<PathBuf> {
    let mut entries: Vec<(SystemTime, PathBuf)> = Vec::new();
    if let Ok(read) = std::fs::read_dir(folder) {
        for entry in read.flatten() {
            let path = entry.path();
            if !api::is_clip_name(&path) || !path.is_file() {
                continue;
            }
            let modified = entry
                .metadata()
                .and_then(|meta| meta.modified())
                .unwrap_or(UNIX_EPOCH);
            entries.push((modified, path));
        }
    }
    // El más nuevo primero: la lista se pinta de arriba hacia abajo.
    entries.sort_by_key(|(time, _)| std::cmp::Reverse(*time));
    entries.into_iter().map(|(_, path)| path).collect()
}

/// Une lo leído del disco con los paths de la sesión. El disco ya llega
/// solo con lo que existe (el más nuevo primero); los paths de sesión que
/// ya no están se caen — igual que `merge_done` poda en el hilo de
/// mensajes — y los que sigan sin estar (tanda en vuelo durante el
/// escaneo) entran al final.
fn merge_disk_and_session(disk: Vec<PathBuf>, session: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = disk;
    for path in session {
        // La condición NO va en un filter de la iteración: ese closure
        // mantiene prestada `out` y chocaría con el push. Así el borrow
        // termina en la línea y el push es libre.
        if path.is_file() && !out.contains(path) {
            out.push(path.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loader::{DownloadedClip, YtdlpDownloadRequest, YtdlpItemOutcome};

    #[test]
    fn disk_scan_lists_only_mp4s_newest_first() {
        let dir = tempfile::tempdir().expect("temp");
        let old = dir.path().join("old.mp4");
        let new = dir.path().join("new.mp4");
        std::fs::write(&old, [0u8; 4]).expect("v");
        std::fs::write(&new, [0u8; 4]).expect("v");
        // mtime determinista. Un handle de LECTURA no puede tocar mtime en
        // Windows: abiertos con acceso de escritura.
        let base = UNIX_EPOCH
            .checked_add(std::time::Duration::from_secs(1_000_000))
            .expect("epoch");
        let stamp = |path: &Path, time: SystemTime| {
            std::fs::OpenOptions::new()
                .write(true)
                .open(path)
                .and_then(|file| file.set_modified(time))
                .expect("mtime");
        };
        stamp(&old, base);
        stamp(
            &new,
            base.checked_add(std::time::Duration::from_secs(60))
                .expect("t"),
        );
        // Lo que NO es un clip descargado queda fuera del listado.
        std::fs::write(dir.path().join("resume.mp4.part"), [0u8; 2]).expect("part");
        std::fs::write(dir.path().join("thumb.png"), [0u8; 2]).expect("png");
        // Un «mute» sin audio que fusionar puede terminar en `.webm`: es
        // un clip terminado y entra (los parciales nunca).
        let mute = dir.path().join("mute.webm");
        std::fs::write(&mute, [0u8; 2]).expect("webm");
        stamp(
            &mute,
            base.checked_add(std::time::Duration::from_secs(120))
                .expect("t"),
        );
        assert_eq!(disk_downloads(dir.path()), vec![mute, new, old]);
    }

    #[test]
    fn sidebar_lists_whole_folder_session_paths_survive_missing_drop() {
        let dir = tempfile::tempdir().expect("temp");
        let older = dir.path().join("older.mp4");
        std::fs::write(&older, [0u8; 4]).expect("v");
        let mut panel = Panel::default();
        // Ruta de sesión de esta tanda y otra que ya no está: la viva se
        // re-encuentra por el escaneo (es la más nueva) y la muerta se cae,
        // igual que `merge_done` poda en el hilo de mensajes.
        let in_flight = dir.path().join("in-flight.mp4");
        std::fs::write(&in_flight, [0u8; 4]).expect("v");
        let stamp = |path: &Path, time: SystemTime| {
            std::fs::OpenOptions::new()
                .write(true)
                .open(path)
                .and_then(|file| file.set_modified(time))
                .expect("mtime");
        };
        let base = UNIX_EPOCH
            .checked_add(std::time::Duration::from_secs(1_000_000))
            .expect("epoch");
        stamp(&older, base);
        stamp(
            &in_flight,
            base.checked_add(std::time::Duration::from_secs(60))
                .expect("t"),
        );
        let gone = dir.path().join("gone.mp4");
        panel.done = vec![in_flight.clone(), gone];
        panel.sync_download_folder(dir.path());
        assert_eq!(panel.done, vec![in_flight.clone(), older.clone()]);
        // El escaneo rueda UNA vez por sesión: borrar el archivo fuera de la
        // app no se re-lee aquí (los mensajes del borrado podan la lista).
        std::fs::remove_file(&older).expect("rm");
        panel.sync_download_folder(dir.path());
        assert_eq!(panel.done, vec![in_flight, older]);
    }
    #[test]
    fn clips_are_visible_before_the_batch_finishes_and_cancel_preserves_retry_target() {
        let mut panel = Panel::default();
        let url = "https://example.test/video".to_owned();
        let target = PathBuf::from("clip.mp4");
        panel.retry_requests.insert(
            url.clone(),
            YtdlpDownloadRequest {
                urls: vec![url.clone()],
                start: Some(2.125),
                end: Some(4.0),
                mute: true,
                dest: PathBuf::new(),
                cancel: Default::default(),
                target: Some(target.clone()),
            },
        );
        panel.finish_download_item(YtdlpItemOutcome {
            url: url.clone(),
            target: None,
            clips: vec![],
            error: Some("Cancelled".into()),
        });
        let retry = &panel.retry_requests[&url];
        assert_eq!(retry.target, Some(target.clone()));
        assert_eq!(retry.start, Some(2.125));
        assert!(retry.mute);
        panel.finish_download_item(YtdlpItemOutcome {
            url: url.clone(),
            target: Some(target.clone()),
            clips: vec![DownloadedClip {
                path: target.clone(),
                title: "Original title".into(),
            }],
            error: None,
        });
        assert_eq!(panel.done, vec![target.clone()]);
        assert_eq!(panel.clip_info[&target].title, "Original title");
        assert!(panel.failed.is_empty());
        assert!(!panel.retry_requests.contains_key(&url));
    }

    #[test]
    fn newest_clip_lands_on_top() {
        let mut panel = Panel::default();
        for name in ["one.mp4", "two.mp4"] {
            panel.finish_download_item(YtdlpItemOutcome {
                url: format!("https://example.test/{name}"),
                target: None,
                clips: vec![DownloadedClip {
                    path: PathBuf::from(name),
                    title: name.to_owned(),
                }],
                error: None,
            });
        }
        assert_eq!(
            panel.done,
            vec![PathBuf::from("two.mp4"), PathBuf::from("one.mp4")]
        );
        // Repetir el mismo clip no lo duplica ni lo mueve.
        panel.finish_download_item(YtdlpItemOutcome {
            url: "https://example.test/one.mp4".to_owned(),
            target: None,
            clips: vec![DownloadedClip {
                path: PathBuf::from("one.mp4"),
                title: "one.mp4".to_owned(),
            }],
            error: None,
        });
        assert_eq!(
            panel.done,
            vec![PathBuf::from("two.mp4"), PathBuf::from("one.mp4")]
        );
    }
}
