//! Audio para video: rodio + symphonia (aac/mp3 en mp4).
#![allow(dead_code)]

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::time::Duration;

use std::sync::{Mutex, OnceLock};

use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink};

pub struct AudioState {
    handle: OutputStreamHandle,
    sink: Option<Sink>,
    current_path: Option<PathBuf>,
}

impl AudioState {
    pub fn new() -> Option<Self> {
        let (stream, handle) = OutputStream::try_default().ok()?;
        std::mem::forget(stream);
        Some(Self {
            handle,
            sink: None,
            current_path: None,
        })
    }

    fn ensure_sink(&mut self) -> &mut Sink {
        if self.sink.is_none() {
            if let Ok(s) = Sink::try_new(&self.handle) {
                self.sink = Some(s);
            }
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
            let sink = Sink::try_new(&self.handle).map_err(|e| e.to_string())?;
            // Intentar abrir y decodificar: rodio + symphonia soporta mp4/aac/mp3/flac
            let file = File::open(path).map_err(|e| e.to_string())?;
            let reader = BufReader::new(file);
            let source = Decoder::new(reader).map_err(|e| e.to_string())?;
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
