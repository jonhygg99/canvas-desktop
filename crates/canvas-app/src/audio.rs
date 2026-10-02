//! Audio para video: rodio + symphonia (aac/mp3 en mp4).
#![allow(dead_code)]

use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::Duration;

use std::sync::{Mutex, OnceLock};

use rodio::{Decoder, OutputStreamBuilder, Sink};

pub struct AudioState {
    mixer: rodio::mixer::Mixer,
    sink: Option<Sink>,
    current_path: Option<PathBuf>,
}

impl AudioState {
    pub fn new() -> Option<Self> {
        let stream = OutputStreamBuilder::open_default_stream().ok()?;
        let mixer = stream.mixer().clone();
        std::mem::forget(stream);
        Some(Self {
            mixer,
            sink: None,
            current_path: None,
        })
    }

    fn ensure_sink(&mut self) -> &mut Sink {
        if self.sink.is_none() {
            self.sink = Some(Sink::connect_new(&self.mixer));
        }
        self.sink.as_mut().expect("sink creado")
    }

    pub fn play(&mut self, path: &Path, seek_secs: f64) -> Result<(), String> {
        let need_new_sink = self.current_path.as_deref().is_some_and(|p| p != path)
            || self.sink.is_none()
            || self
                .sink
                .as_ref()
                .is_some_and(|s| s.empty() || s.is_paused() && seek_secs != 0.0);

        if need_new_sink {
            self.stop();
            let sink = Sink::connect_new(&self.mixer);
            // Intentar abrir y decodificar: rodio + symphonia soporta mp4/aac/mp3/flac
            let file = File::open(path).map_err(|e| e.to_string())?;
            // File aporta longitud y seek reales; necesarios para MP4 y saltos.
            let source = Decoder::try_from(file).map_err(|e| e.to_string())?;
            // Seek si es necesario
            if seek_secs > 0.1 {
                // No hay seek directo en Decoder, usamos try_seek en Sink tras append
                sink.append(source);
                let _ = sink.try_seek(Duration::from_secs_f64(seek_secs.max(0.0)));
            } else {
                sink.append(source);
            }
            sink.play();
            self.sink = Some(sink);
            self.current_path = Some(path.to_owned());
        } else if let Some(s) = &self.sink {
            if s.is_paused() {
                let _ = s.try_seek(Duration::from_secs_f64(seek_secs.max(0.0)));
                s.play();
            } else if (seek_secs - self.current_time().unwrap_or(0.0)).abs() > 0.5 {
                let _ = s.try_seek(Duration::from_secs_f64(seek_secs.max(0.0)));
            }
        }
        Ok(())
    }

    pub fn pause(&mut self) {
        if let Some(s) = &self.sink {
            s.pause();
        }
    }

    pub fn resume(&mut self) {
        if let Some(s) = &self.sink {
            s.play();
        }
    }

    pub fn stop(&mut self) {
        if let Some(s) = self.sink.take() {
            s.stop();
        }
        self.current_path = None;
    }

    pub fn seek(&mut self, secs: f64) {
        if let Some(s) = &self.sink {
            let _ = s.try_seek(Duration::from_secs_f64(secs.max(0.0)));
        }
    }

    pub fn current_time(&self) -> Option<f64> {
        // rodio no expone tiempo actual preciso sin Source queue; aproximamos via is_paused etc.
        None
    }

    pub fn is_playing(&self) -> bool {
        self.sink
            .as_ref()
            .is_some_and(|s| !s.is_paused() && !s.empty())
    }
}

static GLOBAL: OnceLock<Mutex<AudioState>> = OnceLock::new();

fn global_state() -> Option<std::sync::MutexGuard<'static, AudioState>> {
    GLOBAL.get()?.lock().ok()
}

fn ensure_global() -> Option<std::sync::MutexGuard<'static, AudioState>> {
    if GLOBAL.get().is_none() {
        let state = AudioState::new()?;
        let _ = GLOBAL.set(Mutex::new(state));
    }
    global_state()
}

pub fn play(path: &Path, seek_secs: f64) -> Result<(), String> {
    let Some(mut g) = ensure_global() else {
        return Err("no audio device".to_owned());
    };
    g.play(path, seek_secs)
}

pub fn pause() {
    if let Some(mut g) = global_state() {
        g.pause();
    }
}

/// Al cerrar un lienzo solo se pausa el audio que pertenece a su vídeo.
pub fn pause_for(path: &Path) {
    if let Some(mut g) = global_state() {
        if g.current_path.as_deref() == Some(path) {
            g.pause();
        }
    }
}

pub fn stop() {
    if let Some(mut g) = global_state() {
        g.stop();
    }
}

pub fn seek(secs: f64) {
    if let Some(mut g) = global_state() {
        g.seek(secs);
    }
}

pub fn is_playing() -> bool {
    global_state().is_some_and(|g| g.is_playing())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_aac_audio_from_an_mp4_without_an_audio_device() {
        let Some(ffmpeg) = canvas_io::ffmpeg_path() else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audio.mp4");
        assert!(canvas_io::media_command(ffmpeg)
            .args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc=size=64x48:rate=20",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:sample_rate=44100",
                "-t",
                "0.25",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac"
            ])
            .arg(&path)
            .output()
            .unwrap()
            .status
            .success());
        use rodio::Source;
        let mut decoder = Decoder::try_from(File::open(path).unwrap()).unwrap();
        decoder.try_seek(Duration::from_secs_f64(0.1)).unwrap();
        assert!(decoder.take(4096).any(|sample| sample != 0.0));
    }
}
