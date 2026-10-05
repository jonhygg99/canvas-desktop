//! Reproducción temporal del medio individual; no cambia el vídeo original.
mod audio;
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
    pub trim: canvas_io::VideoTrim,
    pub saved_trim: canvas_io::VideoTrim,
    pub muted: bool,
    audio: Option<audio::PlaybackAudio>,
    audio_attempted: bool,
    pub audio_error: Option<String>,
    pub finished: bool,
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
            trim: canvas_io::VideoTrim {
                start_s: 0.0,
                end_s: duration,
            },
            saved_trim: canvas_io::VideoTrim {
                start_s: 0.0,
                end_s: duration,
            },
            muted: true,
            audio: None,
            audio_attempted: false,
            audio_error: None,
            finished: false,
        }
    }
    pub fn seek(&mut self, time: f64, playing: bool, ctx: &egui::Context) {
        self.pause();
        let time = if time.is_finite() {
            time
        } else {
            self.trim.start_s
        };
        self.position = time.clamp(
            self.trim.start_s,
            (self.trim.end_s - 0.05).max(self.trim.start_s),
        );
        self.finished = false;
        self.audio_attempted = false;
        self.playing = playing;
        self.worker = Some(decoder::Worker::start(
            self.path.clone(),
            self.position,
            self.trim.end_s,
            self.size,
            playing,
            ctx.clone(),
        ));
    }
    pub fn pause(&mut self) {
        self.playing = false;
        self.worker = None;
        self.audio = None;
    }
    pub fn poll(&mut self, ctx: &egui::Context) -> Option<Result<decoder::Frame, String>> {
        let frame = self.worker.as_ref()?.take()?;
        match frame {
            Ok(Some(frame)) => {
                self.position = frame.time;
                if self.playing && !self.muted && !self.audio_attempted {
                    self.audio_attempted = true;
                    match audio::PlaybackAudio::start(&self.path, frame.time) {
                        Ok(audio) => {
                            self.audio = Some(audio);
                            self.audio_error = None;
                        }
                        Err(error) => self.audio_error = Some(error),
                    }
                }
                Some(Ok(frame))
            }
            Ok(None) => {
                if self.playing {
                    self.seek(self.trim.start_s, true, ctx);
                } else {
                    self.pause();
                    self.finished = true;
                }
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

impl Video {
    pub fn apply_trim(&mut self, trim: canvas_io::VideoTrim) -> Result<(), String> {
        if !trim.is_valid() || trim.end_s > self.duration {
            return Err("Trim must stay within the original video duration".into());
        }
        self.pause();
        self.trim = trim;
        self.position = self
            .position
            .clamp(trim.start_s, (trim.end_s - 0.05).max(trim.start_s));
        self.finished = false;
        Ok(())
    }
    pub fn set_muted(&mut self, muted: bool, ctx: &egui::Context) {
        self.muted = muted;
        if muted {
            self.audio = None;
        } else if self.playing {
            self.seek(self.position, true, ctx);
        }
    }
}
