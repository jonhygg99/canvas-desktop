//! Transporte, timeline y parámetros de la ventana de vídeo.

use super::playback::PLAYBACK_FPS;
use super::{timecode, VideoEdit, CANVAS_SIZES};
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
        edit.playhead = if edit.loop_selection {
            edit.trim_start
        } else {
            edit.trim_end
        };
        edit.playing = edit.loop_selection;
        crate::audio::pause_for(&edit.path);
    }
    if edit.playing {
        let interval = 1.0 / f64::from(PLAYBACK_FPS);
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(interval));
    }
}

/// Play/Pause y Restart sobre el trim, con audio salvo mute.
/// Sin fotogramas el Play se apaga explicando por qué, con reintento.
pub(super) fn transport_ui(
    edit: &mut VideoEdit,
    ui: &mut egui::Ui,
    settings: &mut AppSettings,
) -> [egui::Response; 2] {
    let buttons = ui
        .horizontal_wrapped(|ui| {
            let ready =
                !edit.frames.is_empty() && edit.frames_error.is_none() && !edit.loading_frames;
            let buttons = playback_buttons(edit, ui, ready);
            ui.add_enabled_ui(ready, |ui| {
                if super::transport_icons::frame_button(ui, false).clicked() {
                    edit.seek(edit.playhead - 1.0 / edit.source_fps);
                }
                if super::transport_icons::frame_button(ui, true).clicked() {
                    edit.seek(edit.playhead + 1.0 / edit.source_fps);
                }
            });
            if super::transport_icons::mute_button(ui, edit.mute, ready).clicked() {
                edit.mute = !edit.mute;
                if edit.mute {
                    crate::audio::pause_for(&edit.path);
                } else if edit.playing {
                    let _ = crate::audio::play(&edit.path, edit.playhead);
                }
            }
            canvas_size_ui(edit, settings, ui);
            buttons
        })
        .inner;
    ui.weak(format!(
        "{} / {}",
        timecode(edit.playhead),
        edit.duration
            .map(timecode)
            .unwrap_or_else(|| "--:--".to_owned())
    ));
    buttons
}

fn playback_buttons(edit: &mut VideoEdit, ui: &mut egui::Ui, ready: bool) -> [egui::Response; 2] {
    let mut play = super::transport_icons::play_button(ui, edit.playing, ready);
    if !ready {
        play = play.on_hover_text(
            edit.frames_error
                .as_deref()
                .unwrap_or(if edit.loading_frames {
                    "Extracting preview..."
                } else {
                    "No preview frames"
                }),
        );
    }
    if play.clicked() {
        if !edit.playing {
            edit.prepare_playback_start();
        }
        edit.playing = !edit.playing;
        edit.playback.stop();
        edit.last_tick = edit.playing.then(std::time::Instant::now);
        if edit.playing {
            ui.ctx().request_repaint();
        } else {
            crate::audio::pause_for(&edit.path);
        }
    }
    let label = if ready { "Restart" } else { "Retry preview" };
    let restart = super::transport_icons::restart_button(ui, !edit.loading_frames, label);
    if restart.clicked() {
        if ready {
            edit.seek(edit.trim_start);
            edit.playing = true;
            edit.last_tick = Some(std::time::Instant::now());
            ui.ctx().request_repaint();
        } else {
            edit.retry_frames = true;
        }
    }
    [play, restart]
}

fn canvas_size_ui(edit: &mut VideoEdit, settings: &mut AppSettings, ui: &mut egui::Ui) {
    let before = settings.ytdlp_canvas_size;
    let selected = CANVAS_SIZES
        .iter()
        .find(|(_, w, h)| (*w, *h) == before)
        .map(|(label, _, _)| *label)
        .unwrap_or("Custom");
    ui.allocate_ui_with_layout(
        egui::vec2(175.0, 30.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            egui::ComboBox::from_id_salt("video-canvas-size")
                .selected_text(selected)
                .width(175.0)
                .truncate()
                .show_ui(ui, |ui| {
                    for (label, w, h) in CANVAS_SIZES {
                        ui.selectable_value(&mut settings.ytdlp_canvas_size, (w, h), label);
                    }
                })
                .response
                .on_hover_text(format!("Canvas size: {selected}"));
        },
    );
    if before != settings.ytdlp_canvas_size {
        settings.save_in_background();
    }
    edit.size = settings.ytdlp_canvas_size;
}

/// Timeline clicable (el Slider de egui salta al pulsar en cualquier punto).
pub(super) fn timeline_ui(edit: &mut VideoEdit, ui: &mut egui::Ui) {
    if edit.duration.is_none() {
        ui.weak(crate::i18n::tr("Duration unknown yet."));
        return;
    }
    super::timeline::show(edit, ui);
}

/// Zoom, background blur y trim; el tamaño se ajusta junto al transporte.
pub(super) fn params_ui(edit: &mut VideoEdit, ui: &mut egui::Ui) {
    ui.spacing_mut().slider_width = (ui.available_width() - 70.0).max(80.0);
    ui.label("Zoom");
    ui.horizontal_wrapped(|ui| {
        ui.add(
            egui::Slider::new(&mut edit.zoom, 1.0..=3.0)
                .show_value(false)
                .clamping(egui::SliderClamping::Edits),
        );
        ui.add(
            egui::DragValue::new(&mut edit.zoom)
                .range(1.0..=10.0)
                .speed(0.05),
        );
    });
    edit.zoom = edit.zoom.clamp(1.0, 10.0);
    ui.label("Background blur");
    ui.add(egui::Slider::new(&mut edit.blur, 0.0..=100.0));
    ui.separator();
    ui.spacing_mut().slider_width = (ui.available_width() - 8.0).min(300.0);
    super::trim_controls::trim_controls(edit, ui);
}
