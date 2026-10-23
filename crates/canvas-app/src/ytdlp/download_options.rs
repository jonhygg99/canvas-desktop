//! Recorte opcional de descarga: valida antes de lanzar yt-dlp.
use crate::ytdlp::state::Panel;
use eframe::egui;

pub(super) type Section = (Option<f64>, Option<f64>);

pub(super) fn validated_section(start: &str, end: &str) -> Result<Section, &'static str> {
    let parse = |text: &str| {
        if text.trim().is_empty() {
            return Ok(None);
        }
        let parts: Vec<_> = text.trim().split(':').collect();
        if parts.len() > 3 {
            return Err("Use seconds or HH:MM:SS.mmm.");
        }
        for (index, part) in parts.iter().enumerate() {
            let value = part.parse::<f64>().map_err(|_| "Enter a valid time.")?;
            if !value.is_finite() || value < 0.0 || (index > 0 && value >= 60.0) {
                return Err("Time must be positive; minutes and seconds must be below 60.");
            }
        }
        super::api::parse_time(text)
            .filter(|v| v.is_finite())
            .map(Some)
            .ok_or("Enter a valid time.")
    };
    let start = parse(start)?;
    let end = parse(end)?;
    if end.is_some_and(|end| end <= start.unwrap_or(0.0)) {
        return Err("End must be later than start.");
    }
    Ok((start, end))
}

/// End a partir de un Start y unos segundos extra. Start vacío vale 0.
/// El texto conserva la forma del Start: `1:23` + 7 s da `1:30`, no `90`
/// (los inputs del formulario aceptan ambas: `validated_section` pasa por
/// `api::parse_time`); sin `:` sigue saliendo en segundos.
fn end_from_start(start_text: &str, extra: f64) -> String {
    let start = if start_text.trim().is_empty() {
        0.0
    } else {
        super::api::parse_time(start_text.trim()).unwrap_or(0.0)
    };
    format_time_like(start_text.trim(), start + extra)
}

/// Redondea a milisegundos y reparte el total en los campos (1-3) que tenía
/// el texto de referencia. La suma se hace en ms para que `59.9 + 7` no
/// acabe en un `:60` que la validación rechaza.
fn format_time_like(reference: &str, total: f64) -> String {
    let fields = reference.matches(':').count() + 1;
    let ms = (total.max(0.0) * 1000.0).round() as u64;
    let (whole, millis) = (ms / 1000, ms % 1000);
    let frac = if millis == 0 {
        String::new()
    } else {
        format!(".{millis:03}").trim_end_matches('0').to_owned()
    };
    let (secs, minutes) = (whole % 60, whole / 60);
    match fields {
        3 => format!(
            "{hours}:{minutes:02}:{secs:02}{frac}",
            hours = minutes / 60,
            minutes = minutes % 60
        ),
        2 => format!("{minutes}:{secs:02}{frac}"),
        _ => format!("{secs}{frac}"),
    }
}

pub(super) fn show(panel: &mut Panel, ui: &mut egui::Ui) -> Result<Section, &'static str> {
    ui.checkbox(&mut panel.mute, "Download without audio")
        .on_hover_text(
            "Removes audio from the saved file. Mute preview only affects playback in Edit video.",
        );
    ui.collapsing("Advanced", |ui| {
        ui.checkbox(&mut panel.download_segment, "Download only a segment");
        ui.add_enabled_ui(panel.download_segment, |ui| {
            // Layout vertical: Start arriba, End justo debajo, y los accesos
            // rápidos de duración (+4/+5/+6/+7 s tras el Start) al final.
            ui.label("Start");
            ui.add(
                egui::TextEdit::singleline(&mut panel.start)
                    // El default útil es «desde el principio».
                    .hint_text("00:00")
                    .desired_width(100.0),
            );
            ui.label("End");
            ui.add(
                egui::TextEdit::singleline(&mut panel.end)
                    .hint_text("End of video")
                    .desired_width(100.0),
            );
            ui.horizontal_wrapped(|ui| {
                for extra in [4.0f64, 5.0, 6.0, 7.0] {
                    let button = ui
                        .add(egui::Button::new(format!("+{}", extra as i32)))
                        .on_hover_text(format!("End = Start + {extra:.0} s"));
                    if button.clicked() {
                        panel.end = end_from_start(&panel.start, extra);
                    }
                }
            });
        });
    });
    let result = if panel.download_segment {
        validated_section(&panel.start, &panel.end)
    } else {
        Ok((None, None))
    };
    if let Err(error) = &result {
        ui.colored_label(ui.visuals().error_fg_color, *error);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quick_duration_buttons_fill_end_from_start() {
        // Start vacío cuenta como 0; los enteros no arrastran decimales.
        assert_eq!(end_from_start("", 4.0), "4");
        assert_eq!(end_from_start("00:00", 7.0), "0:07");
        // Con «:» el resultado sigue en minutos:segundos.
        assert_eq!(end_from_start("1:23", 7.0), "1:30");
        assert_eq!(end_from_start("1:02", 6.0), "1:08");
        // 59.5 + 7 arrastra a otro minuto en vez de dar «:60.5».
        assert_eq!(end_from_start("0:00:59.5", 7.0), "0:01:06.5");
        // Un Start con milisegundos conserva su fracción.
        assert_eq!(end_from_start("3.5", 5.0), "8.5");
        // Texto inválido no bloquea el botón: cae a 0 (la validación de la
        // descarga es la que informa el error).
        assert_eq!(end_from_start("garbage", 4.0), "4");
    }

    #[test]
    fn download_segment_preserves_milliseconds_and_allows_open_end() {
        assert_eq!(
            validated_section("1:02.125", "2:03.456"),
            Ok((Some(62.125), Some(123.456)))
        );
        assert_eq!(validated_section("4.001", ""), Ok((Some(4.001), None)));
        assert_eq!(validated_section("", ""), Ok((None, None)));
    }

    #[test]
    fn invalid_or_reversed_times_cannot_start_a_download() {
        for (start, end) in [
            ("abc", ""),
            ("NaN", ""),
            ("inf", ""),
            ("-1", ""),
            ("1:70", ""),
            ("1:2:3:4", ""),
            ("4", "3"),
            ("4", "4"),
            ("", "0"),
        ] {
            assert!(validated_section(start, end).is_err(), "{start} - {end}");
        }
    }
}
