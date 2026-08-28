//! Operaciones de archivos pedidas desde la galería (crear, duplicar,
//! renombrar, borrar, restaurar) y el escaneo de una carpeta con sus
//! miniaturas.

use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

use canvas_io::IoError;
use eframe::egui;

use super::file_ops::{
    duplicate_into, rename_with_sidecar, restore_one, trash_locally_with_sidecar,
    trash_with_sidecar,
};

use super::load_ops::probe_page_sizes;
use super::{AppMsg, GalleryOp};

/// Ejecuta una `GalleryOp` en un hilo de trabajo y avisa con
/// `AppMsg::GalleryOpDone`.
pub fn spawn_gallery_op(op: GalleryOp, open: bool, tx: Sender<AppMsg>, ctx: egui::Context) {
    std::thread::spawn(move || {
        let (folder, created, result): (PathBuf, Option<PathBuf>, Result<(), IoError>) = match op {
            GalleryOp::Duplicate { path } => {
                let folder = path.parent().map(PathBuf::from).unwrap_or_default();
                match duplicate_into(&path, &folder, true) {
                    Ok(dst) => (folder, Some(dst), Ok(())),
                    Err(e) => (folder, None, Err(e)),
                }
            }
            GalleryOp::CopyInto { src, folder } => {
                let same_folder = src.parent() == Some(folder.as_path());
                match duplicate_into(&src, &folder, same_folder) {
                    Ok(dst) => (folder, Some(dst), Ok(())),
                    Err(e) => (folder, None, Err(e)),
                }
            }
            GalleryOp::Rename { path, new_stem } => {
                let folder = path.parent().map(PathBuf::from).unwrap_or_default();
                match rename_with_sidecar(&path, &new_stem) {
                    Ok(dst) => (folder, Some(dst), Ok(())),
                    Err(e) => (folder, None, Err(e)),
                }
            }
            GalleryOp::Delete { path } => {
                let folder = path.parent().map(PathBuf::from).unwrap_or_default();
                match trash_with_sidecar(&path) {
                    Ok(()) => (folder, None, Ok(())),
                    Err(e) => (folder, None, Err(e)),
                }
            }
            GalleryOp::CreateFolder { parent, name } => {
                let path = parent.join(&name);
                match std::fs::create_dir(&path) {
                    Ok(()) => (parent, Some(path.clone()), Ok(())),
                    Err(source) => (parent, None, Err(canvas_io::IoError::Io { path, source })),
                }
            }
            GalleryOp::RenameFolder { path, new_name } => {
                let parent = path.parent().map(PathBuf::from).unwrap_or_default();
                let dst = parent.join(&new_name);
                match std::fs::rename(&path, &dst) {
                    Ok(()) => (parent, Some(dst), Ok(())),
                    Err(source) => (parent, None, Err(canvas_io::IoError::Io { path, source })),
                }
            }
            GalleryOp::DeleteFolder { path } => {
                let parent = path.parent().map(PathBuf::from).unwrap_or_default();
                match trash::delete(&path) {
                    Ok(()) => (parent, None, Ok(())),
                    Err(e) => (
                        parent,
                        None,
                        Err(canvas_io::IoError::Message {
                            message: e.to_string(),
                        }),
                    ),
                }
            }
        };
        let _ = tx.send(AppMsg::GalleryOpDone(super::GalleryOpOutcome {
            folder,
            created,
            result,
            open,
        }));
        ctx.request_repaint();
    });
}

/// Renombra el archivo abierto en el editor (y su sidecar, si lo tiene) —
/// disparado desde el lápiz junto al nombre en el panel de propiedades.
pub fn spawn_document_rename(
    path: PathBuf,
    new_stem: String,
    tx: Sender<AppMsg>,
    ctx: egui::Context,
) {
    std::thread::spawn(move || {
        let result = rename_with_sidecar(&path, &new_stem);
        let _ = tx.send(AppMsg::DocumentRenamed {
            old_path: path,
            result,
        });
        ctx.request_repaint();
    });
}

/// Mueve a la papelera del proyecto el archivo abierto en el editor (y su
/// sidecar) — disparado desde el botón «Delete» del panel de propiedades, o
/// desde la cabecera de un lienzo de fondo.
pub fn spawn_document_delete(path: PathBuf, tx: Sender<AppMsg>, ctx: egui::Context) {
    std::thread::spawn(move || {
        let result = trash_locally_with_sidecar(&path);
        let _ = tx.send(AppMsg::DocumentDeleted { path, result });
        ctx.request_repaint();
    });
}

/// Restaura de la papelera del proyecto `path` (y `sidecar`, si lo tenía) a
/// su ubicación original — deshacer un `GlobalStep::Delete`. El sidecar es
/// mejor esfuerzo (solo se avisa por log si falla, no aborta el resto).
pub fn spawn_restore_from_trash(
    path: PathBuf,
    sidecar: Option<PathBuf>,
    tx: Sender<AppMsg>,
    ctx: egui::Context,
) {
    std::thread::spawn(move || {
        let result = restore_one(&path);
        if let Some(sidecar) = &sidecar {
            if let Err(e) = restore_one(sidecar) {
                tracing::warn!("no se pudo restaurar el sidecar: {e}");
            }
        }
        let _ = tx.send(AppMsg::DocumentRestored { path, result });
        ctx.request_repaint();
    });
}

/// Descubre los archivos que puede mostrar una galería. Mantiene el
/// descubrimiento separado del hilo/UI para que pueda probarse y para no
/// convertir un error de permisos o una ruta inválida en una galería vacía.
fn discover_gallery_files(
    folder: &std::path::Path,
) -> Result<Vec<(PathBuf, Option<std::time::SystemTime>)>, String> {
    // Reintentos cortos: los montajes de nube (`~/Library/CloudStorage`)
    // fallan con EPERM/EIO TRANSITORIOS mientras el proveedor hidrata
    // contenido solo-en-linea, y sin reintentos eso se ve como galeria vacia.
    let entries = canvas_io::read_dir_resilient(folder)
        .map_err(|e| canvas_io::describe_read_dir_error(folder, &e))?;
    let mut files = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|p| {
            p.file_name() != Some(std::ffi::OsStr::new(canvas_io::SIDECAR_DIR))
                && p.is_file()
                && (canvas_io::is_image_file(p) || canvas_io::is_standalone_design(p))
                && !canvas_shell::is_hidden(p)
        })
        .map(|p| {
            let mtime = std::fs::metadata(&p).and_then(|m| m.modified()).ok();
            (p, mtime)
        })
        .collect::<Vec<_>>();
    files.sort_by_key(|(p, _)| p.file_name().map(|n| n.to_ascii_lowercase()));
    Ok(files)
}

/// Lista las imágenes de una carpeta y genera sus miniaturas en paralelo
/// (rayon), entregándolas por el canal según van saliendo: la cuadrícula se
/// va rellenando sin bloquear nunca la UI.
pub fn spawn_gallery_scan(
    folder: PathBuf,
    cache_dir: Option<PathBuf>,
    tx: Sender<AppMsg>,
    ctx: egui::Context,
) {
    std::thread::spawn(move || {
        let files = match discover_gallery_files(&folder) {
            Ok(files) => files,
            Err(error) => {
                tracing::warn!(folder = %folder.display(), error = %error, "gallery scan failed");
                let _ = tx.send(AppMsg::GalleryScanFailed { folder, error });
                ctx.request_repaint();
                return;
            }
        };
        tracing::debug!(folder = %folder.display(), files = files.len(), "gallery scan complete");

        let _ = tx.send(AppMsg::GalleryScanned {
            folder: folder.clone(),
            files: files.clone(),
        });
        ctx.request_repaint();

        use rayon::prelude::*;

        // Sondeo de tamaños (cabecera, sin decodificar): antes que las
        // miniaturas porque es mucho más barato y la baraja del editor lo
        // necesita para apilar sus lienzos. Un solo mensaje con todos los
        // tamaños, no uno por archivo, para que haga falta un único
        // `relayout`. Este sondeo llega DESDE LA GALERÍA, antes de que la
        // `Deck` de esta carpeta exista todavía (se construye después, al
        // terminar de cargar la imagen en la que se hizo clic) — el guardia
        // de carpeta del manejador de `DeckProbed` en `main.rs` lo descarta
        // en ese caso. No pasa nada: `spawn_deck_probe`, más abajo, repite
        // el sondeo una vez que la baraja ya existe con la carpeta puesta.
        let sizes = probe_page_sizes(&folder, files.iter().map(|(p, _)| p.clone()).collect());
        let _ = tx.send(AppMsg::DeckProbed {
            folder: folder.clone(),
            generation: 0,
            sizes,
        });
        ctx.request_repaint();

        files.par_iter().for_each_with(tx, |tx, (path, _mtime)| {
            send_thumb(tx, &ctx, folder.clone(), path.clone(), cache_dir.as_deref());
        });
    });
}

/// Genera la miniatura de UN solo archivo y la manda por el mismo mensaje
/// que usa el escaneo completo (`GalleryThumb`) — su manejador en `main.rs`
/// ya sabe repartirla a `Deck::set_thumb`/`GalleryState::set_thumb` según
/// quién la quiera. Se usa para refrescar la miniatura de un diseño recién
/// guardado (nuevo o editado) sin tener que resondear la carpeta entera: sin
/// esto, un lienzo añadido durante la sesión (tira "+", `Ctrl+N`, duplicar)
/// se queda con su miniatura en blanco hasta que el usuario vuelve a abrir
/// la carpeta, porque nada más dispara `spawn_gallery_scan`.
pub fn spawn_single_thumb(
    folder: PathBuf,
    path: PathBuf,
    cache_dir: Option<PathBuf>,
    tx: Sender<AppMsg>,
    ctx: egui::Context,
) {
    std::thread::spawn(move || {
        send_thumb(&tx, &ctx, folder, path, cache_dir.as_deref());
    });
}

fn send_thumb(
    tx: &Sender<AppMsg>,
    ctx: &egui::Context,
    folder: PathBuf,
    path: PathBuf,
    cache_dir: Option<&Path>,
) {
    let result = canvas_io::thumbnail(&path, 256, cache_dir);
    let _ = tx.send(AppMsg::GalleryThumb {
        folder,
        path,
        result,
    });
    ctx.request_repaint();
}

/// Intentos del bucle automatico de listado de subcarpetas.
pub(crate) const FOLDER_REFRESH_ATTEMPTS: usize = 4;

/// Espera antes del reintento `attempt_index` (0-based): 1 s, 2 s, 4 s, 4 s…
/// El ciclo entero cubre ~11 s, muy por encima del hidratado tipico.
fn folder_refresh_delay(attempt_index: usize) -> std::time::Duration {
    let shift = attempt_index.min(2);
    std::time::Duration::from_millis(1000 << shift)
}

/// Reintenta en segundo plano el listado de subcarpetas de un montaje de
/// nube mientras falle: hasta `FOLDER_REFRESH_ATTEMPTS` intentos con la
/// espera anterior entre cada uno. Cada resultado viaja por
/// `AppMsg::FoldersRefreshed`; disparar solo cuando hay error persistente y
/// sin bucle en marcha (`GalleryState::take_folder_auto_refresh`).
pub fn spawn_folders_auto_refresh(folder: PathBuf, tx: Sender<AppMsg>, ctx: egui::Context) {
    std::thread::spawn(move || {
        for attempt in 0..FOLDER_REFRESH_ATTEMPTS {
            std::thread::sleep(folder_refresh_delay(attempt));
            let (children, error) = match crate::gallery::read_child_folders(&folder) {
                Ok(children) => (children, None),
                Err(error) => (Vec::new(), Some(error)),
            };
            let done = error.is_none();
            let _ = tx.send(AppMsg::FoldersRefreshed {
                folder: folder.clone(),
                children,
                error,
            });
            ctx.request_repaint();
            if done {
                return;
            }
        }
    });
}

#[cfg(test)]
#[path = "gallery_ops_tests.rs"]
mod tests;
