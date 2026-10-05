//! Ventana «Editar vídeo» del Download: trim, background blur y zoom sobre
//! una vista previa de fotogramas, con play/pause/restart y timeline
//! clicable. La vista previa y el lienzo que crea Aceptar comparten tamaño
//! (cover a página completa).
//!
//! El play decodifica en continuo a 30 fps, con audio opcional y memoria
//! limitada. Las miniaturas (`preview_fps`, tope 120) sirven para scrub y
//! póster; no fijan la cadencia de reproducción.
//!
//! Aceptar no toca el documento actual: devuelve `VideoAccept` y cierra; el
//! llamador crea el lienzo nuevo (con guard de cambios sin guardar).
//! Cancelar/X deja todo intacto.

use std::path::PathBuf;
use std::sync::mpsc::Sender;

use canvas_core::{Document, LayerContent, LayerId, Transform};
use eframe::egui;

use super::api::clamp_trim;
use super::state::Panel;
use crate::loader::{self, AppMsg};
use crate::settings::AppSettings;

#[cfg(test)]
#[path = "edit_tests.rs"]
mod regression_tests;

#[cfg(test)]
#[path = "edit_stream_tests.rs"]
mod stream_tests;

#[path = "edit_preview.rs"]
mod preview;
#[cfg(test)]
use preview::{frame_index, frame_textures};
use preview::{frame_index_at, preview_ui, PreviewCache};

#[path = "edit_controls.rs"]
mod controls;
#[path = "timeline.rs"]
mod timeline;
#[path = "timeline_thumbnails.rs"]
mod timeline_thumbnails;
#[path = "timeline_view.rs"]
mod timeline_view;
#[path = "trim.rs"]
mod trim;
#[path = "trim_controls.rs"]
mod trim_controls;
use trim::{timecode, TrimEdge, TrimHistory};
#[path = "timeline_tests.rs"]
#[cfg(test)]
mod timeline_tests;
#[path = "trim_tests.rs"]
#[cfg(test)]
mod trim_tests;
use controls::{advance_playhead, params_ui, timeline_ui, transport_ui};

#[path = "edit_playback.rs"]
mod playback;
use playback::LivePreview;

/// Tamaños de lienzo ofrecidos (nombre, ancho, alto).
pub const CANVAS_SIZES: [(&str, f64, f64); 3] = [
    ("Full HD 1920 × 1080", 1920.0, 1080.0),
    ("Vertical 1080 × 1920", 1080.0, 1920.0),
    ("4K 3840 × 2160", 3840.0, 2160.0),
];

/// Lo que Aceptar pide crear (el llamador abre el lienzo nuevo).
#[derive(Clone, Debug)]
pub struct VideoAccept {
    pub path: PathBuf,
    pub title: String,
    pub size: (f64, f64),
    /// Dims reales del vídeo (para el contain del builder).
    pub video_size: Option<(f64, f64)>,
    pub trim_start: f64,
    pub trim_end: Option<f64>,
    pub blur_radius: f32,
    pub zoom: f32,
    pub poster: PathBuf,
}

/// Sesión de edición de un clip (vive en `Panel::edit`).
pub struct VideoEdit {
    clip_id: String,
    title: String,
    path: PathBuf,
    pub(crate) duration: Option<f64>,
    pub(crate) trim_start: f64,
    pub(crate) trim_end: f64,
    pub(crate) blur: f32,
    pub(crate) zoom: f32,
    pub(crate) size: (f64, f64),
    pub(crate) video_size: Option<(f64, f64)>,
    playing: bool,
    playhead: f64,
    source_fps: f64,
    trim_history: TrimHistory,
    timeline_range: Option<(f64, f64)>,
    thumbnails: timeline_thumbnails::Thumbnails,
    timeline_gesture: Option<timeline::Gesture>,
    loop_selection: bool,
    /// Pide re-extraer los fotogramas (lo sirve `layers_panel`).
    pub(crate) retry_frames: bool,
    /// Vista previa muda por defecto (se puede quitar).
    pub(crate) mute: bool,
    last_tick: Option<std::time::Instant>,
    frames: Vec<PathBuf>,
    frame_fps: f64,
    loading_frames: bool,
    frames_error: Option<String>,
    preview: PreviewCache,
    playback: LivePreview,
}

impl VideoEdit {
    /// Abre la sesión: restaura sliders desde su capa si el clip ya está
    /// insertado; tamaño inicial, el recordado (defecto Full HD).
    pub(crate) fn open(
        file_name: &str,
        title: String,
        path: PathBuf,
        duration: Option<f64>,
        size: (f64, f64),
        doc: &Document,
    ) -> Self {
        let target_layer = layer_for_clip(doc, file_name);
        let mut edit = Self {
            clip_id: file_name.to_owned(),
            title,
            path,
            duration,
            trim_start: 0.0,
            trim_end: duration.unwrap_or(0.0),
            // Fondo con blur 50 por defecto (receta imágenes); 0 = sin fondo.
            blur: 100.0,
            zoom: 1.0,
            size,
            video_size: None,
            playing: false,
            playhead: 0.0,
            source_fps: 30.0,
            trim_history: TrimHistory::default(),
            timeline_range: None,
            thumbnails: timeline_thumbnails::Thumbnails::default(),
            timeline_gesture: None,
            loop_selection: false,
            retry_frames: false,
            mute: true,
            last_tick: None,
            frames: Vec::new(),
            frame_fps: 1.0,
            loading_frames: true,
            frames_error: None,
            preview: PreviewCache::default(),
            playback: LivePreview::default(),
        };
        if let Some(id) = target_layer {
            edit.restore_from_layer(doc, id);
        }
        edit
    }

    /// Restaura sliders desde la capa (blur del fondo, zoom contra la base
    /// contain con las dims naturales del vídeo, trim del enlace).
    fn restore_from_layer(&mut self, doc: &Document, id: LayerId) {
        let Ok(layer) = doc.layer(id) else {
            return;
        };
        let Ok(page) = doc.page() else {
            return;
        };
        self.blur = slider_for_radius(bg_blur_radius(doc));
        if let LayerContent::Video(link) = &layer.content {
            self.zoom = zoom_for_transform(
                layer.transform.width,
                f64::from(link.natural_width),
                f64::from(link.natural_height),
                page.width,
                page.height,
            );
            self.trim_start = link.trim_start.max(0.0);
            if let Some(end) = link.trim_end {
                self.trim_end = end;
            }
        }
        self.playhead = self.trim_start;
    }

    /// ¿Es la sesión de este clip? (los mensajes tardíos de otra sesión se
    /// ignoran: la ventana pudo cerrarse y reabrirse).
    pub(crate) fn matches(&self, clip_id: &str) -> bool {
        self.clip_id == clip_id
    }

    /// Fotogramas listos: fija dims/duración/trim final y coloca el playhead.
    pub(crate) fn set_frames(
        &mut self,
        files: Vec<PathBuf>,
        fps: f64,
        duration: f64,
        video_size: Option<(f64, f64)>,
    ) {
        self.frames = files;
        self.frame_fps = fps;
        self.duration = Some(duration);
        self.video_size = video_size;
        let end = if self.trim_end > self.trim_start {
            self.trim_end
        } else {
            duration
        };
        (self.trim_start, self.trim_end) = clamp_trim(self.trim_start, end, duration);
        self.playhead = self.trim_start;
        self.preview = PreviewCache::default();
        self.thumbnails = timeline_thumbnails::Thumbnails::default();
        self.playback = LivePreview::default();
        self.loading_frames = false;
        self.frames_error = None;
    }

    /// La vista previa falló: queda la guía visible (el trim espera).
    pub(crate) fn set_frames_error(&mut self, error: String) {
        self.loading_frames = false;
        self.frames_error = Some(error);
    }
}

impl Drop for VideoEdit {
    fn drop(&mut self) {
        self.playback.stop();
        if self.playing {
            crate::audio::pause_for(&self.path);
        }
    }
}

/// Blur del fondo desenfocado (capa «Blurred background» al fondo, receta
/// imágenes): 0 si no hay.
fn bg_blur_radius(doc: &Document) -> f32 {
    doc.page()
        .ok()
        .and_then(|page| page.layers.first())
        .filter(|l| l.name == "Blurred background")
        .map(|l| l.effects.blur_radius)
        .unwrap_or(0.0)
}

/// Título legible desde el nombre de fichero (`clip-Ana_X.mp4` → `clip Ana X`).
fn clip_title(file_name: &str) -> String {
    file_name
        .rsplit_once('.')
        .map(|(s, _)| s.replace(['_', '-'], " "))
        .unwrap_or_else(|| file_name.to_owned())
}

/// Aceptar neutro para Insertar: trim completo, blur 50 (receta imágenes),
/// zoom 1, tamaño recordado. El builder extrae el póster (sin worker previo).
pub(crate) fn default_accept(path: PathBuf, title: String, size: (f64, f64)) -> VideoAccept {
    VideoAccept {
        path,
        title,
        size,
        video_size: None,
        trim_start: 0.0,
        trim_end: None,
        blur_radius: 50.0,
        zoom: 1.0,
        poster: PathBuf::new(),
    }
}

/// Capa de vídeo del proyecto para un clip (por nombre de fichero): la más
/// alta que lo referencie.
pub(crate) fn layer_for_clip(doc: &Document, file_name: &str) -> Option<LayerId> {
    let Ok(page) = doc.page() else {
        return None;
    };
    page.layers.iter().rev().find_map(|layer| {
        let LayerContent::Video(link) = &layer.content else {
            return None;
        };
        let name = link
            .source_path
            .as_deref()?
            .file_name()
            .and_then(|s| s.to_str())?;
        (name == file_name).then_some(layer.id)
    })
}

/// Abre el `pending_edit` del panel y lanza la extracción de fotogramas.
pub fn open_pending_edit(
    video: &mut Panel,
    doc: &Document,
    size: (f64, f64),
    tx: &Sender<AppMsg>,
    ctx: &egui::Context,
) {
    let Some(path) = video.pending_edit.take() else {
        return;
    };
    let file_name = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let title = clip_title(&file_name);
    // La duración la sondea el worker (ffprobe); hasta entonces el trim
    // espera. El tamaño, el recordado en ajustes.
    video.edit = Some(VideoEdit::open(
        &file_name,
        title,
        path.clone(),
        None,
        size,
        doc,
    ));
    loader::spawn_ytdlp_frames(file_name, path, None, tx.clone(), ctx.clone());
}

/// Ventana de edición. Devuelve el `VideoAccept` si se pulsó Aceptar (el
/// llamador crea el lienzo); cerrar/X/Cancelar devuelve `None`.
pub fn edit_window_ui(
    video: &mut Panel,
    settings: &mut AppSettings,
    ui: &mut egui::Ui,
) -> Option<VideoAccept> {
    let title = format!("Edit video — {}", video.edit.as_ref()?.title);
    let mut open = true;
    let mut close = false;
    let mut accept = None;
    egui::Window::new(title)
        .open(&mut open)
        .resizable(true)
        .default_size(egui::vec2(430.0, 600.0))
        .show(ui.ctx(), |ui| {
            let edit = video.edit.as_mut().expect("checked above");
            advance_playhead(edit, &ui.ctx().clone());
            preview_ui(edit, ui);
            transport_ui(edit, ui);
            timeline_ui(edit, ui);
            params_ui(edit, ui, settings);
            ui.separator();
            ui.horizontal(|ui| {
                let ready =
                    !edit.loading_frames && edit.frames_error.is_none() && !edit.frames.is_empty();
                let btn = ui.add_enabled(ready, egui::Button::new(crate::i18n::tr("Aceptar")));
                if btn.clicked() {
                    accept = Some(build_accept(edit));
                }
                if !ready {
                    btn.on_hover_text(crate::i18n::tr("Waiting for preview frames"));
                }
                if ui.button(crate::i18n::tr("Atrás")).clicked() {
                    close = true;
                }
            });
        });
    if !open || close {
        stop_preview_audio(video);
        video.edit = None;
        return None;
    }
    if accept.is_some() {
        stop_preview_audio(video);
        video.edit = None;
    }
    accept
}

/// Corta el audio de la preview al cerrar/aceptar (el lienzo no autoplayea).
fn stop_preview_audio(video: &Panel) {
    if let Some(edit) = video.edit.as_ref() {
        crate::audio::pause_for(&edit.path);
    }
}

/// Construye el accept (siempre con algo válido: el botón lo exige).
fn build_accept(edit: &VideoEdit) -> VideoAccept {
    VideoAccept {
        path: edit.path.clone(),
        title: edit.title.clone(),
        size: edit.size,
        video_size: edit.video_size,
        trim_start: edit.trim_start,
        trim_end: Some(edit.trim_end),
        blur_radius: blur_radius_for_slider(edit.blur),
        zoom: edit.zoom,
        poster: frame_index_at(edit, edit.trim_start)
            .and_then(|i| edit.frames.get(i))
            .cloned()
            .unwrap_or_default(),
    }
}

/// Fija el playhead al fotograma anterior o igual (rejilla `fps`).
pub(crate) fn quantize_playhead(playhead: f64, trim_start: f64, fps: f64) -> f64 {
    if fps <= 0.0 {
        return playhead;
    }
    let k = ((playhead - trim_start) * fps).floor().max(0.0);
    trim_start + k / fps
}

/// Rect contain (x, y, w, h) del vídeo dentro de la caja: crece solo hasta
/// que un lado toca el borde, proporción intacta (como al pegar imágenes).
pub(crate) fn contain_rect(box_w: f32, box_h: f32, vw: f64, vh: f64) -> (f32, f32, f32, f32) {
    if vw <= 0.0 || vh <= 0.0 {
        return (0.0, 0.0, box_w, box_h);
    }
    let scale = (f64::from(box_w) / vw).min(f64::from(box_h) / vh) as f32;
    let (w, h) = (vw as f32 * scale, vh as f32 * scale);
    ((box_w - w) / 2.0, (box_h - h) / 2.0, w, h)
}

/// Rect contain crecido por el zoom alrededor de su centro (como el
/// Transform en el lienzo): la caja de preview lo recorta.
pub(crate) fn grown_rect(x: f32, y: f32, w: f32, h: f32, zoom: f32) -> (f32, f32, f32, f32) {
    let z = zoom.max(1.0);
    let (nw, nh) = (w * z, h * z);
    (x + (w - nw) / 2.0, y + (h - nh) / 2.0, nw, nh)
}

/// Reintento pedido desde la ventana (lo sirve `layers_panel`, que tiene
/// canal y contexto).
pub fn retry_pending_frames(video: &mut Panel, tx: &Sender<AppMsg>, ctx: &egui::Context) {
    let Some(edit) = video.edit.as_mut() else {
        return;
    };
    if !edit.retry_frames {
        return;
    }
    edit.retry_frames = false;
    edit.loading_frames = true;
    edit.frames_error = None;
    edit.playing = false;
    edit.last_tick = None;
    edit.frames.clear();
    edit.preview = PreviewCache::default();
    edit.playback = LivePreview::default();
    crate::audio::pause_for(&edit.path);
    loader::spawn_ytdlp_frames(
        edit.clip_id.clone(),
        edit.path.clone(),
        edit.duration,
        tx.clone(),
        ctx.clone(),
    );
}

/// Encaja la vista previa en 380×300 manteniendo el aspecto elegido.
fn fit_preview(pw: f64, ph: f64) -> egui::Vec2 {
    const MAX_W: f32 = 380.0;
    const MAX_H: f32 = 300.0;
    let scale = (MAX_W / pw as f32).min(MAX_H / ph as f32).max(0.01);
    egui::vec2(pw as f32 * scale, ph as f32 * scale)
}

/// Slider 0..=100 → radio de blur de la capa (el fondo de referencia usa 50).
pub(crate) fn blur_radius_for_slider(v: f32) -> f32 {
    v.clamp(0.0, 100.0) / 2.0
}

/// Radio de la capa → slider (restaura al abrir).
pub(crate) fn slider_for_radius(r: f32) -> f32 {
    (r * 2.0).clamp(0.0, 100.0)
}

/// Transform contain escalado `zoom` desde el centro: base encajada (crece
/// solo hasta tocar el borde), la página recorta lo que sobresale.
pub(crate) fn zoom_transform(vw: f64, vh: f64, pw: f64, ph: f64, zoom: f32) -> Transform {
    let base = (pw / vw.max(1.0)).min(ph / vh.max(1.0));
    let z = zoom.max(1.0) as f64 * base;
    Transform::new((pw - vw * z) / 2.0, (ph - vh * z) / 2.0, vw * z, vh * z)
}

/// Ancho actual → zoom contra la base contain (restaura al abrir). Libre
/// hasta 10× (el slider solo llega a 3, el campo manual más).
pub(crate) fn zoom_for_transform(layer_w: f64, vw: f64, vh: f64, pw: f64, ph: f64) -> f32 {
    let base = (pw / vw.max(1.0)).min(ph / vh.max(1.0));
    if base > 0.0 {
        (layer_w / (vw * base)).clamp(1.0, 10.0) as f32
    } else {
        1.0
    }
}
