//! Estado del panel de descargas yt-dlp (pestaña «Download»).

use std::path::PathBuf;

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
    /// Rutas descargadas en la última tanda (para mostrarlas).
    pub done: Vec<PathBuf>,
    /// Clip pendiente de insertar al canvas (lo pide el botón Insertar; lo
    /// lanza `layers_panel` con el canal a mano).
    pub pending_insert: Option<PathBuf>,
    /// Clip pendiente de editar: abre la ventana de edición.
    pub pending_edit: Option<PathBuf>,
    /// Sesión de edición abierta (`None` = ventana cerrada).
    pub edit: Option<super::edit::VideoEdit>,
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
            pending_insert: None,
            pending_edit: None,
            edit: None,
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
            // Cada clip aparece al terminar, sin esperar al resto de la tanda.
            if !self.done.contains(&clip.path) {
                self.done.push(clip.path.clone());
            }
            self.clip_info.insert(clip.path.clone(), clip);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loader::{DownloadedClip, YtdlpDownloadRequest, YtdlpItemOutcome};
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
}
