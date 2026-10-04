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
                let start = if video.position >= video.duration - 0.05 {
                    0.0
                } else {
                    video.position
                };
                video.seek(start, true, ui.ctx());
            }
        }
        if ui.button("Restart").clicked() {
            video.seek(0.0, video.playing, ui.ctx());
        }
    });
    ui.label(format!(
        "{} / {}",
        time(video.position),
        time(video.duration)
    ));
    let mut position = video.position;
    let response = ui
        .add(egui::Slider::new(&mut position, 0.0..=video.duration).show_value(false))
        .on_hover_text("Seek video");
    if response.changed() {
        video.pause();
        video.position = position;
    }
    if response.drag_stopped() || response.changed() && !response.dragged() {
        video.seek(position, false, ui.ctx());
    }
    ui.weak("The blurred background follows the same video frame.");
    ui.separator();
}
