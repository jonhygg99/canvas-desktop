//! Metadatos y previews acotados de Gallery; cada ventana posee sus resultados.
use canvas_core::framing::Framing;
use eframe::egui;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::mpsc,
};
#[cfg(test)]
#[path = "framing_tests.rs"]
mod tests;

#[derive(Default, Clone, Copy, PartialEq)]
pub(crate) enum StatusFilter {
    #[default]
    All,
    Saved,
    Missing,
}

#[derive(Default)]
pub(crate) struct Card {
    pub saved: Option<Framing>,
    pub error: Option<String>,
    pub texture: Option<egui::TextureHandle>,
    pub background: Option<egui::TextureHandle>,
    preview_failed: bool,
    last_used: u64,
}

struct ResultCard {
    path: PathBuf,
    version: u64,
    saved: Option<Framing>,
    error: Option<String>,
    source: Option<canvas_io::LoadedImage>,
    background: Option<egui::ColorImage>,
}

pub(crate) struct GalleryFramings {
    pub vertical: bool,
    pub filter: StatusFilter,
    pub session: Option<crate::framing::Session>,
    cards: HashMap<PathBuf, Card>,
    versions: HashMap<PathBuf, u64>,
    inflight: HashSet<PathBuf>,
    tx: mpsc::Sender<ResultCard>,
    rx: mpsc::Receiver<ResultCard>,
    tick: u64,
}

impl Default for GalleryFramings {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            vertical: false,
            filter: StatusFilter::All,
            session: None,
            cards: HashMap::new(),
            versions: HashMap::new(),
            inflight: HashSet::new(),
            tx,
            rx,
            tick: 0,
        }
    }
}

impl GalleryFramings {
    pub fn poll(&mut self, ctx: &egui::Context) {
        self.tick += 1;
        while let Ok(result) = self.rx.try_recv() {
            self.inflight.remove(&result.path);
            if self.versions.get(&result.path).copied().unwrap_or(0) != result.version {
                continue;
            }
            let card = self.cards.entry(result.path).or_default();
            card.saved = result.saved;
            card.error = result.error;
            if let Some(source) = result.source {
                card.texture = Some(ctx.load_texture(
                    "gallery-framing",
                    egui::ColorImage::from_rgba_unmultiplied(
                        [source.width as usize, source.height as usize],
                        &source.rgba,
                    ),
                    egui::TextureOptions::LINEAR,
                ));
                card.background = result.background.map(|bg| {
                    ctx.load_texture("gallery-framing-bg", bg, egui::TextureOptions::LINEAR)
                });
            } else if card.error.is_some() {
                card.preview_failed = true;
            }
        }
        let mut cached: Vec<_> = self
            .cards
            .iter()
            .filter(|(_, card)| card.texture.is_some())
            .map(|(path, card)| (path.clone(), card.last_used))
            .collect();
        cached.sort_by_key(|(_, tick)| *tick);
        let excess = cached.len().saturating_sub(64);
        for (path, _) in cached.into_iter().take(excess) {
            if let Some(card) = self.cards.get_mut(&path) {
                card.texture = None;
                card.background = None;
            }
        }
    }

    pub fn invalidate(&mut self, path: &Path) {
        *self.versions.entry(path.to_owned()).or_default() += 1;
        self.cards.remove(path);
    }

    pub fn card(&mut self, path: &Path, ctx: &egui::Context, preview: bool) -> Option<&Card> {
        let needs_load = self.cards.get(path).is_none_or(|c| {
            preview && c.saved.is_some() && c.texture.is_none() && !c.preview_failed
        });
        if needs_load && self.inflight.len() < 4 && self.inflight.insert(path.to_owned()) {
            let path = path.to_owned();
            let tx = self.tx.clone();
            let ctx = ctx.clone();
            let version = self.versions.get(&path).copied().unwrap_or(0);
            rayon::spawn(move || {
                let (saved, mut error) = match canvas_io::read_framing(&path) {
                    Ok(f) => (f, None),
                    Err(e) => (None, Some(e.to_string())),
                };
                let source = if preview && saved.is_some() {
                    match canvas_io::thumbnail(&path, 512, None) {
                        Ok(img) => Some(img),
                        Err(e) => {
                            error = Some(e.to_string());
                            None
                        }
                    }
                } else {
                    None
                };
                let background = source
                    .as_ref()
                    .and_then(|s| crate::framing::preview::background(s).ok());
                let _ = tx.send(ResultCard {
                    path,
                    version,
                    saved,
                    error,
                    source,
                    background,
                });
                ctx.request_repaint();
            });
        }
        if let Some(card) = self.cards.get_mut(path) {
            card.last_used = self.tick;
        }
        self.cards.get(path)
    }

    pub fn matches(&self, path: &Path) -> bool {
        match (self.filter, self.cards.get(path)) {
            (StatusFilter::Saved, Some(card)) => card.saved.is_some(),
            (StatusFilter::Missing, Some(card)) => card.saved.is_none(),
            _ => true,
        }
    }

    pub fn modal(&mut self, ctx: &egui::Context) {
        let Some(session) = &mut self.session else {
            return;
        };
        let before = session.saved;
        let mut open = true;
        egui::Window::new("Framing 9:16")
            .id(egui::Id::new("gallery-framing-editor"))
            .open(&mut open)
            .default_size(egui::vec2(680.0, 700.0))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(230.0);
                        ui.set_min_height(580.0);
                        egui::ScrollArea::vertical()
                            .id_salt("framing-controls")
                            .max_height(ui.available_height().max(580.0))
                            .show(ui, |ui| session.controls(ui));
                    });
                    ui.separator();
                    ui.vertical(|ui| {
                        ui.set_min_size(egui::vec2(300.0, 580.0));
                        session.canvas(ui);
                    });
                });
            });
        let changed = before != session.saved;
        let path = session.path.clone();
        let close = session.closed || (!open && !session.busy());
        if close {
            self.session = None;
        }
        if changed {
            if let Some(path) = path {
                self.invalidate(&path);
            }
        }
    }
}
