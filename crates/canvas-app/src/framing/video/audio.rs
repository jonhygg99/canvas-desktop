//! Audio de esta sesión: cerrar otro documento nunca pausa este sink.
use rodio::{Decoder, OutputStreamBuilder, Sink};
use std::{
    fs::File,
    path::{Path, PathBuf},
    sync::mpsc::{self, Sender},
    thread::{self, JoinHandle},
    time::Duration,
};

pub(super) struct PlaybackAudio {
    stop: Option<Sender<()>>,
    worker: Option<JoinHandle<()>>,
}
impl PlaybackAudio {
    pub fn start(path: &Path, time: f64) -> Result<Self, String> {
        let path = PathBuf::from(path);
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let (stop_tx, stop_rx) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("framing-preview-audio".into())
            .spawn(move || {
                let stream = match OutputStreamBuilder::open_default_stream() {
                    Ok(stream) => stream,
                    Err(error) => {
                        let _ = ready_tx.send(Err(error.to_string()));
                        return;
                    }
                };
                let sink = Sink::connect_new(stream.mixer());
                sink.pause();
                let decoder = match File::open(path)
                    .map_err(|error| error.to_string())
                    .and_then(|file| Decoder::try_from(file).map_err(|error| error.to_string()))
                {
                    Ok(decoder) => decoder,
                    Err(error) => {
                        let _ = ready_tx.send(Err(error));
                        return;
                    }
                };
                sink.append(decoder);
                if time > 0.0 {
                    if let Err(error) = sink.try_seek(Duration::from_secs_f64(time)) {
                        let _ = ready_tx.send(Err(error.to_string()));
                        return;
                    }
                }
                sink.play();
                if ready_tx.send(Ok(())).is_ok() {
                    let _ = stop_rx.recv();
                }
                sink.stop();
                drop(stream);
            })
            .map_err(|error| error.to_string())?;
        match ready_rx.recv().map_err(|error| error.to_string())? {
            Ok(()) => Ok(Self {
                stop: Some(stop_tx),
                worker: Some(worker),
            }),
            Err(error) => {
                let _ = worker.join();
                Err(error)
            }
        }
    }
}
impl Drop for PlaybackAudio {
    fn drop(&mut self) {
        drop(self.stop.take());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
