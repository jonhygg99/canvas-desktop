//! Un decoder por reproducción; mailbox de un frame y cancelación inmediata.
use eframe::egui;
use std::{
    path::PathBuf,
    sync::{mpsc, Arc, Mutex},
    time::{Duration, Instant},
};

pub(in crate::framing) struct Frame {
    pub time: f64,
    pub image: canvas_io::LoadedImage,
    pub background: egui::ColorImage,
}
type ResultFrame = Result<Option<Frame>, String>;
pub(super) struct Worker {
    frames: Arc<Mutex<Option<ResultFrame>>>,
    cancel: mpsc::Sender<()>,
    decoder: Arc<Mutex<Option<canvas_io::VideoStreamCancellation>>>,
}
impl Worker {
    pub fn start(
        path: PathBuf,
        time: f64,
        end: f64,
        size: (u32, u32),
        playing: bool,
        ctx: egui::Context,
    ) -> Self {
        let frames = Arc::new(Mutex::new(None));
        let decoder = Arc::new(Mutex::new(None));
        let (cancel, rx) = mpsc::channel();
        let mailbox = frames.clone();
        let handle = decoder.clone();
        let viewport = ctx.viewport_id();
        std::thread::spawn(move || {
            let publish = |frame| {
                *mailbox.lock().unwrap_or_else(|e| e.into_inner()) = Some(frame);
                ctx.request_repaint_of(viewport);
            };
            let mut stream = match canvas_io::VideoFrameStream::open(&path, time, size, 20) {
                Ok(stream) => stream,
                Err(error) => {
                    publish(Err(error.to_string()));
                    return;
                }
            };
            *handle.lock().unwrap_or_else(|e| e.into_inner()) = Some(stream.cancellation());
            let mut clock = None;
            for index in 0u64.. {
                if rx.try_recv() != Err(mpsc::TryRecvError::Empty) {
                    break;
                }
                let current = time + index as f64 / 20.0;
                if current >= end {
                    publish(Ok(None));
                    break;
                }
                let frame = stream
                    .next_frame()
                    .map_err(|e| e.to_string())
                    .and_then(|frame| {
                        frame
                            .map(|image| {
                                super::super::preview::background(&image).map(|background| Frame {
                                    time: current,
                                    image,
                                    background,
                                })
                            })
                            .transpose()
                    });
                let wait = Duration::from_secs_f64(index as f64 / 20.0)
                    .saturating_sub(clock.get_or_insert_with(Instant::now).elapsed());
                if rx.recv_timeout(wait) != Err(mpsc::RecvTimeoutError::Timeout) {
                    break;
                }
                let done = !matches!(frame, Ok(Some(_)));
                publish(frame);
                if done || !playing {
                    break;
                }
            }
        });
        Self {
            frames,
            cancel,
            decoder,
        }
    }
    pub fn take(&self) -> Option<ResultFrame> {
        self.frames.lock().unwrap_or_else(|e| e.into_inner()).take()
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.cancel.send(());
        if let Some(decoder) = self
            .decoder
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
            decoder.cancel();
        }
    }
}
