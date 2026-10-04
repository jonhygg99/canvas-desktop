//! Reproducci?n temporal del medio individual; no cambia el v?deo original.
mod controls;
mod decoder;
#[cfg(test)]
mod tests;
use eframe::egui;
use std::path::PathBuf;

pub(super) struct Video {
    path: PathBuf,
    pub position: f64,
    pub duration: f64,
    pub playing: bool,
    size: (u32, u32),
    worker: Option<decoder::Worker>,
}
impl Video {
    pub fn new(path: PathBuf, size: (u32, u32), duration: f64) -> Self {
        let scale = (960.0 / f64::from(size.0.max(size.1))).min(1.0);
        Self {
            path,
            position: 0.0,
            duration,
            playing: false,
            size: (
                (f64::from(size.0) * scale).round().max(2.0) as u32,
                (f64::from(size.1) * scale).round().max(2.0) as u32,
            ),
            worker: None,
        }
    }
    pub fn seek(&mut self, time: f64, playing: bool, ctx: &egui::Context) {
        self.worker = None;
        self.position = time.clamp(0.0, (self.duration - 0.05).max(0.0));
        self.playing = playing;
        self.worker = Some(decoder::Worker::start(
            self.path.clone(),
            self.position,
            self.duration,
            self.size,
            playing,
            ctx.clone(),
        ));
    }
    pub fn pause(&mut self) {
        self.playing = false;
        self.worker = None;
    }
    pub fn poll(&mut self) -> Option<Result<decoder::Frame, String>> {
        let frame = self.worker.as_ref()?.take()?;
        match frame {
            Ok(Some(frame)) => {
                self.position = frame.time;
                Some(Ok(frame))
            }
            Ok(None) => {
                self.pause();
                None
            }
            Err(error) => {
                self.pause();
                Some(Err(error))
            }
        }
    }
}

impl Video {
    pub fn controls(&mut self, ui: &mut egui::Ui) {
        controls::show(self, ui);
    }
}
