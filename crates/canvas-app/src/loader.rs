//! Trabajo en hilos aparte: carga de imágenes y diálogos nativos. La UI nunca
//! bloquea en disco; los resultados llegan por canal.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::Sender;
use std::time::{SystemTime, UNIX_EPOCH};

use canvas_core::LayerId;
use canvas_io::{ImageMetadata, LoadedImage, RestoredDocument};
use eframe::egui;

/// Resultado de abrir una imagen: mapa de bits plano, o documento con capas
/// restaurado desde su sidecar `.canvas`. `Design` es un `.canvas` autónomo:
/// el archivo abierto ES el documento, no la imagen que lo acompaña.
pub enum LoadOutcome {
    Flat(LoadedImage),
    Restored(RestoredDocument),
    Design(RestoredDocument),
}

pub enum AppMsg {
    FilePicked(Option<PathBuf>),
    FolderPicked(Option<PathBuf>),
    ImageLoaded {
        path: PathBuf,
        result: Result<LoadOutcome, String>,
        /// ICC/EXIF del archivo original, para preservarlos al guardar.
        metadata: ImageMetadata,
    },
    /// Imagen cargada para AÑADIRSE como capa al documento abierto.
    ImageLoadedForLayer {
        path: PathBuf,
        result: Result<LoadedImage, String>,
    },
    /// Imagen cargada para REEMPLAZAR una capa de imagen concreta.
    ImageLoadedForReplace {
        layer: LayerId,
        label: String,
        source_path: Option<PathBuf>,
        result: Result<LoadedImage, String>,
    },
    SaveAsPicked(Option<PathBuf>),
    Saved {
        path: PathBuf,
        result: Result<(), String>,
        /// true si venía de «Guardar como…» y el documento debe apuntar aquí.
        new_source: bool,
    },
    /// Ruta elegida para exportar (o `None` si se canceló el diálogo).
    ExportPathPicked(Option<PathBuf>),
    Exported {
        path: PathBuf,
        result: Result<(), String>,
    },
    GalleryScanned {
        folder: PathBuf,
        /// (ruta, fecha de modificación si se pudo leer)
        files: Vec<(PathBuf, Option<std::time::SystemTime>)>,
    },
    GalleryThumb {
        folder: PathBuf,
        path: PathBuf,
        result: Result<LoadedImage, String>,
    },
    /// Tamaños de página sondeados de toda una carpeta, en UN solo mensaje
    /// (la sonda es de cabecera: la carpeta entera son decenas de ms) para
    /// que la baraja del editor haga un único `relayout`, no uno por
    /// archivo. `None` por archivo si `probe_page_size` falló para ese uno.
    DeckProbed {
        folder: PathBuf,
        sizes: Vec<(PathBuf, Option<(f64, f64)>)>,
    },
    /// Un lienzo de la baraja terminó de cargar en segundo plano (scroll,
    /// tira, `PageUp`/`PageDown`). Deliberadamente NO es `ImageLoaded`: esa
    /// rama puede abrir un `rfd::MessageDialog` modal ("Image changed
    /// outside Canvas Desktop"), inaceptable para una carga disparada solo
    /// por hacer scroll — aquí un hash que no coincide se guarda como
    /// `external_change` en silencio y se muestra como el banner normal en
    /// cuanto la ranura se activa.
    SlotLoaded {
        folder: PathBuf,
        path: PathBuf,
        result: Result<LoadOutcome, String>,
        metadata: ImageMetadata,
    },
    /// Nombre reservado en disco para una ranura PROVISIONAL de la baraja
    /// que el usuario acaba de empezar a editar. Solo reserva: el archivo se
    /// escribe después, por el camino de guardado de siempre
    /// (`start_save_design`), que es el único que tiene la GPU para hornear
    /// la miniatura embebida. `slot` es el id ESTABLE, no el índice — entre
    /// la petición y la respuesta la baraja puede haberse reordenado (mismo
    /// criterio que `save_all_queue`).
    CanvasPathReserved {
        folder: PathBuf,
        slot: u64,
        result: Result<PathBuf, String>,
    },
    /// Ruta llegada desde una segunda instancia (por el socket local).
    OpenPathExternal(PathBuf),
    /// Una segunda instancia sin rutas pide traer la ventana al frente.
    FocusWindow,
    /// El archivo abierto cambió en disco (watcher `notify`).
    SourceChangedOnDisk {
        path: PathBuf,
    },
    /// Resultado del registro/desregistro de la integración con el shell.
    ShellIntegrationDone(Result<String, String>),
    /// Una operación de archivos de la galería (crear, duplicar, pegar)
    /// terminó. `created` es la ruta resultante, para abrirla o para que la
    /// galería la resalte al rescanear; ausente si la operación falló.
    GalleryOpDone {
        folder: PathBuf,
        created: Option<PathBuf>,
        result: Result<(), String>,
        /// Si venía de «✚ New design», abre el archivo recién creado.
        open: bool,
    },
    /// El archivo abierto en el editor se renombró (botón «✏» junto al
    /// nombre en el panel). No reutiliza `Saved`: renombrar no debe marcar
    /// el documento como recién guardado.
    DocumentRenamed {
        old_path: PathBuf,
        result: Result<PathBuf, String>,
    },
    /// El archivo abierto en el editor se envió a la Papelera (botón
    /// «Delete» del panel).
    DocumentDeleted {
        path: PathBuf,
        result: Result<(), String>,
    },
    /// Se restauró `path` desde la Papelera de reciclaje al deshacer un
    /// `GlobalStep::Delete` (`editor::EditorState::pending_restore`).
    DocumentRestored {
        path: PathBuf,
        result: Result<(), String>,
    },
}

/// Operación de archivos pedida desde la galería. Siempre en un hilo aparte:
/// copiar un PNG grande no puede bloquear la UI.
pub enum GalleryOp {
    /// Crea un lienzo en blanco en `folder` con un nombre libre. `ext` es la
    /// extensión elegida en Ajustes (`settings.new_canvas_format`): un
    /// raster real (con su sidecar) salvo que sea `canvas`, que sigue siendo
    /// un diseño autónomo. `jpeg_quality` solo importa si `ext == "jpg"`.
    NewDesign {
        folder: PathBuf,
        page: (f64, f64),
        ext: String,
        jpeg_quality: u8,
    },
    /// Duplica `path` (y su sidecar, si es una imagen que tiene uno) dentro
    /// de la misma carpeta, con sufijo « copy».
    Duplicate { path: PathBuf },
    /// Copia `src` (y su sidecar, si lo tiene) dentro de `folder`. Mismo
    /// nombre si no colisiona; si `src` ya está en `folder`, se comporta
    /// como `Duplicate` (sufijo « copy»).
    CopyInto { src: PathBuf, folder: PathBuf },
    /// Cambia solo el nombre base (stem); la extensión no se toca —
    /// cambiarla rompería `is_image_file`/`is_canvas_file` y la detección
    /// de sidecar.
    Rename { path: PathBuf, new_stem: String },
    /// A la Papelera de reciclaje (crate `trash`), no borrado permanente.
    Delete { path: PathBuf },
}

/// Nombre de archivo sin su última extensión, y esa extensión (ambos vacíos
/// si `path` no los tiene).
fn split_name(path: &std::path::Path) -> (String, String) {
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
fn duplicate_into(
    src: &std::path::Path,
    folder: &std::path::Path,
    force_copy_suffix: bool,
) -> Result<PathBuf, String> {
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
fn rename_with_sidecar(path: &std::path::Path, new_stem: &str) -> Result<PathBuf, String> {
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
fn trash_with_sidecar(path: &std::path::Path) -> Result<(), String> {
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
fn trash_locally_with_sidecar(path: &std::path::Path) -> Result<(), String> {
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
            GalleryOp::NewDesign {
                folder,
                page,
                ext,
                jpeg_quality,
            } => {
                let outcome = canvas_io::reserve_numbered_path(&folder, &ext)
                    .map_err(|e| e.to_string())
                    .and_then(|path| {
                        canvas_io::write_blank_canvas(&path, page.0, page.1, jpeg_quality)
                            .map(|()| path)
                            .map_err(|e| e.to_string())
                    });
                match outcome {
                    Ok(path) => (folder, Some(path), Ok(())),
                    Err(e) => (folder, None, Err(e)),
                }
            }
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

fn restore_one(original: &std::path::Path) -> Result<(), String> {
    let staged = canvas_io::local_trash_path(original);
    canvas_io::restore_from_local_trash(&staged, original).map_err(|e| e.to_string())
}

/// Carga una imagen. Con `use_sidecar`, intenta primero restaurar las capas
/// editables desde su `.canvas`; un sidecar ilegible degrada a carga plana.
pub fn spawn_load_image(path: PathBuf, use_sidecar: bool, tx: Sender<AppMsg>, ctx: egui::Context) {
    std::thread::spawn(move || {
        let result = (|| {
            if use_sidecar {
                match canvas_io::read_sidecar(&path) {
                    Ok(Some(restored)) => return Ok(LoadOutcome::Restored(restored)),
                    Ok(None) => {}
                    Err(e) => {
                        tracing::warn!("sidecar ilegible ({e}); abriendo la imagen aplanada")
                    }
                }
            }
            canvas_io::load_image(&path)
                .map(LoadOutcome::Flat)
                .map_err(|e| e.to_string())
        })();
        // ICC/EXIF del archivo en disco (mejor esfuerzo), venga el documento
        // aplanado o restaurado del sidecar: el original es el mismo.
        let metadata = canvas_io::extract_metadata_from_file(&path);
        let _ = tx.send(AppMsg::ImageLoaded {
            path,
            result,
            metadata,
        });
        ctx.request_repaint();
    });
}

/// Carga un diseño autónomo. Reutiliza `AppMsg::ImageLoaded` (y con él la
/// puerta de `View::Loading{path}` que descarta cargas obsoletas): un diseño
/// no tiene ICC/EXIF que preservar, así que la metadata va vacía.
pub fn spawn_load_design(path: PathBuf, tx: Sender<AppMsg>, ctx: egui::Context) {
    std::thread::spawn(move || {
        let result = canvas_io::read_design(&path)
            .map(LoadOutcome::Design)
            .map_err(|e| e.to_string());
        let _ = tx.send(AppMsg::ImageLoaded {
            path,
            result,
            metadata: ImageMetadata::default(),
        });
        ctx.request_repaint();
    });
}

/// Carga un lienzo de la baraja del editor en segundo plano (no la primera
/// apertura: eso sigue siendo `spawn_load_image`/`spawn_load_design` vía
/// `ImageLoaded`). Misma bifurcación que `App::open_path`: un `.canvas` ES
/// el documento; una imagen restaura su sidecar si lo tiene.
pub fn spawn_load_slot(folder: PathBuf, path: PathBuf, tx: Sender<AppMsg>, ctx: egui::Context) {
    std::thread::spawn(move || {
        let is_canvas = canvas_io::is_canvas_file(&path);
        let result = if is_canvas {
            canvas_io::read_design(&path)
                .map(LoadOutcome::Design)
                .map_err(|e| e.to_string())
        } else {
            match canvas_io::read_sidecar(&path) {
                Ok(Some(restored)) => Ok(LoadOutcome::Restored(restored)),
                Ok(None) => canvas_io::load_image(&path)
                    .map(LoadOutcome::Flat)
                    .map_err(|e| e.to_string()),
                Err(e) => {
                    tracing::warn!("sidecar ilegible ({e}); abriendo la imagen aplanada");
                    canvas_io::load_image(&path)
                        .map(LoadOutcome::Flat)
                        .map_err(|e| e.to_string())
                }
            }
        };
        let metadata = if is_canvas {
            ImageMetadata::default()
        } else {
            canvas_io::extract_metadata_from_file(&path)
        };
        let _ = tx.send(AppMsg::SlotLoaded {
            folder,
            path,
            result,
            metadata,
        });
        ctx.request_repaint();
    });
}

/// Sondeo de tamaños de página en paralelo (rayon), solo cabecera: una
/// carpeta entera son decenas de ms, sin decodificar un solo píxel. Factor
/// común entre `spawn_gallery_scan` (sondea al escanear una carpeta) y
/// `spawn_deck_probe` (sondea las ranuras de una baraja ya construida).
///
/// Todos los sidecar de `folder` viven en una única carpeta (`.canvas/`), así
/// que se listan UNA vez aquí en vez de que cada tarea del `par_iter` haga su
/// propio `is_file()`: un `read_dir` para la carpeta entera en vez de uno por
/// archivo. A diferencia de `canvas_io::probe_page_size` (que sí cae al
/// hermano legacy vía `find_sidecar`), esta versión en lote solo mira
/// `.canvas/`: una carpeta migrada solo a medias puede sondear con el tamaño
/// del raster en vez del de un sidecar legacy que aún no se ha guardado con
/// esta versión — inocuo, es solo la disposición de la baraja hasta que se
/// guarde una vez; abrir el archivo sigue restaurando sus capas igual
/// (`find_sidecar` sí mira el legacy).
fn probe_page_sizes(
    folder: &std::path::Path,
    paths: Vec<PathBuf>,
) -> Vec<(PathBuf, Option<(f64, f64)>)> {
    use rayon::prelude::*;
    let sidecar_dir = canvas_io::sidecar_dir(folder);
    let sidecars: std::collections::HashSet<std::ffi::OsString> = std::fs::read_dir(&sidecar_dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .map(|e| e.file_name())
                .collect()
        })
        .unwrap_or_default();
    paths
        .into_par_iter()
        .map(|path| {
            let sidecar = path
                .file_name()
                .map(|n| {
                    let mut n = n.to_owned();
                    n.push(".");
                    n.push(canvas_io::CANVAS_EXTENSION);
                    n
                })
                .filter(|name| sidecars.contains(name))
                .map(|name| sidecar_dir.join(name));
            let size = canvas_io::probe_page_size_with(&path, sidecar.as_deref()).ok();
            (path, size)
        })
        .collect()
}

/// Sondea los tamaños de página de las ranuras de una baraja YA construida y
/// los entrega en un solo mensaje `DeckProbed`. Existe además del sondeo de
/// `spawn_gallery_scan` porque aquel se lanza desde la GALERÍA, cuando
/// todavía no hay baraja para esa carpeta: el guardia de carpeta del
/// manejador de `DeckProbed` tira ese lote entero, y las ranuras se quedan
/// con el tamaño ESTIMADO de `Slot::size()` — que no coincide con los
/// píxeles que de verdad se pintan, y se come el hueco entre lienzos hasta
/// que cada uno se activa por turno. Este sondeo se lanza DESPUÉS de
/// construir la baraja (`App::resolve_deck`), así que su `folder` ya
/// coincide y el guardia lo deja pasar.
pub fn spawn_deck_probe(
    folder: PathBuf,
    paths: Vec<PathBuf>,
    tx: Sender<AppMsg>,
    ctx: egui::Context,
) {
    std::thread::spawn(move || {
        let sizes = probe_page_sizes(&folder, paths);
        let _ = tx.send(AppMsg::DeckProbed { folder, sizes });
        ctx.request_repaint();
    });
}

/// Reserva atómicamente (`create_new`, vía `canvas_io::reserve_numbered_path`)
/// un nombre libre en `folder` para una ranura provisional que el usuario
/// acaba de empezar a editar. En un hilo por disciplina: en una unidad de
/// red con cientos de lienzos numerados ya creados, el bucle de reserva sí
/// se nota, y la UI no espera nunca al disco.
pub fn spawn_reserve_canvas_path(
    folder: PathBuf,
    slot: u64,
    ext: String,
    tx: Sender<AppMsg>,
    ctx: egui::Context,
) {
    std::thread::spawn(move || {
        let result = canvas_io::reserve_numbered_path(&folder, &ext).map_err(|e| e.to_string());
        let _ = tx.send(AppMsg::CanvasPathReserved {
            folder,
            slot,
            result,
        });
        ctx.request_repaint();
    });
}

/// Reescribe (atómico) un diseño autónomo en `path`, sin rasterizar nada.
pub fn spawn_save_design(
    path: PathBuf,
    payload: canvas_io::CanvasPayload,
    new_source: bool,
    tx: Sender<AppMsg>,
    ctx: egui::Context,
) {
    std::thread::spawn(move || {
        let result = canvas_io::write_design(&path, &payload).map_err(|e| e.to_string());
        let _ = tx.send(AppMsg::Saved {
            path,
            result,
            new_source,
        });
        ctx.request_repaint();
    });
}

/// Diálogo «Guardar como…» para un diseño: un único filtro `.canvas`, a
/// diferencia de `spawn_pick_save_path` que ofrece los cinco formatos
/// rasterizables de Guardar.
pub fn spawn_pick_design_path(suggested: Option<String>, tx: Sender<AppMsg>, ctx: egui::Context) {
    let suffix = format!(".{}", canvas_io::CANVAS_EXTENSION);
    let suggested = suggested.map(|name| {
        if name.to_ascii_lowercase().ends_with(&suffix) {
            name
        } else {
            format!("{name}{suffix}")
        }
    });
    std::thread::spawn(move || {
        let mut dialog = rfd::FileDialog::new()
            .set_title("Save design as…")
            .add_filter("Canvas design", &[canvas_io::CANVAS_EXTENSION]);
        if let Some(name) = suggested {
            dialog = dialog.set_file_name(name);
        }
        let picked = dialog.save_file();
        let _ = tx.send(AppMsg::SaveAsPicked(picked));
        ctx.request_repaint();
    });
}

/// Como `spawn_load_image`, pero el resultado se añade como capa nueva al
/// documento abierto en vez de sustituirlo.
pub fn spawn_load_image_as_layer(path: PathBuf, tx: Sender<AppMsg>, ctx: egui::Context) {
    std::thread::spawn(move || {
        let result = canvas_io::load_image(&path).map_err(|e| e.to_string());
        let _ = tx.send(AppMsg::ImageLoadedForLayer { path, result });
        ctx.request_repaint();
    });
}

pub fn spawn_pick_replacement_image(
    layer: LayerId,
    start_dir: Option<PathBuf>,
    tx: Sender<AppMsg>,
    ctx: egui::Context,
) {
    std::thread::spawn(move || {
        let mut dialog = rfd::FileDialog::new()
            .set_title("Replace image")
            .add_filter("Images", canvas_io::IMAGE_EXTENSIONS);
        if let Some(dir) = start_dir.filter(|dir| dir.is_dir()) {
            dialog = dialog.set_directory(dir);
        }
        if let Some(path) = dialog.pick_file() {
            let label = image_label_from_path(&path);
            let result = canvas_io::load_image(&path).map_err(|e| e.to_string());
            let _ = tx.send(AppMsg::ImageLoadedForReplace {
                layer,
                label,
                source_path: Some(path),
                result,
            });
        }
        ctx.request_repaint();
    });
}

pub fn spawn_load_replacement_image_from_url(
    layer: LayerId,
    url: String,
    tx: Sender<AppMsg>,
    ctx: egui::Context,
) {
    std::thread::spawn(move || {
        let label = image_label_from_url(&url);
        let result = download_url_to_temp(&url).and_then(|path| {
            let loaded = canvas_io::load_image(&path).map_err(|e| e.to_string());
            let _ = std::fs::remove_file(&path);
            loaded
        });
        let _ = tx.send(AppMsg::ImageLoadedForReplace {
            layer,
            label,
            source_path: None,
            result,
        });
        ctx.request_repaint();
    });
}

fn image_label_from_path(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| "Image".to_owned())
}

fn image_label_from_url(url: &str) -> String {
    url.split(['?', '#'])
        .next()
        .and_then(|clean| clean.rsplit('/').next())
        .map(Path::new)
        .map(image_label_from_path)
        .filter(|name| name != "Image")
        .unwrap_or_else(|| "Internet image".to_owned())
}

fn extension_from_url(url: &str) -> &str {
    let clean = url.split(['?', '#']).next().unwrap_or(url);
    let ext = clean
        .rsplit('/')
        .next()
        .and_then(|name| Path::new(name).extension());
    ext.and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .filter(|value| canvas_io::IMAGE_EXTENSIONS.contains(&value.as_str()))
        .map(|value| match value.as_str() {
            "jpg" => "jpg",
            "jpeg" => "jpeg",
            "webp" => "webp",
            "gif" => "gif",
            "bmp" => "bmp",
            "svg" => "svg",
            _ => "png",
        })
        .unwrap_or("png")
}

fn download_url_to_temp(url: &str) -> Result<PathBuf, String> {
    let trimmed = url.trim();
    if !(trimmed.starts_with("http://") || trimmed.starts_with("https://")) {
        return Err("Use an http:// or https:// image URL".to_owned());
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis();
    let path = std::env::temp_dir().join(format!(
        "canvas-desktop-replace-{stamp}.{}",
        extension_from_url(trimmed)
    ));
    let out = path.to_string_lossy().into_owned();

    // `-Command` no vincula argumentos extra del proceso a `$args` (eso solo
    // pasa con `-File script.ps1 arg1 arg2`) — se pegarían sueltos al final
    // del script, dejando `$args[0]`/`$args[1]` siempre en `$null`. La URL y
    // la ruta van incrustadas como literales `'...'` del propio comando, con
    // la comilla simple escapada duplicándola (`''`), la forma estándar de
    // PowerShell — a diferencia de comillas dobles, no interpola variables.
    #[cfg(windows)]
    let output = {
        let escaped_url = trimmed.replace('\'', "''");
        let escaped_out = out.replace('\'', "''");
        Command::new("powershell")
            .arg("-NoProfile")
            .arg("-Command")
            .arg(format!(
                "Invoke-WebRequest -Uri '{escaped_url}' -OutFile '{escaped_out}'"
            ))
            .output()
    };

    #[cfg(not(windows))]
    let output = Command::new("curl")
        .arg("--location")
        .arg("--fail")
        .arg("--silent")
        .arg("--show-error")
        .arg("--output")
        .arg(&out)
        .arg(trimmed)
        .output();

    let output = output.map_err(|e| format!("Could not start downloader: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let message = if stderr.is_empty() {
            format!("Downloader exited with {}", output.status)
        } else {
            stderr
        };
        let _ = std::fs::remove_file(&path);
        return Err(message);
    }
    Ok(path)
}
pub fn spawn_pick_file(tx: Sender<AppMsg>, ctx: egui::Context) {
    std::thread::spawn(move || {
        let picked = rfd::FileDialog::new()
            .set_title("Open image")
            .add_filter("Images", canvas_io::IMAGE_EXTENSIONS)
            .pick_file();
        let _ = tx.send(AppMsg::FilePicked(picked));
        ctx.request_repaint();
    });
}

pub fn spawn_pick_folder(tx: Sender<AppMsg>, ctx: egui::Context) {
    std::thread::spawn(move || {
        let picked = rfd::FileDialog::new()
            .set_title("Open folder")
            .pick_folder();
        let _ = tx.send(AppMsg::FolderPicked(picked));
        ctx.request_repaint();
    });
}

/// Codifica y escribe (atómico) en un hilo de trabajo; el RGBA ya viene
/// horneado de la GPU.
#[allow(clippy::too_many_arguments)]
pub fn spawn_save(
    path: PathBuf,
    rgba: Vec<u8>,
    width: u32,
    height: u32,
    jpeg_quality: u8,
    metadata: Option<ImageMetadata>,
    new_source: bool,
    sidecar: Option<canvas_io::CanvasPayload>,
    tx: Sender<AppMsg>,
    ctx: egui::Context,
) {
    std::thread::spawn(move || {
        // La miniatura embebida del sidecar se reduce de este mismo RGBA
        // (ya horneado a tamaño completo): no hace falta una segunda pasada
        // de GPU solo para la celda de la galería.
        let preview = sidecar
            .is_some()
            .then(|| canvas_io::make_preview(&rgba, width, height))
            .flatten();
        // `save_rgba` devuelve los bytes EXACTOS que quedaron en disco (ya
        // con metadatos reinsertados): el hash del sidecar se calcula sobre
        // eso directamente, sin releer el archivo — en un PNG de 30 MP eso
        // ahorra decenas de MB de I/O en cada `Ctrl+S`.
        let save_result =
            canvas_io::save_rgba(&path, rgba, width, height, jpeg_quality, metadata.as_ref());
        let result = match save_result {
            Ok(bytes) => {
                match sidecar {
                    Some(mut payload) => {
                        payload.preview = preview;
                        if let Err(e) = canvas_io::write_sidecar(&path, &bytes, &payload) {
                            tracing::warn!("no se pudo escribir el sidecar: {e}");
                        }
                    }
                    // Sidecar desactivado: retira el que hubiera para no
                    // dejar uno obsoleto que luego avise de hash cambiado.
                    None => canvas_io::delete_sidecar(&path),
                }
                Ok(())
            }
            Err(e) => Err(e.to_string()),
        };
        let _ = tx.send(AppMsg::Saved {
            path,
            result,
            new_source,
        });
        ctx.request_repaint();
    });
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

pub fn spawn_pick_save_path(suggested: Option<String>, tx: Sender<AppMsg>, ctx: egui::Context) {
    // El lienzo no sabe guardar SVG: sugiere el mismo nombre en .png.
    let suggested = suggested.map(|name| {
        if name.to_ascii_lowercase().ends_with(".svg") {
            format!("{}.png", &name[..name.len() - 4])
        } else {
            name
        }
    });
    std::thread::spawn(move || {
        let mut dialog = rfd::FileDialog::new()
            .set_title("Save as…")
            .add_filter("PNG", &["png"])
            .add_filter("JPEG", &["jpg", "jpeg"])
            .add_filter("WebP", &["webp"])
            .add_filter("GIF", &["gif"])
            .add_filter("BMP", &["bmp"]);
        if let Some(name) = suggested {
            dialog = dialog.set_file_name(name);
        }
        let picked = dialog.save_file();
        let _ = tx.send(AppMsg::SaveAsPicked(picked));
        ctx.request_repaint();
    });
}

/// Diálogo «Guardar como…» de Export: un único filtro para el formato ya
/// elegido en el modal (a diferencia de `spawn_pick_save_path`, que ofrece
/// los cinco formatos rasterizables de Guardar).
pub fn spawn_pick_export_path(
    suggested_name: String,
    format: canvas_io::ExportFormat,
    tx: Sender<AppMsg>,
    ctx: egui::Context,
) {
    std::thread::spawn(move || {
        let picked = rfd::FileDialog::new()
            .set_title(format!("Export as {}…", format.label()))
            .add_filter(format.label(), &[format.extension()])
            .set_file_name(suggested_name)
            .save_file();
        let _ = tx.send(AppMsg::ExportPathPicked(picked));
        ctx.request_repaint();
    });
}

/// Codifica y escribe un export raster (PNG/JPEG) en un hilo de trabajo; el
/// RGBA ya viene horneado de la GPU a la escala elegida. Sin metadatos: no
/// es el archivo original que se está sobrescribiendo, es un export nuevo.
pub fn spawn_export_raster(
    path: PathBuf,
    rgba: Vec<u8>,
    width: u32,
    height: u32,
    jpeg_quality: u8,
    tx: Sender<AppMsg>,
    ctx: egui::Context,
) {
    std::thread::spawn(move || {
        let result = canvas_io::save_rgba(&path, rgba, width, height, jpeg_quality, None)
            .map(|_bytes| ())
            .map_err(|e| e.to_string());
        let _ = tx.send(AppMsg::Exported { path, result });
        ctx.request_repaint();
    });
}

/// Genera el SVG (y, si el formato lo pide, el PDF a partir de él) en un
/// hilo de trabajo: codifica cada capa raster a PNG, monta el documento a
/// mano y escribe atómicamente.
pub fn spawn_export_vector(
    path: PathBuf,
    document: canvas_core::Document,
    images: Vec<canvas_io::LayerPixels>,
    format: canvas_io::ExportFormat,
    scale: f64,
    tx: Sender<AppMsg>,
    ctx: egui::Context,
) {
    std::thread::spawn(move || {
        let result = (|| -> Result<(), String> {
            let mut export_images = canvas_io::ExportImages::new();
            for (id, rgba, w, h) in &images {
                let png_base64 =
                    canvas_io::encode_layer_png(rgba, *w, *h).map_err(|e| e.to_string())?;
                export_images.insert(*id, png_base64);
            }
            let svg = canvas_io::document_to_svg(
                &document,
                &export_images,
                scale,
                &canvas_render::text_lines,
            )
            .map_err(|e| e.to_string())?;
            let bytes = if format == canvas_io::ExportFormat::Pdf {
                canvas_io::svg_to_pdf(&svg).map_err(|e| e.to_string())?
            } else {
                svg.into_bytes()
            };
            canvas_io::write_atomic(&path, &bytes).map_err(|e| e.to_string())
        })();
        let _ = tx.send(AppMsg::Exported { path, result });
        ctx.request_repaint();
    });
}
