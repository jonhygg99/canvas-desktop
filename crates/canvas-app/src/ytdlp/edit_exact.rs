//! Scrub preciso: un decode pendiente y dos texturas, sin bloquear egui.
use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use eframe::egui;

use super::playback::{prepare_background, update_texture};

type Decoded = Result<(f64, u8, egui::ColorImage, Option<egui::ColorImage>), String>;

#[derive(Default)]
pub(super) struct ExactPreview {
    requested: Option<(f64, Instant)>,
    pending: Option<mpsc::Receiver<Decoded>>,
    sharp: Option<egui::TextureHandle>,
    background: Option<egui::TextureHandle>,
    shown: Option<(f64, u8)>,
    failed: Option<(f64, u8)>,
}

impl ExactPreview {
    pub(super) fn seek(&mut self, time: f64) {
        self.requested = Some((time, Instant::now()));
        self.failed = None;
    }

    #[cfg(test)]
    pub(super) fn shown_time(&self) -> Option<f64> {
        self.shown.map(|(time, _)| time)
    }

    pub(super) fn textures(
        &mut self,
        path: &Path,
        step: u8,
        ctx: &egui::Context,
    ) -> Option<(egui::TextureId, Option<egui::TextureId>)> {
        let (time, changed) = self.requested?;
        self.poll(time, step, ctx);
        let key = (time, step);
        if self.pending.is_none() && self.shown != Some(key) && self.failed != Some(key) {
            // Mientras se arrastra, agrupa los seeks. Al soltar llega el último.
            if changed.elapsed() >= Duration::from_millis(90) {
                self.decode(path, time, step, ctx);
            } else {
                ctx.request_repaint_after(Duration::from_millis(90));
            }
        }
        Some((
            self.sharp.as_ref()?.id(),
            self.background.as_ref().map(|t| t.id()),
        ))
    }

    fn poll(&mut self, time: f64, step: u8, ctx: &egui::Context) {
        let Some(rx) = &self.pending else { return };
        let result = match rx.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => Err("Preview worker stopped".into()),
        };
        self.pending = None;
        match result {
            Ok((decoded_time, decoded_step, sharp, background))
                if (decoded_time, decoded_step) == (time, step) =>
            {
                update_texture(&mut self.sharp, sharp, "video-exact", ctx);
                if let Some(background) = background {
                    update_texture(&mut self.background, background, "video-exact-bg", ctx);
                } else {
                    self.background = None;
                }
                self.shown = Some((time, step));
            }
            Err(error) => {
                self.failed = Some((time, step));
                tracing::warn!("Exact video preview: {error}");
            }
            _ => {} // Descarta respuestas de un seek que ya cambió.
        }
    }

    fn decode(&mut self, path: &Path, time: f64, step: u8, ctx: &egui::Context) {
        let path = path.to_owned();
        let ctx = ctx.clone();
        let viewport = ctx.viewport_id();
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        std::thread::spawn(move || {
            let result = canvas_io::load_video_frame(&path, time)
                .map(|image| {
                    let pixels = image::RgbaImage::from_raw(image.width, image.height, image.rgba)
                        .expect("frame RGBA completo");
                    let small = image::imageops::thumbnail(&pixels, 480, 480);
                    let image = canvas_io::LoadedImage {
                        width: small.width(),
                        height: small.height(),
                        rgba: small.into_raw(),
                    };
                    let sharp = egui::ColorImage::from_rgba_unmultiplied(
                        [image.width as usize, image.height as usize],
                        &image.rgba,
                    );
                    let background = (step != 0).then(|| prepare_background(&image, step));
                    (time, step, sharp, background)
                })
                .map_err(|error| error.to_string());
            if tx.send(result).is_ok() {
                ctx.request_repaint_of(viewport);
            }
        });
    }
}
