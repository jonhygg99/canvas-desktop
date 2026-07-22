//! Trabajo en hilos aparte: carga de imágenes y diálogos nativos. La UI nunca
//! bloquea en disco; los resultados llegan por canal.

use std::path::PathBuf;
use std::sync::mpsc::Sender;

use canvas_io::{ImageMetadata, LoadedImage, RestoredDocument};
use eframe::egui;

/// Resultado de abrir una imagen: mapa de bits plano, o documento con capas
/// restaurado desde su sidecar `.canvas`.
pub enum LoadOutcome {
    Flat(LoadedImage),
    Restored(RestoredDocument),
}

/// Datos que el hilo de guardado necesita para escribir el sidecar.
pub struct SidecarPayload {
    pub document: canvas_core::Document,
    pub images: Vec<canvas_io::LayerPixels>,
    pub background_layer: Option<u64>,
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
    SaveAsPicked(Option<PathBuf>),
    Saved {
        path: PathBuf,
        result: Result<(), String>,
        /// true si venía de «Guardar como…» y el documento debe apuntar aquí.
        new_source: bool,
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

/// Como `spawn_load_image`, pero el resultado se añade como capa nueva al
/// documento abierto en vez de sustituirlo.
pub fn spawn_load_image_as_layer(path: PathBuf, tx: Sender<AppMsg>, ctx: egui::Context) {
    std::thread::spawn(move || {
        let result = canvas_io::load_image(&path).map_err(|e| e.to_string());
        let _ = tx.send(AppMsg::ImageLoadedForLayer { path, result });
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
    sidecar: Option<SidecarPayload>,
    tx: Sender<AppMsg>,
    ctx: egui::Context,
) {
    std::thread::spawn(move || {
        let result =
            canvas_io::save_rgba(&path, rgba, width, height, jpeg_quality, metadata.as_ref())
                .map_err(|e| e.to_string());
        if result.is_ok() {
            match sidecar {
                Some(payload) => {
                    // El hash del sidecar debe ser el del archivo tal y como
                    // quedó en disco: se relee tras la escritura atómica.
                    match std::fs::read(&path) {
                        Ok(bytes) => {
                            if let Err(e) = canvas_io::write_sidecar(
                                &path,
                                &bytes,
                                &payload.document,
                                &payload.images,
                                payload.background_layer,
                            ) {
                                tracing::warn!("no se pudo escribir el sidecar: {e}");
                            }
                        }
                        Err(e) => {
                            tracing::warn!("no se pudo releer la imagen para el sidecar: {e}")
                        }
                    }
                }
                // Sidecar desactivado: retira el que hubiera para no dejar
                // uno obsoleto que luego avise de hash cambiado.
                None => canvas_io::delete_sidecar(&path),
            }
        }
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
        // Solo el primer nivel, solo imágenes, sin archivos ocultos.
        let mut files: Vec<(PathBuf, Option<std::time::SystemTime>)> = std::fs::read_dir(&folder)
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|p| {
                        p.is_file() && canvas_io::is_image_file(p) && !canvas_shell::is_hidden(p)
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
        files.par_iter().for_each_with(tx, |tx, (path, _mtime)| {
            let result =
                canvas_io::thumbnail(path, 256, cache_dir.as_deref()).map_err(|e| e.to_string());
            let _ = tx.send(AppMsg::GalleryThumb {
                folder: folder.clone(),
                path: path.clone(),
                result,
            });
            ctx.request_repaint();
        });
    });
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
