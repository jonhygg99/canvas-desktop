//! Trabajo en hilos aparte: carga de imágenes y diálogos nativos. La UI nunca
//! bloquea en disco; los resultados llegan por canal.

use std::path::PathBuf;
use std::sync::mpsc::Sender;

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
}

/// Operación de archivos pedida desde la galería. Siempre en un hilo aparte:
/// copiar un PNG grande no puede bloquear la UI.
pub enum GalleryOp {
    /// Crea un `.canvas` en blanco en `folder` con un nombre libre.
    NewDesign { folder: PathBuf, page: (f64, f64) },
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
        let src_sidecar = canvas_io::sidecar_path(src);
        if src_sidecar.is_file() {
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
        let src_sidecar = canvas_io::sidecar_path(path);
        if src_sidecar.is_file() {
            let dst_sidecar = canvas_io::sidecar_path(&dst);
            if let Err(e) = std::fs::rename(&src_sidecar, &dst_sidecar) {
                tracing::warn!("no se pudo renombrar el sidecar: {e}");
            }
        }
    }
    Ok(dst)
}

/// Envía `path` (y su sidecar, si es una imagen que tiene uno) a la
/// Papelera de reciclaje del sistema: recuperable si el usuario se
/// equivoca, a diferencia de un borrado permanente.
fn trash_with_sidecar(path: &std::path::Path) -> Result<(), String> {
    trash::delete(path).map_err(|e| e.to_string())?;
    if canvas_io::is_image_file(path) {
        let sidecar = canvas_io::sidecar_path(path);
        if sidecar.is_file() {
            if let Err(e) = trash::delete(&sidecar) {
                tracing::warn!("no se pudo borrar el sidecar: {e}");
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
            GalleryOp::NewDesign { folder, page } => {
                let outcome = canvas_io::reserve_unique_path(
                    &folder,
                    "Untitled",
                    canvas_io::CANVAS_EXTENSION,
                )
                .map_err(|e| e.to_string())
                .and_then(|path| {
                    canvas_io::write_design(&path, &canvas_io::blank_design(page.0, page.1))
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

/// Envía a la Papelera de reciclaje el archivo abierto en el editor (y su
/// sidecar) — disparado desde el botón «Delete» del panel de propiedades.
pub fn spawn_document_delete(path: PathBuf, tx: Sender<AppMsg>, ctx: egui::Context) {
    std::thread::spawn(move || {
        let result = trash_with_sidecar(&path);
        let _ = tx.send(AppMsg::DocumentDeleted { path, result });
        ctx.request_repaint();
    });
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
        let result =
            canvas_io::save_rgba(&path, rgba, width, height, jpeg_quality, metadata.as_ref())
                .map_err(|e| e.to_string());
        if result.is_ok() {
            match sidecar {
                Some(mut payload) => {
                    payload.preview = preview;
                    // El hash del sidecar debe ser el del archivo tal y como
                    // quedó en disco: se relee tras la escritura atómica.
                    match std::fs::read(&path) {
                        Ok(bytes) => {
                            if let Err(e) = canvas_io::write_sidecar(&path, &bytes, &payload) {
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
        // Solo el primer nivel, imágenes y diseños, sin archivos ocultos.
        // `is_standalone_design` deja fuera el sidecar de una imagen que ya
        // sale por sí sola en la cuadrícula.
        let mut files: Vec<(PathBuf, Option<std::time::SystemTime>)> = std::fs::read_dir(&folder)
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|p| {
                        p.is_file()
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
