//! Una sesión de encuadre por ventana/archivo, sin modificar el documento.
mod controls;
mod jobs;
pub(crate) mod preview;
mod status;
pub(crate) use status::{has_saved, record_saved};
#[cfg(test)]
mod tests;

use canvas_core::framing::Framing;
use eframe::egui;
use std::{path::PathBuf, sync::mpsc::Receiver};

pub(crate) struct Session {
    pub path: Option<PathBuf>,
    pub value: Framing,
    pub saved: Option<Framing>,
    pub error: Option<String>,
    pub status: Option<String>,
    pub closed: bool,
    pub exportable: bool,
    source: Option<canvas_io::LoadedImage>,
    texture: Option<egui::TextureHandle>,
    background: Option<egui::TextureHandle>,
    receiver: Option<Receiver<Result<jobs::Outcome, String>>>,
    undo: Vec<Framing>,
    redo: Vec<Framing>,
    gesture: Option<Framing>,
}

impl Session {
    pub fn new(path: Option<PathBuf>, exportable: bool) -> Self {
        Self {
            path,
            value: Framing::default(),
            saved: None,
            error: None,
            status: None,
            closed: false,
            exportable,
            source: None,
            texture: None,
            background: None,
            receiver: None,
            undo: Vec::new(),
            redo: Vec::new(),
            gesture: None,
        }
    }

    pub fn open(path: PathBuf, ctx: &egui::Context) -> Self {
        let mut session = Self::new(Some(path.clone()), false);
        session.receiver = Some(jobs::load(path, ctx.clone()));
        session
    }

    pub fn from_composition(
        path: Option<PathBuf>,
        source: canvas_io::LoadedImage,
        ctx: &egui::Context,
    ) -> Self {
        let mut session = Self::new(path.clone(), true);
        session.receiver = Some(jobs::prepare(path, source, ctx.clone()));
        session
    }

    pub fn poll(&mut self, ctx: &egui::Context) {
        let result = self.receiver.as_ref().and_then(|rx| match rx.try_recv() {
            Ok(value) => Some(value),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                Some(Err("Framing worker stopped unexpectedly".into()))
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
        });
        if let Some(result) = result {
            self.receiver = None;
            match result {
                Ok(jobs::Outcome::Loaded {
                    source,
                    background,
                    framing,
                    error,
                }) => {
                    self.saved = framing;
                    if let Some(path) = &self.path {
                        record_saved(ctx, path, framing.is_some());
                    }
                    self.value = framing.unwrap_or_default();
                    self.error = error;
                    let (w, h) = (source.width as usize, source.height as usize);
                    self.texture = Some(ctx.load_texture(
                        "framing-source",
                        egui::ColorImage::from_rgba_unmultiplied([w, h], &source.rgba),
                        egui::TextureOptions::LINEAR,
                    ));
                    self.background = Some(ctx.load_texture(
                        "framing-background",
                        background,
                        egui::TextureOptions::LINEAR,
                    ));
                    self.source = Some(source);
                }
                Ok(jobs::Outcome::Saved {
                    value,
                    path,
                    sidecar_only,
                }) => {
                    if sidecar_only {
                        self.saved = Some(value);
                        if let Some(path) = &self.path {
                            record_saved(ctx, path, true);
                        }
                    }
                    self.status = Some(format!("Saved: {}", path.display()));
                    self.error = None;
                }
                Ok(jobs::Outcome::Cancelled) => {}
                Err(error) => self.error = Some(error),
            }
        }
    }

    pub fn busy(&self) -> bool {
        self.receiver.is_some()
    }
    pub fn ready(&self) -> bool {
        self.texture.is_some()
    }
    pub fn save(&mut self, ctx: &egui::Context) {
        if self.ready() && !self.busy() {
            if let Some(path) = &self.path {
                tracing::info!(path = %path.display(), framing = ?self.value, "saving framing");
                self.receiver = Some(jobs::save(path.clone(), self.value, ctx.clone()));
            }
        }
    }

    fn commit(&mut self, before: Framing) {
        if before != self.value {
            self.undo.push(before);
            self.redo.clear();
        }
    }

    pub fn undo(&mut self) {
        if let Some(value) = self.undo.pop() {
            self.redo.push(self.value);
            self.value = value;
        }
    }

    pub fn redo(&mut self) {
        if let Some(value) = self.redo.pop() {
            self.undo.push(self.value);
            self.value = value;
        }
    }

    pub fn controls(&mut self, ui: &mut egui::Ui) {
        controls::show(self, ui);
    }
    pub fn canvas(&mut self, ui: &mut egui::Ui) {
        preview::show(self, ui);
    }
}

pub(crate) fn portrait_icon(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let size = egui::vec2(rect.height() * 9.0 / 16.0, rect.height());
    painter.rect_stroke(
        egui::Rect::from_center_size(rect.center(), size),
        2.0,
        egui::Stroke::new(1.5, color),
        egui::StrokeKind::Inside,
    );
}
