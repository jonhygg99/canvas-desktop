//! Reproducción independiente de la selección y de los desplegables del panel.
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use canvas_core::{LayerContent, LayerId, VideoContent};
use canvas_io::LoadedImage;
use eframe::egui;

use super::EditorState;

const FPS: u32 = 20;
#[cfg(test)]
#[path = "video_playback_tests.rs"]
mod tests;
type FrameResult = Result<Option<(f64, LoadedImage)>, String>;

pub(crate) struct VideoPlayback {
    layer: LayerId,
    slot: u64,
    video: VideoContent,
    frames: Arc<Mutex<Option<FrameResult>>>,
    cancel: mpsc::Sender<()>,
    playing: bool,
}

impl VideoPlayback {
    fn start(
        layer: LayerId,
        slot: u64,
        video: VideoContent,
        playing: bool,
        ctx: egui::Context,
    ) -> Self {
        let frames = Arc::new(Mutex::new(None));
        let mailbox = frames.clone();
        let (cancel, cancelled) = mpsc::channel();
        let source = video.clone();
        let viewport = ctx.viewport_id();
        std::thread::spawn(move || {
            let publish = |frame| {
                if let Ok(mut pending) = mailbox.lock() {
                    *pending = Some(frame);
                }
                ctx.request_repaint_of(viewport);
            };
            let path = source
                .source_path
                .as_deref()
                .expect("video source validated");
            if !playing {
                publish(
                    canvas_io::load_video_frame(path, source.poster_time)
                        .map(|img| Some((source.poster_time, img)))
                        .map_err(|e| e.to_string()),
                );
                return;
            }
            let mut stream = match canvas_io::VideoFrameStream::open(
                path,
                source.poster_time,
                (source.natural_width, source.natural_height),
                FPS,
            ) {
                Ok(stream) => stream,
                Err(e) => {
                    publish(Err(e.to_string()));
                    return;
                }
            };
            let (_, end) = source.playback_range();
            let mut clock = None;
            let mut index = 0u64;
            loop {
                if cancelled.try_recv() != Err(mpsc::TryRecvError::Empty) {
                    break;
                }
                let time = source.poster_time + index as f64 / f64::from(FPS);
                let frame = if end.is_some_and(|end| time >= end) {
                    Ok(None)
                } else {
                    stream
                        .next_frame()
                        .map(|img| img.map(|img| (time, img)))
                        .map_err(|e| e.to_string())
                };
                let start = *clock.get_or_insert_with(Instant::now);
                let wait = Duration::from_secs_f64(index as f64 / f64::from(FPS))
                    .saturating_sub(start.elapsed());
                if cancelled.recv_timeout(wait) != Err(mpsc::RecvTimeoutError::Timeout) {
                    break;
                }
                let done = !matches!(frame, Ok(Some(_)));
                publish(frame);
                if done {
                    break;
                }
                index += 1;
            }
        });
        Self {
            layer,
            slot,
            video,
            frames,
            cancel,
            playing,
        }
    }

    fn matches(&self, state: &EditorState) -> bool {
        self.slot == state.active_slot_id && state.doc.layer(self.layer).is_ok_and(|layer| {
            matches!(&layer.content, LayerContent::Video(v) if
                v.source_path == self.video.source_path && v.playback_range() == self.video.playback_range()
                && (v.natural_width, v.natural_height) == (self.video.natural_width, self.video.natural_height))
        })
    }
}

impl Drop for VideoPlayback {
    fn drop(&mut self) {
        let _ = self.cancel.send(());
        if self.playing {
            if let Some(path) = self.video.source_path.as_deref() {
                crate::audio::pause_for(path);
            }
        }
    }
}

impl EditorState {
    pub(crate) fn video_layer(&self) -> Option<LayerId> {
        let eligible = |id| {
            self.doc.layer(id).is_ok_and(|l| {
                Some(id) != self.background_layer && matches!(l.content, LayerContent::Video(_))
            })
        };
        self.selection
            .primary()
            .filter(|id| eligible(*id))
            .or(self.video_playing_layer.filter(|id| eligible(*id)))
            .or_else(|| {
                self.doc
                    .page()
                    .ok()?
                    .layers
                    .iter()
                    .rev()
                    .find(|l| eligible(l.id))
                    .map(|l| l.id)
            })
    }

    pub(crate) fn pause_video(&mut self) {
        self.video_playing_layer = None;
        self.video_playback = None;
    }

    pub(crate) fn seek_video(
        &mut self,
        layer: LayerId,
        time: f64,
        playing: bool,
        ctx: &egui::Context,
    ) {
        self.pause_video();
        let Ok(l) = self.doc.layer_mut(layer) else {
            return;
        };
        let LayerContent::Video(video) = &mut l.content else {
            return;
        };
        let Some(path) = video.source_path.clone() else {
            return;
        };
        let (start, end) = video.playback_range();
        video.poster_time = if time.is_finite() {
            time.max(start)
        } else {
            start
        };
        if let Some(end) = end {
            // FFmpeg devuelve EOF si se pide exactamente el final del archivo.
            video.poster_time = video
                .poster_time
                .min((end - 1.0 / f64::from(FPS)).max(start));
        }
        let video = video.clone();
        if playing {
            self.video_playing_layer = Some(layer);
            if let Err(e) = crate::audio::play(&path, video.poster_time) {
                tracing::debug!("video preview without audio: {e}");
            }
        }
        self.video_playback = Some(VideoPlayback::start(
            layer,
            self.active_slot_id,
            video,
            playing,
            ctx.clone(),
        ));
    }

    pub(crate) fn tick_video(&mut self, ctx: &egui::Context) {
        if self
            .video_playback
            .as_ref()
            .is_some_and(|p| !p.matches(self))
        {
            self.pause_video();
        }
        if let Some(layer) = self.video_playing_layer {
            if self
                .video_playback
                .as_ref()
                .is_none_or(|p| p.layer != layer)
            {
                let time = self
                    .doc
                    .layer(layer)
                    .ok()
                    .and_then(|l| match &l.content {
                        LayerContent::Video(v) => Some(v.poster_time),
                        _ => None,
                    })
                    .unwrap_or(0.0);
                self.seek_video(layer, time, true, ctx);
            }
        }
        let Some(playback) = &self.video_playback else {
            return;
        };
        let layer = playback.layer;
        let frame = playback
            .frames
            .lock()
            .ok()
            .and_then(|mut frame| frame.take());
        match frame {
            Some(Ok(Some((time, image)))) => self.apply_video_frame(layer, time, image),
            Some(Ok(None)) => {
                let start = playback.video.playback_range().0;
                let playing = self.video_playing_layer == Some(layer);
                self.seek_video(layer, start, playing, ctx);
            }
            Some(Err(error)) => {
                self.pause_video();
                self.save_error = Some(error);
            }
            None => {}
        }
        if self.video_playing_layer.is_some() {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
    }

    fn apply_video_frame(&mut self, layer: LayerId, time: f64, image: LoadedImage) {
        let path: Option<PathBuf> = match self.doc.layer_mut(layer) {
            Ok(l) => match &mut l.content {
                LayerContent::Video(v) => {
                    v.poster_time = time;
                    v.source_path.clone()
                }
                _ => return,
            },
            Err(_) => return,
        };
        let data = canvas_render::image_data_from_rgba(image.rgba, image.width, image.height);
        self.images.insert(layer, data.clone());
        // El fondo comparte los fotogramas, pero conserva su cover y sus efectos.
        if let Some(bg) = self.background_layer.filter(|id| *id != layer) {
            if let Ok(l) = self.doc.layer_mut(bg) {
                if let LayerContent::Video(v) = &mut l.content {
                    if path.is_some() && v.source_path == path {
                        v.poster_time = time;
                        self.images.insert(bg, data);
                    }
                }
            }
        }
    }
}
