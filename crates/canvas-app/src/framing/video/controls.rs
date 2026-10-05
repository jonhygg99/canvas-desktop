use super::Video;
use eframe::egui;

fn time(value: f64) -> String {
    let seconds = value.max(0.0) as u64;
    format!(
        "{:02}:{:02}.{:01}",
        seconds / 60,
        seconds % 60,
        (value.fract() * 10.0) as u64
    )
}
pub(super) fn show(video: &mut Video, ui: &mut egui::Ui) {
    ui.strong("Video");
    ui.horizontal(|ui| {
        if ui
            .button(if video.playing { "Pause" } else { "Play" })
            .clicked()
        {
            if video.playing {
                video.pause();
            } else {
                let start = if video.finished || video.position >= video.trim.end_s - 0.05 {
                    video.trim.start_s
                } else {
                    video.position
                };
                video.seek(start, true, ui.ctx());
            }
        }
        if ui.button("Restart").clicked() {
            video.seek(video.trim.start_s, video.playing, ui.ctx());
        }
    });
    ui.label(format!(
        "{} / {}",
        time(video.position),
        time(video.duration)
    ));
    let mut position = video.position;
    let response = ui
        .add(
            egui::Slider::new(&mut position, video.trim.start_s..=video.trim.end_s)
                .show_value(false),
        )
        .on_hover_text("Seek video");
    if response.changed() {
        video.pause();
        video.position = position;
    }
    if response.drag_stopped() || response.changed() && !response.dragged() {
        video.seek(position, false, ui.ctx());
    }
    let mut muted = video.muted;
    if ui.checkbox(&mut muted, "Mute preview audio").changed() {
        video.set_muted(muted, ui.ctx());
    }
    if let Some(error) = &video.audio_error {
        ui.weak(format!("Preview audio unavailable: {error}"));
    }
    trim(video, ui);
    ui.weak("The blurred background follows the same video frame.");
    ui.separator();
}

fn trim(video: &mut Video, ui: &mut egui::Ui) {
    ui.strong("Trim original video");
    let minimum = 0.05_f64.min(video.duration);
    let mut trim = video.trim;
    let input = ui.add(
        egui::DragValue::new(&mut trim.start_s)
            .speed(0.1)
            .range(0.0..=(trim.end_s - minimum).max(0.0))
            .prefix("In: ")
            .suffix(" s"),
    );
    let output = ui.add(
        egui::DragValue::new(&mut trim.end_s)
            .speed(0.1)
            .range((trim.start_s + minimum)..=video.duration)
            .prefix("Out: ")
            .suffix(" s"),
    );
    let reset = ui.button("Reset trim").clicked();
    if reset {
        trim = canvas_io::VideoTrim {
            start_s: 0.0,
            end_s: video.duration,
        };
    }
    if input.changed() || output.changed() || reset {
        let _ = video.apply_trim(trim);
    }
    if input.drag_stopped()
        || output.drag_stopped()
        || input.lost_focus()
        || output.lost_focus()
        || reset
    {
        video.seek(video.position, false, ui.ctx());
    }
    ui.small(format!(
        "Selected duration: {}",
        time(video.trim.end_s - video.trim.start_s)
    ));
}
