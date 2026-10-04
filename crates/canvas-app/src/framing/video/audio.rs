//! Audio de esta sesión: cerrar otro documento nunca pausa este sink.
use rodio::{Decoder, OutputStream, OutputStreamBuilder, Sink};
use std::{fs::File, path::Path, time::Duration};

pub(super) struct PlaybackAudio {
    // Mantiene vivo el dispositivo durante la reproducción del sink.
    _stream: OutputStream,
    sink: Sink,
}
impl PlaybackAudio {
    pub fn start(path: &Path, time: f64) -> Result<Self, String> {
        let stream = OutputStreamBuilder::open_default_stream().map_err(|e| e.to_string())?;
        let sink = Sink::connect_new(stream.mixer());
        sink.pause();
        let decoder = Decoder::try_from(File::open(path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        sink.append(decoder);
        if time > 0.0 {
            sink.try_seek(Duration::from_secs_f64(time))
                .map_err(|e| e.to_string())?;
        }
        sink.play();
        Ok(Self {
            _stream: stream,
            sink,
        })
    }
}
impl Drop for PlaybackAudio {
    fn drop(&mut self) {
        self.sink.stop();
    }
}
