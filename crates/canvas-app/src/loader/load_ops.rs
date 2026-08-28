//! Abrir imágenes, diseños autónomos y ranuras de la baraja del editor; y los
//! diálogos nativos de "abrir archivo"/"abrir carpeta".

use std::path::PathBuf;
use std::sync::mpsc::Sender;

use canvas_io::ImageMetadata;
use eframe::egui;

use super::{AppMsg, LoadOutcome};
use crate::app::persistence::build_slot_doc;

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
            canvas_io::load_image(&path).map(LoadOutcome::Flat)
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
        let result = canvas_io::read_design(&path).map(LoadOutcome::Design);
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
pub fn spawn_load_slot(
    folder: PathBuf,
    path: PathBuf,
    generation: u64,
    sidecar_default: bool,
    tx: Sender<AppMsg>,
    ctx: egui::Context,
) {
    std::thread::spawn(move || {
        let started = std::time::Instant::now();
        let is_canvas = canvas_io::is_canvas_file(&path);
        let result = if is_canvas {
            canvas_io::read_design(&path).map(LoadOutcome::Design)
        } else {
            match canvas_io::read_sidecar(&path) {
                Ok(Some(restored)) => Ok(LoadOutcome::Restored(restored)),
                Ok(None) => canvas_io::load_image(&path).map(LoadOutcome::Flat),
                Err(e) => {
                    tracing::warn!("sidecar ilegible ({e}); abriendo la imagen aplanada");
                    canvas_io::load_image(&path).map(LoadOutcome::Flat)
                }
            }
        };
        let metadata = if is_canvas {
            ImageMetadata::default()
        } else {
            canvas_io::extract_metadata_from_file(&path)
        };
        let prepared = result.and_then(|outcome| {
            build_slot_doc(
                path.clone(),
                outcome,
                (!metadata.is_empty()).then_some(metadata),
                sidecar_default,
            )
            .ok_or_else(|| canvas_io::IoError::Message {
                message: "could not build the background document".to_owned(),
            })
        });
        tracing::debug!(
            target: "canvas.preload",
            path = %path.display(),
            generation,
            elapsed_ms = started.elapsed().as_secs_f64() * 1000.0,
            "preload prepared"
        );
        let _ = tx.send(AppMsg::SlotPrepared {
            folder,
            generation,
            path,
            result: prepared,
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
pub(super) fn probe_page_sizes(
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
    generation: u64,
    paths: Vec<PathBuf>,
    tx: Sender<AppMsg>,
    ctx: egui::Context,
) {
    std::thread::spawn(move || {
        let sizes = probe_page_sizes(&folder, paths);
        let _ = tx.send(AppMsg::DeckProbed {
            folder,
            generation,
            sizes,
        });
        ctx.request_repaint();
    });
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
