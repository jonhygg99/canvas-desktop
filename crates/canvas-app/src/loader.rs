//! Trabajo en hilos aparte: carga de imágenes y diálogos nativos. La UI nunca
//! bloquea en disco; los resultados llegan por canal.

use std::path::PathBuf;
use std::sync::mpsc::Sender;

use canvas_io::LoadedImage;
use eframe::egui;

pub enum AppMsg {
    FilePicked(Option<PathBuf>),
    FolderPicked(Option<PathBuf>),
    ImageLoaded {
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
        files: Vec<PathBuf>,
    },
    GalleryThumb {
        folder: PathBuf,
        index: usize,
        result: Result<LoadedImage, String>,
    },
}

pub fn spawn_load_image(path: PathBuf, tx: Sender<AppMsg>, ctx: egui::Context) {
    std::thread::spawn(move || {
        let result = canvas_io::load_image(&path).map_err(|e| e.to_string());
        let _ = tx.send(AppMsg::ImageLoaded { path, result });
        ctx.request_repaint();
    });
}

pub fn spawn_pick_file(tx: Sender<AppMsg>, ctx: egui::Context) {
    std::thread::spawn(move || {
        let picked = rfd::FileDialog::new()
            .set_title("Abrir imagen")
            .add_filter("Imágenes", canvas_io::IMAGE_EXTENSIONS)
            .pick_file();
        let _ = tx.send(AppMsg::FilePicked(picked));
        ctx.request_repaint();
    });
}

pub fn spawn_pick_folder(tx: Sender<AppMsg>, ctx: egui::Context) {
    std::thread::spawn(move || {
        let picked = rfd::FileDialog::new()
            .set_title("Abrir carpeta")
            .pick_folder();
        let _ = tx.send(AppMsg::FolderPicked(picked));
        ctx.request_repaint();
    });
}

/// Codifica y escribe (atómico) en un hilo de trabajo; el RGBA ya viene
/// horneado de la GPU.
pub fn spawn_save(
    path: PathBuf,
    rgba: Vec<u8>,
    width: u32,
    height: u32,
    new_source: bool,
    tx: Sender<AppMsg>,
    ctx: egui::Context,
) {
    std::thread::spawn(move || {
        let result = canvas_io::save_rgba(&path, rgba, width, height).map_err(|e| e.to_string());
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
        let mut files: Vec<PathBuf> = std::fs::read_dir(&folder)
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|p| p.is_file() && canvas_io::is_image_file(p))
                    .collect()
            })
            .unwrap_or_default();
        files.sort_by_key(|p| p.file_name().map(|n| n.to_ascii_lowercase()));

        let _ = tx.send(AppMsg::GalleryScanned {
            folder: folder.clone(),
            files: files.clone(),
        });
        ctx.request_repaint();

        use rayon::prelude::*;
        files
            .par_iter()
            .enumerate()
            .for_each_with(tx, |tx, (index, path)| {
                let result = canvas_io::thumbnail(path, 256, cache_dir.as_deref())
                    .map_err(|e| e.to_string());
                let _ = tx.send(AppMsg::GalleryThumb {
                    folder: folder.clone(),
                    index,
                    result,
                });
                ctx.request_repaint();
            });
    });
}

pub fn spawn_pick_save_path(suggested: Option<String>, tx: Sender<AppMsg>, ctx: egui::Context) {
    std::thread::spawn(move || {
        let mut dialog = rfd::FileDialog::new()
            .set_title("Guardar como…")
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
