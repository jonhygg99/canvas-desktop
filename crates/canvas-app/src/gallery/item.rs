//! Elemento de la galería: un archivo (imagen o diseño autónomo) y su
//! miniatura ya subida a GPU (si llegó).

use std::path::PathBuf;
use std::time::SystemTime;

use eframe::egui;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ItemKind {
    Image,
    Design,
}

pub struct GalleryItem {
    pub path: PathBuf,
    pub name: String,
    pub mtime: Option<SystemTime>,
    pub kind: ItemKind,
    pub tex: Option<egui::TextureHandle>,
    pub failed: bool,
}
