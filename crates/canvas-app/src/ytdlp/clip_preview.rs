//! Miniatura y duración de tarjetas, con un único trabajo fuera de egui.
use eframe::egui;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

#[derive(Default)]
pub(crate) struct ClipPreviews {
    ready: HashMap<PathBuf, ClipPreview>,
    pending: Option<(PathBuf, mpsc::Receiver<Decoded>)>,
}

pub(super) struct ClipPreview {
    pub texture: Option<egui::TextureHandle>,
    pub duration: Option<f64>,
}

struct Decoded {
    image: Option<egui::ColorImage>,
    duration: Option<f64>,
}

impl ClipPreviews {
    pub(super) fn update(&mut self, paths: &[PathBuf], ctx: &egui::Context) {
        self.ready.retain(|path, _| paths.contains(path));
        if let Some((path, rx)) = &self.pending {
            match rx.try_recv() {
                Ok(frame) => {
                    if paths.contains(path) {
                        self.ready.insert(
                            path.clone(),
                            ClipPreview {
                                duration: frame.duration,
                                texture: frame.image.map(|image| {
                                    ctx.load_texture(
                                        format!("download-card-{}", path.display()),
                                        image,
                                        egui::TextureOptions::LINEAR,
                                    )
                                }),
                            },
                        );
                    }
                    self.pending = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.ready.insert(
                        path.clone(),
                        ClipPreview {
                            texture: None,
                            duration: None,
                        },
                    );
                    self.pending = None;
                }
                Err(mpsc::TryRecvError::Empty) => return,
            }
        }
        if let Some(path) = paths.iter().find(|path| !self.ready.contains_key(*path)) {
            self.request(path, ctx);
        }
    }

    pub(super) fn get(&self, path: &Path) -> Option<&ClipPreview> {
        self.ready.get(path)
    }

    fn request(&mut self, path: &Path, ctx: &egui::Context) {
        let path = path.to_owned();
        let (tx, rx) = mpsc::channel();
        self.pending = Some((path.clone(), rx));
        let ctx = ctx.clone();
        let viewport = ctx.viewport_id();
        std::thread::spawn(move || {
            let duration = canvas_io::probe_video_size(&path)
                .ok()
                .and_then(|(_, _, d)| d);
            let image = canvas_io::load_video_frame(&path, 0.0)
                .ok()
                .and_then(|image| {
                    let pixels = image::RgbaImage::from_raw(image.width, image.height, image.rgba)?;
                    let small = image::imageops::thumbnail(&pixels, 192, 108);
                    Some(egui::ColorImage::from_rgba_unmultiplied(
                        [small.width() as usize, small.height() as usize],
                        small.as_raw(),
                    ))
                });
            if tx.send(Decoded { image, duration }).is_ok() {
                ctx.request_repaint_of(viewport);
            }
        });
    }
}
