//! Vigilancia del archivo abierto con `notify`: si cambia en disco mientras
//! está en el editor, la app avisa y ofrece recargar. Los eventos que provoca
//! nuestro propio guardado se descartan con una ventana de gracia que
//! gestiona `App` (además el watcher se recrea tras cada guardado, porque la
//! sustitución atómica puede invalidar el watch).

use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

use eframe::egui;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};

use crate::loader::AppMsg;

pub struct DocWatcher {
    /// Mantener vivo el watcher; al soltarlo deja de vigilar.
    _watcher: RecommendedWatcher,
    pub path: PathBuf,
}

/// Vigila `path` y envía `SourceChangedOnDisk` en cada modificación. Mejor
/// esfuerzo: si el watcher no se puede crear, la app funciona sin aviso.
pub fn watch(path: &Path, tx: Sender<AppMsg>, ctx: egui::Context) -> Option<DocWatcher> {
    let reported = path.to_owned();
    let mut watcher =
        notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
            let Ok(event) = res else { return };
            if matches!(
                event.kind,
                notify::EventKind::Modify(_)
                    | notify::EventKind::Create(_)
                    | notify::EventKind::Remove(_)
            ) {
                let _ = tx.send(AppMsg::SourceChangedOnDisk {
                    path: reported.clone(),
                });
                ctx.request_repaint();
            }
        })
        .map_err(|e| tracing::warn!("no se pudo crear el watcher de {}: {e}", path.display()))
        .ok()?;

    watcher
        .watch(path, RecursiveMode::NonRecursive)
        .map_err(|e| tracing::warn!("no se pudo vigilar {}: {e}", path.display()))
        .ok()?;

    Some(DocWatcher {
        _watcher: watcher,
        path: path.to_owned(),
    })
}
