//! Cadencia media del vídeo para los controles de avance por fotograma.
use std::path::Path;

pub fn probe_video_fps(path: &Path) -> Option<f64> {
    let probe = crate::ffprobe_path()?;
    let out = crate::media_command(probe)
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=avg_frame_rate",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
        ])
        .arg(path)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_rate(String::from_utf8_lossy(&out.stdout).trim())
}

fn parse_rate(text: &str) -> Option<f64> {
    let (numerator, denominator) = text.split_once('/')?;
    let value = numerator.parse::<f64>().ok()? / denominator.parse::<f64>().ok()?;
    (value.is_finite() && value > 0.0 && value <= 1000.0).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::parse_rate;

    #[test]
    fn fractional_rates_are_preserved_and_invalid_rates_rejected() {
        assert_eq!(parse_rate("30000/1001"), Some(30000.0 / 1001.0));
        assert_eq!(parse_rate("60/1"), Some(60.0));
        for value in ["0/0", "30/0", "NaN/1", "-30/1", "N/A", "1001/1"] {
            assert_eq!(parse_rate(value), None);
        }
    }
}
