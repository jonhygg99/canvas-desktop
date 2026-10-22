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
            Some(Action::Trash) => loader::spawn_ytdlp_delete(
                vec![path],
                !panel.downloading,
                tx.clone(),
                ui.ctx().clone(),
            ),
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
    // El formato del clip, a la vista junto a la duración: 9:16, 16:9, 4:5…
    // Así la miniatura vertical no se lee como una tarjeta rota.
    let format = texture
        .map(|texture| texture.size_vec2())
        .map(|size| ThumbFormat::of(size.x / size.y))
        .and_then(ThumbFormat::label);
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.horizontal_top(|ui| {
            thumbnail(ui, texture);
            ui.vertical(|ui| {
                ui.add(egui::Label::new(egui::RichText::new(title).strong()).wrap());
                ui.weak(match format {
                    Some(format) => format!("{duration} · {format}"),
                    None => duration.to_owned(),
                });
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

/// Alto de la miniatura: fijo, para que la lista no cambie de ritmo entre
/// clips. El ANCHO es el que sigue al formato del vídeo.
const THUMB_H: f32 = 56.0;
/// Un clip más panorámico que 21:9 no ensancha más la tarjeta (el sidebar es
/// estrecho): el ancho se acota y el exceso se letterboxea — nunca se deforma.
const THUMB_MAX_W: f32 = 104.0;
/// Un vertical extremo tampoco baja de aquí: por debajo la miniatura deja de
/// leerse y el título queda descolgado.
const THUMB_MIN_W: f32 = 30.0;
/// Tolerancia para reconocer un formato: un 1080×1920 y un 720×1280 comparten
/// tarjeta, y la lista no cambia de anchos con cada descarga.
const FORMAT_TOLERANCE: f32 = 0.04;

/// Formatos de vídeo reconocidos por la miniatura. Antes todos se dibujaban
/// en un marco fijo de 16:9 (`80×48`): un clip vertical quedaba como una tira
/// fina y descuadrado respecto a su clip. Ahora el marco toma la relación del
/// formato real, así que 9:16, 4:5, 1:1, 4:3 y 21:9 se ven con su forma.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ThumbFormat {
    Wide,
    Landscape,
    Classic,
    Square,
    Portrait,
    Tall,
    Other,
}

impl ThumbFormat {
    /// (forma, relación ancho/alto) de los formatos que la tarjeta reconoce.
    const KNOWN: [(Self, f32); 6] = [
        (Self::Wide, 21.0 / 9.0),
        (Self::Landscape, 16.0 / 9.0),
        (Self::Classic, 4.0 / 3.0),
        (Self::Square, 1.0),
        (Self::Portrait, 4.0 / 5.0),
        (Self::Tall, 9.0 / 16.0),
    ];

    /// Formato MÁS CERCANO a la relación real, o `Other` si ninguno encaja.
    fn of(aspect: f32) -> Self {
        if !aspect.is_finite() || aspect <= 0.0 {
            return Self::Landscape;
        }
        Self::KNOWN
            .iter()
            .find(|(_, target)| (aspect / target - 1.0).abs() <= FORMAT_TOLERANCE)
            .map_or(Self::Other, |(format, _)| *format)
    }

    /// Relación exacta del formato, si es uno conocido.
    fn aspect(self) -> Option<f32> {
        Self::KNOWN
            .iter()
            .find(|(format, _)| *format == self)
            .map(|(_, aspect)| *aspect)
    }

    /// Etiqueta corta para la tarjeta (`9:16`, `16:9`…).
    fn label(self) -> Option<&'static str> {
        Some(match self {
            Self::Wide => "21:9",
            Self::Landscape => "16:9",
            Self::Classic => "4:3",
            Self::Square => "1:1",
            Self::Portrait => "4:5",
            Self::Tall => "9:16",
            Self::Other => return None,
        })
    }
}

/// Marco de la miniatura para un vídeo de `video` píxeles. El alto es fijo; el
/// ancho sale del formato reconocido (o del real, acotado) — así el clip no se
/// aplasta dentro de un marco ajeno.
fn thumb_size(video: egui::Vec2) -> egui::Vec2 {
    let real = if video.x > 0.0 && video.y > 0.0 {
        video.x / video.y
    } else {
        16.0 / 9.0
    };
    let target = ThumbFormat::of(real).aspect().unwrap_or(real);
    egui::vec2((THUMB_H * target).clamp(THUMB_MIN_W, THUMB_MAX_W), THUMB_H)
}

/// Rectángulo que contiene `size` centrado en `frame` SIN deformarlo (fit).
fn fit_rect(frame: egui::Rect, size: egui::Vec2) -> egui::Rect {
    let scale = (frame.width() / size.x).min(frame.height() / size.y);
    egui::Rect::from_center_size(frame.center(), size * scale)
}

fn thumbnail(ui: &mut egui::Ui, texture: Option<&egui::TextureHandle>) {
    // Antes de decodificar no se conoce la forma del vídeo: se reserva un
    // 16:9 y la tarjeta se recoloca UNA vez cuando llega la miniatura (el
    // marco ya sigue el formato real; ver `thumb_size`).
    let size = texture.map_or(egui::vec2(16.0, 9.0), |texture| texture.size_vec2());
    let (rect, _) = ui.allocate_exact_size(thumb_size(size), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, 4.0, ui.visuals().extreme_bg_color);
    if let Some(texture) = texture {
        let fitted = fit_rect(rect, size);
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

    /// La tarjeta ya no asume 16:9: cada formato se dibuja con su forma.
    #[test]
    fn the_card_frame_follows_the_clip_format() {
        let wide = thumb_size(egui::vec2(192.0, 108.0));
        let tall = thumb_size(egui::vec2(61.0, 108.0));
        let square = thumb_size(egui::vec2(108.0, 108.0));
        let portrait = thumb_size(egui::vec2(86.0, 108.0));
        assert!(wide.x > wide.y, "16:9 se dibuja apaisado: {wide:?}");
        assert!(tall.y > tall.x, "9:16 se dibuja vertical: {tall:?}");
        assert_eq!(square.x, square.y, "1:1 es cuadrado: {square:?}");
        assert!(
            portrait.x > tall.x && portrait.x < square.x,
            "4:5 cae entre el vertical y el cuadrado: {portrait:?}"
        );
        assert!((wide.x / wide.y - 16.0 / 9.0).abs() < 0.1);
        assert!((tall.x / tall.y - 9.0 / 16.0).abs() < 0.02);
        assert_eq!(tall.y, THUMB_H);
        // Ningún formato inventa un ancho fuera del panel, y el alto no cambia.
        for aspect in [0.1, 0.3, 0.56, 1.0, 1.78, 2.33, 4.0] {
            let size = thumb_size(egui::vec2(aspect * 100.0, 100.0));
            assert!(
                (THUMB_MIN_W..=THUMB_MAX_W).contains(&size.x),
                "{aspect}: {size:?}"
            );
            assert_eq!(size.y, THUMB_H);
        }
        // Un tamaño degenerado (textura aún sin cargar, 0 px) no rompe nada.
        assert_eq!(
            thumb_size(egui::Vec2::ZERO),
            thumb_size(egui::vec2(16.0, 9.0))
        );
    }

    #[test]
    fn known_formats_win_over_their_exact_pixels() {
        assert_eq!(ThumbFormat::of(1080.0 / 1920.0), ThumbFormat::Tall);
        assert_eq!(ThumbFormat::of(720.0 / 1280.0), ThumbFormat::Tall);
        assert_eq!(ThumbFormat::of(4.0 / 5.0), ThumbFormat::Portrait);
        assert_eq!(ThumbFormat::of(1.0), ThumbFormat::Square);
        assert_eq!(ThumbFormat::of(4.0 / 3.0), ThumbFormat::Classic);
        assert_eq!(ThumbFormat::of(16.0 / 9.0), ThumbFormat::Landscape);
        assert_eq!(ThumbFormat::of(21.0 / 9.0), ThumbFormat::Wide);
        assert_eq!(ThumbFormat::of(3.0), ThumbFormat::Other);
        assert_eq!(ThumbFormat::of(0.0), ThumbFormat::Landscape);
        assert_eq!(ThumbFormat::of(f32::NAN), ThumbFormat::Landscape);
        // La etiqueta que ve el usuario nombra cada formato.
        assert_eq!(ThumbFormat::Tall.label(), Some("9:16"));
        assert_eq!(ThumbFormat::Landscape.label(), Some("16:9"));
        assert_eq!(ThumbFormat::Portrait.label(), Some("4:5"));
        assert_eq!(ThumbFormat::Other.label(), None);
    }

    /// Pintado de verdad: la tarjeta de un clip 9:16 dibuja la imagen en un
    /// rectángulo VERTICAL del mismo formato, no en un 16:9 aplastado.
    #[test]
    fn a_vertical_clip_is_painted_upright_and_undistorted() {
        let ctx = egui::Context::default();
        let texture = ctx.load_texture(
            "card-9x16",
            egui::ColorImage::filled([108, 192], egui::Color32::from_rgb(9, 9, 9)),
            egui::TextureOptions::LINEAR,
        );
        let output = ctx.run_ui(
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
                    "Vertical clip",
                    "0:05",
                    Some(&texture),
                    false,
                );
            },
        );
        let mut painted = egui::Rect::NOTHING;
        for clipped in &output.shapes {
            if let egui::Shape::Mesh(mesh) = &clipped.shape {
                if mesh.texture_id == texture.id() {
                    painted = painted.union(mesh.calc_bounds());
                }
            }
        }
        assert!(
            painted.is_positive(),
            "la miniatura no se pintó: {painted:?}"
        );
        assert!(painted.height() > painted.width(), "vertical: {painted:?}");
        assert!(
            (painted.width() / painted.height() - 9.0 / 16.0).abs() < 0.02,
            "el 9:16 se pinta con su forma: {painted:?}"
        );
    }

    /// El fit conserva la relación: la imagen nunca se estira dentro del marco,
    /// aunque el formato reconocido y los píxeles no coincidan al 100 %.
    #[test]
    fn the_image_is_never_stretched_inside_the_frame() {
        let frame = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(30.0, 56.0));
        for size in [
            egui::vec2(1080.0, 1920.0),
            egui::vec2(192.0, 108.0),
            egui::vec2(1.0, 3.0),
        ] {
            let fitted = fit_rect(frame, size);
            assert!((fitted.width() / fitted.height() - size.x / size.y).abs() < 0.001);
            assert!(fitted.width() <= frame.width() + 0.001);
            assert!(fitted.height() <= frame.height() + 0.001);
        }
    }
}
