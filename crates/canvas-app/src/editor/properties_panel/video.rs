//! Controles de reproducción para capa Video (play/pausa/reiniciar + timeline).

use canvas_core::{LayerContent, LayerId};

use crate::editor::EditorState;
use crate::loader;

fn fmt_time(s: f64) -> String {
    let total = s.max(0.0).floor() as u64;
    let m = total / 60;
    let sec = total % 60;
    format!("{m:02}:{sec:02}")
}

pub fn video_controls_ui(
    state: &mut EditorState,
    ui: &mut eframe::egui::Ui,
    sel: LayerId,
    tx: &std::sync::mpsc::Sender<loader::AppMsg>,
    ctx: &eframe::egui::Context,
) {
    let Ok(layer) = state.doc.layer(sel).cloned() else {
        return;
    };
    let LayerContent::Video(vid) = layer.content else {
        return;
    };
    let Some(path) = vid.source_path.clone() else {
        ui.weak("Video sin archivo fuente");
        return;
    };
    let duration = vid.duration_secs.or_else(|| {
        canvas_io::probe_video_size(&path)
            .ok()
            .and_then(|(_, _, d)| d)
    });
    let is_playing = state.video_playing_layer == Some(sel);
    let mut current = vid.poster_time;

    ui.horizontal(|ui| {
        if ui
            .button(if is_playing { "⏸ Pause" } else { "▶ Play" })
            .clicked()
        {
            if is_playing {
                state.video_playing_layer = None;
                state.video_last_tick = None;
            } else {
                state.video_playing_layer = Some(sel);
                state.video_last_tick = Some(std::time::Instant::now());
                // Asegurar que el frame actual está cargado
                if duration.is_some() {
                    // Ya está en poster_time
                }
                ctx.request_repaint();
            }
        }
        if ui.button("↺ Restart").clicked() {
            current = 0.0;
            state.video_playing_layer = None;
            state.video_last_tick = None;
            if let Ok(l) = state.doc.layer_mut(sel) {
                if let LayerContent::Video(v) = &mut l.content {
                    v.poster_time = 0.0;
                }
            }
            loader::spawn_video_frame(path.clone(), sel, 0.0, tx.clone(), ctx.clone());
        }
    });

    // Timeline slider
    let max = duration.unwrap_or(60.0).max(1.0);
    let mut slider_val = current.clamp(0.0, max);
    let slider_text = format!(
        "{} / {}",
        fmt_time(slider_val),
        duration.map(fmt_time).unwrap_or_else(|| "--:--".to_owned())
    );
    let slider_resp = ui.add(
        eframe::egui::Slider::new(&mut slider_val, 0.0..=max)
            .text(slider_text)
            .show_value(false),
    );
    if slider_resp.changed() {
        current = slider_val;
        if let Ok(l) = state.doc.layer_mut(sel) {
            if let LayerContent::Video(v) = &mut l.content {
                v.poster_time = current;
            }
        }
        // Si estaba reproduciendo, pausar al scruBBear
        state.video_playing_layer = None;
        state.video_last_tick = None;
        loader::spawn_video_frame(path.clone(), sel, current, tx.clone(), ctx.clone());
    }
    if slider_resp.drag_stopped() {
        ctx.request_repaint();
    }

    // Avance automático si está reproduciendo
    if is_playing {
        let now = std::time::Instant::now();
        if let Some(last) = state.video_last_tick {
            let dt = now.duration_since(last).as_secs_f64();
            if dt >= 0.05 {
                // 20 fps aprox para no saturar ffmpeg
                let next = current + dt;
                let next = if let Some(d) = duration {
                    if next >= d {
                        0.0
                    } else {
                        next
                    }
                } else {
                    next % max
                };
                if let Ok(l) = state.doc.layer_mut(sel) {
                    if let LayerContent::Video(v) = &mut l.content {
                        v.poster_time = next;
                    }
                }
                state.video_last_tick = Some(now);
                loader::spawn_video_frame(path.clone(), sel, next, tx.clone(), ctx.clone());
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(20));
            }
        } else {
            state.video_last_tick = Some(now);
            ctx.request_repaint();
        }
    }

    if let Some(d) = duration {
        ui.weak(format!(
            "Duración: {}  |  Actual: {}",
            fmt_time(d),
            fmt_time(current)
        ));
    } else {
        ui.weak(format!("Actual: {}", fmt_time(current)));
    }
}
