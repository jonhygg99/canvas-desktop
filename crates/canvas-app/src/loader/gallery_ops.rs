//! Operaciones de archivos pedidas desde la galería (crear, duplicar,
//! renombrar, borrar, restaurar) y el escaneo de una carpeta con sus
//! miniaturas.

use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

use eframe::egui;

use super::load_ops::probe_page_sizes;
use super::{AppMsg, GalleryOp};

/// Nombre de archivo sin su última extensión, y esa extensión (ambos vacíos
/// si `path` no los tiene).
fn split_name(path: &Path) -> (String, String) {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_default();
    (stem, ext)
}

/// Copia `src` (y su sidecar, si es una imagen que tiene uno) a una ruta
/// libre en `folder`. Con `force_copy_suffix`, el nombre siempre lleva
/// « copy» (duplicar en el sitio); si no, se intenta primero el mismo
/// nombre y solo se numera si colisiona (pegar en otra carpeta). Revierte
/// la primera copia si la del sidecar falla, para no dejar un duplicado a
/// medias.
fn duplicate_into(src: &Path, folder: &Path, force_copy_suffix: bool) -> Result<PathBuf, String> {
    let (stem, ext) = split_name(src);
    let base = if force_copy_suffix {
        format!("{stem} copy")
    } else {
        stem
    };
    let dst = canvas_io::reserve_unique_path(folder, &base, &ext).map_err(|e| e.to_string())?;
    if let Err(e) = std::fs::copy(src, &dst) {
        let _ = std::fs::remove_file(&dst);
        return Err(e.to_string());
    }
    if canvas_io::is_image_file(src) {
        if let Some(src_sidecar) = canvas_io::find_sidecar(src) {
            // El destino de sidecar cae en la carpeta oculta de `folder`
            // (que puede no ser la de `src` — copiar entre carpetas): hay
            // que asegurarla antes de copiar, no solo antes de reservar `dst`.
            if let Err(e) = canvas_io::ensure_sidecar_dir(folder) {
                let _ = std::fs::remove_file(&dst);
                return Err(e.to_string());
            }
            let dst_sidecar = canvas_io::sidecar_path(&dst);
            if let Err(e) = std::fs::copy(&src_sidecar, &dst_sidecar) {
                let _ = std::fs::remove_file(&dst);
                return Err(e.to_string());
            }
        }
    }
    Ok(dst)
}

/// Cambia el nombre base de `path` a `new_stem`, conservando su extensión
/// original. Si el destino ya existe se rechaza en vez de sobrescribir:
/// `std::fs::rename` en Windows usa `MOVEFILE_REPLACE_EXISTING`, así que sin
/// este chequeo previo un nombre repetido perdería el archivo que ya
/// hubiera ahí en silencio. Si `path` es una imagen con sidecar, lo
/// renombra también (mejor esfuerzo: un fallo ahí no deshace el renombrado
/// principal, solo se registra).
fn rename_with_sidecar(path: &Path, new_stem: &str) -> Result<PathBuf, String> {
    let folder = path.parent().map(PathBuf::from).unwrap_or_default();
    let (_, ext) = split_name(path);
    let new_name = if ext.is_empty() {
        new_stem.to_owned()
    } else {
        format!("{new_stem}.{ext}")
    };
    let dst = folder.join(&new_name);
    if dst.exists() {
        return Err(format!("\"{new_name}\" already exists"));
    }
    std::fs::rename(path, &dst).map_err(|e| e.to_string())?;
    if canvas_io::is_image_file(path) {
        if let Some(src_sidecar) = canvas_io::find_sidecar(path) {
            if let Err(e) = canvas_io::ensure_sidecar_dir(&folder) {
                tracing::warn!("no se pudo preparar la carpeta de sidecars: {e}");
            } else {
                let dst_sidecar = canvas_io::sidecar_path(&dst);
                if let Err(e) = std::fs::rename(&src_sidecar, &dst_sidecar) {
                    tracing::warn!("no se pudo renombrar el sidecar: {e}");
                }
            }
        }
    }
    Ok(dst)
}

/// Envía `path` (y su sidecar, si es una imagen que tiene uno) a la
/// Papelera de reciclaje del sistema: recuperable si el usuario se
/// equivoca, a diferencia de un borrado permanente. Usado por el borrado
/// desde la GALERÍA (`GalleryOp::Delete`) — ese no tiene deshacer, así que
/// sí le compensa depender de la papelera real del sistema.
fn trash_with_sidecar(path: &Path) -> Result<(), String> {
    trash::delete(path).map_err(|e| e.to_string())?;
    if canvas_io::is_image_file(path) {
        if let Some(sidecar) = canvas_io::find_sidecar(path) {
            if let Err(e) = trash::delete(&sidecar) {
                tracing::warn!("no se pudo borrar el sidecar: {e}");
            }
        }
    }
    Ok(())
}

/// Mueve `path` (y su sidecar, si es una imagen que tiene uno) a la
/// papelera PROPIA del proyecto (`canvas_io::move_to_local_trash`), no la
/// del sistema — este borrado sí tiene deshacer (botón «Delete» del
/// editor/cabecera de un lienzo), y restaurar con un `rename` no depende de
/// ninguna API de plataforma (a diferencia de `trash::os_limited`, que en
/// macOS no existe).
fn trash_locally_with_sidecar(path: &Path) -> Result<(), String> {
    canvas_io::move_to_local_trash(path).map_err(|e| e.to_string())?;
    if canvas_io::is_image_file(path) {
        if let Some(sidecar) = canvas_io::find_sidecar(path) {
            if let Err(e) = canvas_io::move_to_local_trash(&sidecar) {
                tracing::warn!("no se pudo mover el sidecar a la papelera: {e}");
            }
        }
    }
    Ok(())
}

/// Ejecuta una `GalleryOp` en un hilo de trabajo y avisa con
/// `AppMsg::GalleryOpDone`.
pub fn spawn_gallery_op(op: GalleryOp, open: bool, tx: Sender<AppMsg>, ctx: egui::Context) {
    std::thread::spawn(move || {
        let (folder, created, result): (PathBuf, Option<PathBuf>, Result<(), String>) = match op {
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
                    Ok(()) => (parent, Some(path), Ok(())),
                    Err(e) => (parent, None, Err(e.to_string())),
                }
            }
            GalleryOp::RenameFolder { path, new_name } => {
                let parent = path.parent().map(PathBuf::from).unwrap_or_default();
                let dst = parent.join(&new_name);
                match std::fs::rename(&path, &dst) {
                    Ok(()) => (parent, Some(dst), Ok(())),
                    Err(e) => (parent, None, Err(e.to_string())),
                }
            }
            GalleryOp::DeleteFolder { path } => {
                let parent = path.parent().map(PathBuf::from).unwrap_or_default();
                match trash::delete(&path) {
                    Ok(()) => (parent, None, Ok(())),
                    Err(e) => (parent, None, Err(e.to_string())),
                }
            }
        };
        let _ = tx.send(AppMsg::GalleryOpDone {
            folder,
            created,
            result,
            open,
        });
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

fn restore_one(original: &Path) -> Result<(), String> {
    let staged = canvas_io::local_trash_path(original);
    canvas_io::restore_from_local_trash(&staged, original).map_err(|e| e.to_string())
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
        // Solo el primer nivel, imágenes y diseños, sin archivos ocultos.
        // `is_standalone_design` deja fuera el sidecar de una imagen que ya
        // sale por sí sola en la cuadrícula. `p.is_file()` ya excluye la
        // carpeta `.canvas/` (es un directorio) — el chequeo por nombre es
        // cinturón y tirantes: no depende de que siga siendo un directorio ni
        // de que el usuario le haya quitado el atributo oculto.
        let mut files: Vec<(PathBuf, Option<std::time::SystemTime>)> = std::fs::read_dir(&folder)
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .map(|e| e.path())
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
                    .collect()
            })
            .unwrap_or_default();
        files.sort_by_key(|(p, _)| p.file_name().map(|n| n.to_ascii_lowercase()));

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
    let result = canvas_io::thumbnail(&path, 256, cache_dir).map_err(|e| e.to_string());
    let _ = tx.send(AppMsg::GalleryThumb {
        folder,
        path,
        result,
    });
    ctx.request_repaint();
}
