//! Tarjetas de clips: miniatura, duración y acciones que no desbordan el panel.
use super::Panel;
use crate::loader::{self, AppMsg};
use eframe::egui;
use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;
use std::sync::mpsc::Sender;

pub(super) fn show(panel: &mut Panel, ui: &mut egui::Ui, tx: &Sender<AppMsg>) {
    panel.clip_previews.update(&panel.done, ui.ctx());
    panel.clip_info.retain(|path, _| panel.done.contains(path));
    for path in panel.done.clone() {
        let title = panel
            .clip_info
            .get(&path)
            .map(|clip| clip.title.as_str())
            .unwrap_or_else(|| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("Video")
            });
        let preview = panel.clip_previews.get(&path);
        let duration = preview
            .and_then(|preview| preview.duration)
            .map(duration_label)
            .unwrap_or_else(|| {
                if preview.is_some() {
                    "Duration unavailable".into()
                } else {
                    "Reading duration...".into()
                }
            });
        let texture = preview.and_then(|preview| preview.texture.as_ref());
        let action = card(ui, &path, title, &duration, texture, panel.downloading);
        match action {
            Some(Action::Edit) => panel.pending_edit = Some(path),
            Some(Action::Insert) => panel.pending_insert = Some(path),
            Some(Action::Folder) => {
                if let Err(error) = show_folder(&path) {
                    panel.error = Some(error);
                }
            }
            Some(Action::Trash) => {
                loader::spawn_ytdlp_delete(vec![path], tx.clone(), ui.ctx().clone())
            }
            None => {}
        }
    }
}

enum Action {
    Edit,
    Insert,
    Folder,
    Trash,
}

fn card(
    ui: &mut egui::Ui,
    path: &Path,
    title: &str,
    duration: &str,
    texture: Option<&egui::TextureHandle>,
    busy: bool,
) -> Option<Action> {
    let mut action = None;
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.horizontal_top(|ui| {
            thumbnail(ui, texture);
            ui.vertical(|ui| {
                ui.add(egui::Label::new(egui::RichText::new(title).strong()).wrap());
                ui.weak(duration);
            });
        });
        ui.add(egui::Label::new(path.file_name().unwrap_or_default().to_string_lossy()).truncate())
            .on_hover_text(path.display().to_string());
        ui.horizontal_wrapped(|ui| {
            let exists = path.is_file();
            if ui
                .add_enabled(exists, egui::Button::new("Edit video"))
                .clicked()
            {
                action = Some(Action::Edit);
            }
            if ui
                .add_enabled(exists, egui::Button::new("Insert on canvas"))
                .clicked()
            {
                action = Some(Action::Insert);
            }
            if ui.button("Show in folder").clicked() {
                action = Some(Action::Folder);
            }
            if ui
                .add_enabled(!busy && exists, egui::Button::new("Move to trash"))
                .clicked()
            {
                action = Some(Action::Trash);
            }
        });
    });
    action
}

fn thumbnail(ui: &mut egui::Ui, texture: Option<&egui::TextureHandle>) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(80.0, 48.0), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, 4.0, ui.visuals().extreme_bg_color);
    if let Some(texture) = texture {
        let size = texture.size_vec2();
        let scale = (rect.width() / size.x).min(rect.height() / size.y);
        let fitted = egui::Rect::from_center_size(rect.center(), size * scale);
        ui.painter().image(
            texture.id(),
            fitted,
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    } else {
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Video",
            egui::FontId::proportional(12.0),
            ui.visuals().weak_text_color(),
        );
    }
}

fn duration_label(seconds: f64) -> String {
    let seconds = seconds.round() as u64;
    if seconds >= 3600 {
        format!(
            "{}:{:02}:{:02}",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60
        )
    } else {
        format!("{}:{:02}", seconds / 60, seconds % 60)
    }
}

fn show_folder(path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    let result = canvas_io::media_command("explorer.exe")
        .arg("/select,")
        .arg(path)
        .spawn();
    #[cfg(target_os = "macos")]
    let result = canvas_io::media_command("open").arg("-R").arg(path).spawn();
    #[cfg(target_os = "linux")]
    let result = canvas_io::media_command("xdg-open")
        .arg(path.parent().unwrap_or(path))
        .spawn();
    result
        .map(|_| ())
        .map_err(|error| format!("Could not open folder: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn long_clip_titles_fit_a_narrow_sidebar() {
        let ctx = egui::Context::default();
        let mut width = 0.0;
        let _ = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(240.0, 700.0),
                )),
                ..Default::default()
            },
            |ui| {
                card(
                    ui,
                    &PathBuf::from("clip.mp4"),
                    &"A long video title ".repeat(15),
                    "1:03",
                    None,
                    false,
                );
                width = ui.min_rect().width();
            },
        );
        assert!(width <= 240.0, "card exceeds sidebar: {width}");
        assert_eq!(duration_label(3661.0), "1:01:01");
    }
}
