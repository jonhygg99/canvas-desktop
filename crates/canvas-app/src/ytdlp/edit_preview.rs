//! Caché de miniaturas: decodificación y blur fuera del hilo de egui.
//! Un trabajo pendiente y un fondo por índice evitan colas y acumulación
//! de texturas al arrastrar el trim o el radio de desenfoque.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{mpsc, Arc};

use eframe::egui;

use super::playback::live_textures;
use super::{contain_rect, fit_preview, grown_rect, quantize_playhead, VideoEdit};

#[derive(Default)]
pub(super) struct PreviewCache {
    images: HashMap<usize, Arc<canvas_io::LoadedImage>>,
    sharp: HashMap<usize, egui::TextureHandle>,
    backgrounds: HashMap<usize, (u8, egui::TextureHandle)>,
    pending: Option<mpsc::Receiver<Result<DecodedFrame, String>>>,
    last: Option<usize>,
    error: Option<String>,
}

struct DecodedFrame {
    index: usize,
    step: u8,
    image: Arc<canvas_io::LoadedImage>,
    background: Option<egui::ColorImage>,
}

pub(super) fn frame_index(edit: &VideoEdit) -> Option<usize> {
    frame_index_at(edit, edit.playhead)
}

/// Las miniaturas cubren el archivo entero, no solo el tramo recortado.
pub(super) fn frame_index_at(edit: &VideoEdit, time: f64) -> Option<usize> {
    if edit.frames.is_empty() {
        return None;
    }
    let i = (quantize_playhead(time, 0.0, edit.frame_fps) * edit.frame_fps).round() as usize;
    Some(i.min(edit.frames.len() - 1))
}

pub(super) fn frame_textures(
    edit: &mut VideoEdit,
    ctx: &egui::Context,
    idx: usize,
) -> Option<(egui::TextureId, Option<egui::TextureId>)> {
    edit.preview.poll(ctx);
    if let Some(error) = edit.preview.error.take() {
        edit.set_frames_error(error);
        edit.playing = false;
        edit.last_tick = None;
        crate::audio::pause_for(&edit.path);
    }
    let step = edit.blur.round().clamp(0.0, 100.0) as u8;
    let cached = edit.preview.sharp.contains_key(&idx);
    let background_ready = step == 0
        || edit
            .preview
            .backgrounds
            .get(&idx)
            .is_some_and(|(s, _)| *s == step);
    if (!cached || !background_ready)
        && edit.preview.pending.is_none()
        && edit.frames_error.is_none()
    {
        let path = edit.frames.get(idx)?.clone();
        edit.preview.request(idx, step, path, ctx);
    }
    if cached {
        edit.preview.last = Some(idx);
    }
    // Mantiene la última imagen mientras llega un scrub nuevo: sin flashes.
    let visible = edit.preview.last?;
    let sharp = edit.preview.sharp.get(&visible)?.id();
    let background = (step != 0)
        .then(|| edit.preview.backgrounds.get(&visible).map(|(_, t)| t.id()))
        .flatten();
    Some((sharp, background))
}

impl PreviewCache {
    fn request(&mut self, index: usize, step: u8, path: PathBuf, ctx: &egui::Context) {
        let image = self.images.get(&index).cloned();
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        let ctx = ctx.clone();
        let viewport = ctx.viewport_id();
        std::thread::spawn(move || {
            let result = image
                .map(Ok)
                .unwrap_or_else(|| canvas_io::load_image(&path).map(Arc::new))
                .map(|image| {
                    let background = (step != 0).then(|| {
                        let rgba = blurred_rgba(&image, f32::from(step));
                        egui::ColorImage::from_rgba_unmultiplied(
                            [image.width as usize, image.height as usize],
                            &rgba,
                        )
                    });
                    DecodedFrame {
                        index,
                        step,
                        image,
                        background,
                    }
                })
                .map_err(|e| format!("Could not load preview: {e}"));
            if tx.send(result).is_ok() {
                ctx.request_repaint_of(viewport);
            }
        });
    }

    fn poll(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.pending else { return };
        let frame = match rx.try_recv() {
            Ok(frame) => frame,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("Preview worker stopped. Retry preview.".to_owned())
            }
        };
        self.pending = None;
        let frame = match frame {
            Ok(frame) => frame,
            Err(error) => {
                self.error = Some(error);
                return;
            }
        };
        self.sharp.entry(frame.index).or_insert_with(|| {
            ctx.load_texture(
                format!("video-frame-{}", frame.index),
                egui::ColorImage::from_rgba_unmultiplied(
                    [frame.image.width as usize, frame.image.height as usize],
                    &frame.image.rgba,
                ),
                egui::TextureOptions::LINEAR,
            )
        });
        self.images.insert(frame.index, frame.image);
        if let Some(background) = frame.background {
            let texture = ctx.load_texture(
                format!("video-bg-{}-{}", frame.index, frame.step),
                background,
                egui::TextureOptions::LINEAR,
            );
            self.backgrounds.insert(frame.index, (frame.step, texture));
        }
    }
}

fn blurred_rgba(img: &canvas_io::LoadedImage, blur: f32) -> Vec<u8> {
    let sigma = blur / 100.0 * 5.0;
    image::RgbaImage::from_raw(img.width, img.height, img.rgba.clone())
        .map(|buf| image::imageops::blur(&buf, sigma).into_raw())
        .unwrap_or_else(|| img.rgba.clone())
}

/// Vista previa con vídeo en contain y fondo; espera al worker sin bloquear.
pub(super) fn preview_ui(edit: &mut VideoEdit, ui: &mut egui::Ui) {
    let size = fit_preview(edit.size.0, edit.size.1);
    let (vw, vh) = edit.video_size.unwrap_or((16.0, 9.0));
    let was_playing = edit.playing;
    let tex = live_textures(edit, ui.ctx()).or_else(|| {
        if was_playing {
            return None;
        }
        let step = edit.blur.round().clamp(0.0, 100.0) as u8;
        edit.exact
            .textures(&edit.path, step, ui.ctx())
            .or_else(|| frame_index(edit).and_then(|i| frame_textures(edit, ui.ctx(), i)))
    });
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    // Fondo de página por defecto (blanco): la ventana no conoce el del doc.
    ui.painter().rect_filled(rect, 0.0, egui::Color32::WHITE);
    let Some((sharp, bg)) = tex else {
        let msg = edit.frames_error.as_deref().unwrap_or({
            if edit.loading_frames || edit.preview.pending.is_some() || was_playing {
                "Extracting preview…"
            } else {
                "No preview frames. Retry preview."
            }
        });
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            msg,
            egui::FontId::proportional(13.0),
            ui.visuals().weak_text_color(),
        );
        return;
    };
    if let Some(bg) = bg {
        ui.painter().image(
            bg,
            rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    }
    let (x, y, w, h) = contain_rect(size.x, size.y, vw, vh);
    // La caja CRECE con el zoom (como el Transform en el lienzo) y la caja
    // de preview la recorta; UV completo siempre.
    let (zx, zy, zw, zh) = grown_rect(x, y, w, h, edit.zoom);
    let sharp_rect = egui::Rect::from_min_size(
        egui::pos2(rect.left() + zx, rect.top() + zy),
        egui::vec2(zw, zh),
    );
    ui.painter().with_clip_rect(rect).image(
        sharp,
        sharp_rect,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );
}
