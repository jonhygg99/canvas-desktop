//! Preview continua a 30 fps con un FFmpeg por reproducción. El worker
//! conserva solo el último fotograma y egui actualiza dos texturas estables.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui;

use super::VideoEdit;

pub(super) const PLAYBACK_FPS: u32 = 30;
type Textures = (egui::TextureId, Option<egui::TextureId>);

#[derive(Default)]
pub(super) struct LivePreview {
    worker: Option<StreamWorker>,
    sharp: Option<egui::TextureHandle>,
    background: Option<egui::TextureHandle>,
    shown_time: Option<f64>,
    audio_pending: bool,
    pixels: Option<Arc<canvas_io::LoadedImage>>,
    background_step: u8,
    pending_background: Option<mpsc::Receiver<StillBackground>>,
}

struct StreamWorker {
    mailbox: Arc<Mutex<Mailbox>>,
    cancel: mpsc::Sender<()>,
    blur: Arc<AtomicU8>,
    decoder: Arc<Mutex<Option<canvas_io::VideoStreamCancellation>>>,
}

#[derive(Default)]
struct Mailbox {
    frame: Option<StreamFrame>,
    finished: bool,
    error: Option<String>,
}

struct StreamFrame {
    time: f64,
    sharp: egui::ColorImage,
    background: Option<egui::ColorImage>,
    pixels: Arc<canvas_io::LoadedImage>,
    step: u8,
}

struct StillBackground {
    step: u8,
    image: egui::ColorImage,
}

struct StreamRequest {
    path: PathBuf,
    start: f64,
    end: f64,
    size: (u32, u32),
}

pub(super) fn live_textures(edit: &mut VideoEdit, ctx: &egui::Context) -> Option<Textures> {
    let step = edit.blur.round().clamp(0.0, 100.0) as u8;
    if !edit.playing {
        edit.playback.stop();
        if edit
            .playback
            .shown_time
            .is_some_and(|time| (time - edit.playhead).abs() < 1e-6)
        {
            edit.playback.update_still_background(step, ctx);
            return edit.playback.textures();
        }
        return None;
    }
    if !edit.playback.is_running() {
        edit.playback.pending_background = None;
        let request = StreamRequest {
            path: edit.path.clone(),
            start: edit.playhead,
            end: edit.trim_end,
            size: preview_dimensions(edit.video_size.unwrap_or((16.0, 9.0))),
        };
        edit.playback.worker = Some(StreamWorker::start(request, step, ctx.clone()));
        edit.playback.audio_pending = true;
    }
    let worker = edit.playback.worker.as_ref()?;
    worker.blur.store(step, Ordering::Relaxed);
    let (frame, finished, error) = {
        let mut pending = worker.mailbox.lock().ok()?;
        (pending.frame.take(), pending.finished, pending.error.take())
    };
    if let Some(frame) = frame {
        edit.playhead = frame.time;
        edit.last_tick = Some(Instant::now());
        edit.playback.upload(frame, ctx);
        if edit.playback.audio_pending {
            edit.playback.audio_pending = false;
            if !edit.mute {
                let _ = crate::audio::play(&edit.path, edit.playhead);
            }
        }
    }
    if finished || error.is_some() {
        let looping = error.is_none() && edit.loop_selection;
        edit.playing = looping;
        edit.last_tick = None;
        edit.playback.stop();
        crate::audio::pause_for(&edit.path);
        if let Some(error) = error {
            edit.set_frames_error(error);
        } else if looping {
            edit.playhead = edit.trim_start;
            edit.last_tick = Some(Instant::now());
            ctx.request_repaint();
        } else {
            edit.playhead = edit.trim_end;
            edit.playback.shown_time = Some(edit.playhead);
        }
    }
    edit.playback.textures()
}

impl LivePreview {
    pub(super) fn is_running(&self) -> bool {
        self.worker.is_some()
    }

    pub(super) fn stop(&mut self) {
        self.worker = None;
        self.audio_pending = false;
    }

    fn textures(&self) -> Option<Textures> {
        Some((
            self.sharp.as_ref()?.id(),
            self.background.as_ref().map(|t| t.id()),
        ))
    }

    fn upload(&mut self, frame: StreamFrame, ctx: &egui::Context) {
        update_texture(&mut self.sharp, frame.sharp, "video-live", ctx);
        if let Some(background) = frame.background {
            update_texture(&mut self.background, background, "video-live-bg", ctx);
        } else {
            self.background = None;
        }
        self.shown_time = Some(frame.time);
        self.pixels = Some(frame.pixels);
        self.background_step = frame.step;
    }

    fn update_still_background(&mut self, step: u8, ctx: &egui::Context) {
        if step == 0 {
            self.background = None;
            self.background_step = 0;
        }
        if let Some(rx) = &self.pending_background {
            match rx.try_recv() {
                Ok(result) => {
                    self.pending_background = None;
                    if result.step == step {
                        update_texture(&mut self.background, result.image, "video-live-bg", ctx);
                        self.background_step = step;
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => self.pending_background = None,
                Err(mpsc::TryRecvError::Empty) => return,
            }
        }
        if step == self.background_step {
            return;
        }
        let Some(pixels) = self.pixels.clone() else {
            return;
        };
        let (tx, rx) = mpsc::channel();
        self.pending_background = Some(rx);
        let ctx = ctx.clone();
        let viewport = ctx.viewport_id();
        // Cambiar blur en Pause mantiene el fotograma exacto sin bloquear UI.
        std::thread::spawn(move || {
            let image = prepare_background(&pixels, step);
            if tx.send(StillBackground { step, image }).is_ok() {
                ctx.request_repaint_of(viewport);
            }
        });
    }
}

impl StreamWorker {
    fn start(request: StreamRequest, step: u8, ctx: egui::Context) -> Self {
        let mailbox = Arc::new(Mutex::new(Mailbox::default()));
        let frames = mailbox.clone();
        let blur = Arc::new(AtomicU8::new(step));
        let blur_step = blur.clone();
        let (cancel, cancelled) = mpsc::channel();
        let decoder = Arc::new(Mutex::new(None));
        let decoder_handle = decoder.clone();
        let viewport = ctx.viewport_id();
        std::thread::spawn(move || {
            let publish = |frame| {
                if let Ok(mut pending) = frames.lock() {
                    pending.frame = Some(frame);
                }
                ctx.request_repaint_of(viewport);
            };
            let result = run_stream(request, &cancelled, &blur_step, &decoder_handle, publish);
            if let Ok(mut pending) = frames.lock() {
                // EOF no reemplaza el último fotograma todavía sin pintar.
                pending.finished = true;
                pending.error = result.err();
            }
            ctx.request_repaint_of(viewport);
        });
        Self {
            mailbox,
            cancel,
            blur,
            decoder,
        }
    }
}

impl Drop for StreamWorker {
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

fn run_stream(
    request: StreamRequest,
    cancelled: &mpsc::Receiver<()>,
    blur: &AtomicU8,
    decoder: &Mutex<Option<canvas_io::VideoStreamCancellation>>,
    publish: impl Fn(StreamFrame),
) -> Result<(), String> {
    let mut stream =
        canvas_io::VideoFrameStream::open(&request.path, request.start, request.size, PLAYBACK_FPS)
            .map_err(|e| e.to_string())?;
    *decoder.lock().unwrap_or_else(|e| e.into_inner()) = Some(stream.cancellation());
    let interval = 1.0 / f64::from(PLAYBACK_FPS);
    let mut clock = None;
    let mut index = 0u64;
    loop {
        if cancelled.try_recv() != Err(mpsc::TryRecvError::Empty) {
            return Ok(());
        }
        let offset = index as f64 * interval;
        let time = request.start + offset;
        if time >= request.end {
            if let Some(clock) = clock {
                wait_until(cancelled, clock, (request.end - request.start).max(0.0));
            }
            return Ok(());
        }
        let Some(image) = stream.next_frame().map_err(|e| e.to_string())? else {
            return Ok(());
        };
        let started = *clock.get_or_insert_with(Instant::now);
        index += 1;
        // Si un decode se atrasó, recupera el reloj sin encolar imágenes viejas.
        if started.elapsed().as_secs_f64() > offset + interval * 2.0 {
            continue;
        }
        let frame = prepare_frame(image, time, blur.load(Ordering::Relaxed));
        if !wait_until(cancelled, started, offset) {
            return Ok(());
        }
        publish(frame);
    }
}

fn wait_until(cancelled: &mpsc::Receiver<()>, clock: Instant, offset: f64) -> bool {
    let wait = Duration::from_secs_f64(offset).saturating_sub(clock.elapsed());
    cancelled.recv_timeout(wait) == Err(mpsc::RecvTimeoutError::Timeout)
}

fn prepare_frame(image: canvas_io::LoadedImage, time: f64, step: u8) -> StreamFrame {
    let sharp = egui::ColorImage::from_rgba_unmultiplied(
        [image.width as usize, image.height as usize],
        &image.rgba,
    );
    let background = (step != 0).then(|| prepare_background(&image, step));
    StreamFrame {
        time,
        sharp,
        background,
        pixels: Arc::new(image),
        step,
    }
}

fn prepare_background(image: &canvas_io::LoadedImage, step: u8) -> egui::ColorImage {
    let pixels = image::RgbaImage::from_raw(image.width, image.height, image.rgba.clone())
        .expect("frame RGBA completo");
    // El fondo desenfocado necesita pocos píxeles: nunca frena el foreground.
    let small = image::imageops::thumbnail(&pixels, 160, 160);
    let sigma = f32::from(step) / 100.0 * 5.0 * small.width() as f32 / 320.0;
    let blurred = image::imageops::blur(&small, sigma);
    egui::ColorImage::from_rgba_unmultiplied(
        [blurred.width() as usize, blurred.height() as usize],
        blurred.as_raw(),
    )
}

fn update_texture(
    texture: &mut Option<egui::TextureHandle>,
    image: egui::ColorImage,
    name: &str,
    ctx: &egui::Context,
) {
    if let Some(texture) = texture {
        texture.set(image, egui::TextureOptions::LINEAR);
    } else {
        *texture = Some(ctx.load_texture(name, image, egui::TextureOptions::LINEAR));
    }
}

fn preview_dimensions((width, height): (f64, f64)) -> (u32, u32) {
    let scale = (480.0 / width.max(height).max(1.0)).min(1.0);
    (
        (width * scale).round().max(1.0) as u32,
        (height * scale).round().max(1.0) as u32,
    )
}
