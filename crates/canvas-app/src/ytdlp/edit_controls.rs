//! Transporte, timeline y parámetros de la ventana de vídeo.

use super::playback::PLAYBACK_FPS;
use super::{clamp_trim, mmss, VideoEdit, CANVAS_SIZES};
use crate::settings::AppSettings;
use eframe::egui;

/// Conserva el tiempo continuo aunque egui repinte entre miniaturas.
/// Solo la selección del fotograma se cuantiza; el reloj nunca se redondea.
pub(super) fn advance_playhead(edit: &mut VideoEdit, ctx: &egui::Context) {
    if !edit.playing {
        edit.last_tick = None;
        return;
    }
    // En reproducción continua, los timestamps publicados por el decoder
    // gobiernan el reloj (incluido el tiempo de arranque y el EOF del trim).
    if edit.playback.is_running() {
        return;
    }
    let now = std::time::Instant::now();
    if let Some(last) = edit.last_tick {
        edit.playhead += now.duration_since(last).as_secs_f64();
    }
    edit.last_tick = Some(now);
    if edit.playhead >= edit.trim_end {
        edit.playhead = edit.trim_end;
        edit.playing = false;
        crate::audio::pause_for(&edit.path);
    }
    if edit.playing {
        let interval = 1.0 / f64::from(PLAYBACK_FPS);
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(interval));
    }
}

/// Play/Pause y Restart sobre el trim, con audio salvo mute.
/// Sin fotogramas el Play se apaga explicando por qué, con reintento.
pub(super) fn transport_ui(edit: &mut VideoEdit, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        if edit.frames.is_empty() || edit.frames_error.is_some() || edit.loading_frames {
            let btn = ui.add_enabled(false, egui::Button::new(crate::i18n::tr("Play")));
            btn.on_hover_text(edit.frames_error.as_deref().unwrap_or({
                if edit.loading_frames {
                    "Extracting preview…"
                } else {
                    "No preview frames"
                }
            }));
            if !edit.loading_frames && ui.button(crate::i18n::tr("Retry preview")).clicked() {
                edit.retry_frames = true;
            }
            return;
        }
        let label = if edit.playing { "Pause" } else { "Play" };
        if ui.button(label).clicked() && !edit.frames.is_empty() {
            if !edit.playing && edit.playhead >= edit.trim_end {
                edit.playhead = edit.trim_start;
            }
            edit.playing = !edit.playing;
            edit.playback.stop();
            edit.last_tick = edit.playing.then(std::time::Instant::now);
            if edit.playing {
                // Arranca el bucle: advance programa los siguientes, pero
                // este frame no tendría continuación sin pedirlo aquí.
                ui.ctx().request_repaint();
            } else {
                crate::audio::pause_for(&edit.path);
            }
        }
        if ui.button(crate::i18n::tr("Restart")).clicked() {
            edit.playback.stop();
            crate::audio::pause_for(&edit.path);
            edit.playhead = edit.trim_start;
            edit.playing = !edit.frames.is_empty();
            edit.last_tick = edit.playing.then(std::time::Instant::now);
            if edit.playing {
                ui.ctx().request_repaint();
            }
        }
        if ui
            .checkbox(&mut edit.mute, crate::i18n::tr("Mute"))
            .changed()
        {
            if edit.mute {
                crate::audio::pause_for(&edit.path);
            } else if edit.playing {
                let _ = crate::audio::play(&edit.path, edit.playhead);
            }
        }
        ui.weak(format!(
            "{} / {}",
            mmss(edit.playhead),
            edit.duration
                .map(mmss)
                .unwrap_or_else(|| "--:--".to_owned())
        ));
    });
}

/// Timeline clicable (el Slider de egui salta al pulsar en cualquier punto).
pub(super) fn timeline_ui(edit: &mut VideoEdit, ui: &mut egui::Ui) {
    let Some(duration) = edit.duration else {
        ui.weak(crate::i18n::tr("Duration unknown yet."));
        return;
    };
    let (start, end) = (edit.trim_start, edit.trim_end);
    // Al mover a mano se pausa (como el panel de propiedades): el audio no
    // persigue el scrub.
    if ui
        .add(
            egui::Slider::new(&mut edit.playhead, start..=end)
                .text(crate::i18n::tr("Timeline"))
                .show_value(false),
        )
        .changed()
    {
        edit.playing = false;
        edit.playback.stop();
        edit.last_tick = None;
        crate::audio::pause_for(&edit.path);
    }
    ui.weak(format!(
        "Trim {} – {}  ·  full {}",
        mmss(start),
        mmss(end),
        mmss(duration)
    ));
    edit.playhead = edit.playhead.clamp(start, end);
}

/// Tamaño de lienzo + trim + background blur + zoom. Sin duración, el trim
/// y el timeline esperan.
pub(super) fn params_ui(edit: &mut VideoEdit, ui: &mut egui::Ui, settings: &mut AppSettings) {
    ui.horizontal(|ui| {
        ui.label(crate::i18n::tr("Canvas:"));
        for (label, w, h) in CANVAS_SIZES {
            if ui
                .selectable_label(settings.ytdlp_canvas_size == (w, h), label)
                .clicked()
            {
                settings.ytdlp_canvas_size = (w, h);
                settings.save_in_background();
            }
        }
    });
    edit.size = settings.ytdlp_canvas_size;
    if let Some(duration) = edit.duration {
        let mut start = edit.trim_start;
        let mut end = edit.trim_end;
        let start_changed = ui
            .add(egui::Slider::new(&mut start, 0.0..=duration).text(crate::i18n::tr("Trim start")))
            .changed();
        let end_changed = ui
            .add(egui::Slider::new(&mut end, 0.0..=duration).text(crate::i18n::tr("Trim end")))
            .changed();
        (edit.trim_start, edit.trim_end) = clamp_trim(start, end, duration);
        if start_changed || end_changed {
            edit.playing = false;
            edit.playback.stop();
            edit.last_tick = None;
            crate::audio::pause_for(&edit.path);
            if start_changed {
                edit.playhead = edit.trim_start;
            }
        }
        edit.playhead = edit.playhead.clamp(edit.trim_start, edit.trim_end);
    } else {
        ui.weak(crate::i18n::tr("Trim waits for duration."));
    }
    ui.add(egui::Slider::new(&mut edit.blur, 0.0..=100.0).text(crate::i18n::tr("Background blur")));
    ui.horizontal(|ui| {
        ui.label(crate::i18n::tr("Zoom"));
        // Slider capado para ajuste rápido + campo manual hasta 10×.
        ui.add(egui::Slider::new(&mut edit.zoom, 1.0..=3.0).show_value(false));
        ui.add(
            egui::DragValue::new(&mut edit.zoom)
                .range(1.0..=10.0)
                .speed(0.05),
        );
    });
    edit.zoom = edit.zoom.clamp(1.0, 10.0);
}
