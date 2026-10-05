//! Transporte visible arriba del panel; los fotogramas avanzan desde el editor.
use canvas_core::{LayerContent, LayerId, SetContent};
use eframe::egui;

use crate::editor::EditorState;

fn fmt_time(s: f64) -> String {
    let total = s.max(0.0).floor() as u64;
    format!(
        "{:02}:{:02}.{:01}",
        total / 60,
        total % 60,
        (s.fract().max(0.0) * 10.0) as u64
    )
}

pub(super) fn video_controls_ui(state: &mut EditorState, ui: &mut egui::Ui, sel: LayerId) {
    let Ok(layer) = state.doc.layer(sel).cloned() else {
        return;
    };
    let LayerContent::Video(vid) = &layer.content else {
        return;
    };
    if vid.source_path.is_none() {
        ui.weak(crate::i18n::tr("Video without a source file"));
        return;
    }
    let (start, end) = vid.playback_range();
    let max = end.unwrap_or(vid.poster_time.max(start) + 60.0);
    let is_playing = state.video_playing_layer == Some(sel);
    ui.strong(crate::i18n::tr("Video"));
    ui.horizontal(|ui| {
        if ui
            .button(if is_playing { "Pause" } else { "Play" })
            .clicked()
        {
            if is_playing {
                state.pause_video();
            } else {
                let current = if vid.poster_time >= max - 0.001 {
                    start
                } else {
                    vid.poster_time
                };
                state.seek_video(sel, current, true, ui.ctx());
            }
        }
        if ui.button(crate::i18n::tr("Restart")).clicked() {
            state.seek_video(sel, start, is_playing, ui.ctx());
        }
        ui.label(format!(
            "{} / {}",
            fmt_time(vid.poster_time),
            end.map(fmt_time).unwrap_or_else(|| "--:--".into())
        ));
    });
    let mut current = vid.poster_time.clamp(start, max);
    let timeline = ui
        .add(egui::Slider::new(&mut current, start..=max).show_value(false))
        .on_hover_text(crate::i18n::tr("Seek within the trimmed clip"));
    if timeline.changed() {
        state.pause_video();
        if let Ok(layer) = state.doc.layer_mut(sel) {
            if let LayerContent::Video(video) = &mut layer.content {
                video.poster_time = current;
            }
        }
    }
    // Durante el arrastre solo cambia el indicador; decodifica al soltar para
    // no lanzar un proceso de seek por cada movimiento del puntero.
    if timeline.drag_stopped() || (timeline.changed() && !timeline.dragged()) {
        state.seek_video(sel, current, false, ui.ctx());
    }

    egui::CollapsingHeader::new(crate::i18n::tr("Trim clip"))
        .id_salt((sel.raw(), "trim"))
        .show(ui, |ui| {
            let mut edited = vid.clone();
            let mut changed = false;
            let mut commit = false;
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr("In"));
                let response = ui.add(
                    egui::DragValue::new(&mut edited.trim_start)
                        .speed(0.1)
                        .range(0.0..=(max - 0.05).max(0.0))
                        .suffix(" s"),
                );
                changed |= response.changed();
                commit |= response.drag_stopped() || response.lost_focus();
                ui.label(crate::i18n::tr("Out"));
                let mut out = max;
                let response = ui.add(
                    egui::DragValue::new(&mut out)
                        .speed(0.1)
                        .range((edited.trim_start + 0.05)..=vid.duration_secs.unwrap_or(f64::MAX))
                        .suffix(" s"),
                );
                commit |= response.drag_stopped() || response.lost_focus();
                if response.changed() {
                    edited.trim_end = Some(out);
                    changed = true;
                }
            });
            if ui.button(crate::i18n::tr("Reset trim")).clicked() {
                edited.trim_start = 0.0;
                edited.trim_end = None;
                changed = true;
                commit = true;
            }
            if changed {
                let (start, end) = edited.playback_range();
                edited.trim_start = start;
                edited.trim_end = end;
                let time = edited.poster_time.max(start).min(end.unwrap_or(f64::MAX));
                state.pause_video();
                state.selection = canvas_core::Selection::single(sel);
                super::commit_stale_panel_edits(state);
                if state.content_edit.is_none() {
                    state.content_edit = Some((sel, layer.content.clone()));
                }
                state.doc.layer_mut(sel).expect("existing video").content =
                    LayerContent::Video(edited);
                state.seek_video(sel, time, false, ui.ctx());
            }
            if commit {
                if let Some((id, before)) = state.content_edit.take() {
                    let after = state
                        .doc
                        .layer(id)
                        .map(|l| l.content.clone())
                        .unwrap_or_else(|_| before.clone());
                    if after != before {
                        state.push_undo_step(Box::new(SetContent {
                            layer: id,
                            before,
                            after,
                        }));
                    }
                }
            }
        });
    ui.weak(crate::i18n::tr(
        "Crop: drag the corners · Size / Zoom: enlarge the video",
    ));
    ui.separator();
}
