//! Miniaturas de la timeline: un trabajo a la vez y como máximo 120 texturas.
use eframe::egui;
use std::{
    collections::{HashMap, HashSet},
    path::Path,
    sync::mpsc,
};

#[derive(Default)]
pub(super) struct Thumbnails {
    textures: HashMap<usize, egui::TextureHandle>,
    failed: HashSet<usize>,
    pending: Option<mpsc::Receiver<(usize, Option<canvas_io::LoadedImage>)>>,
}

impl Thumbnails {
    pub fn texture(
        &mut self,
        index: usize,
        path: &Path,
        ctx: &egui::Context,
    ) -> Option<egui::TextureId> {
        if let Some(rx) = &self.pending {
            match rx.try_recv() {
                Ok((index, image)) => {
                    self.pending = None;
                    if let Some(image) = image {
                        self.textures.insert(
                            index,
                            ctx.load_texture(
                                format!("trim-thumb-{index}"),
                                egui::ColorImage::from_rgba_unmultiplied(
                                    [image.width as usize, image.height as usize],
                                    &image.rgba,
                                ),
                                egui::TextureOptions::LINEAR,
                            ),
                        );
                    } else {
                        self.failed.insert(index);
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => self.pending = None,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if let Some(texture) = self.textures.get(&index) {
            return Some(texture.id());
        }
        if self.pending.is_none() && !self.failed.contains(&index) && self.textures.len() < 120 {
            let (tx, rx) = mpsc::channel();
            self.pending = Some(rx);
            let path = path.to_owned();
            let ctx = ctx.clone();
            let viewport = ctx.viewport_id();
            std::thread::spawn(move || {
                let _ = tx.send((index, canvas_io::load_image(&path).ok()));
                ctx.request_repaint_of(viewport);
            });
        }
        None
    }
}
