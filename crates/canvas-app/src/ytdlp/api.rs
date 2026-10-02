//! Resolución del binario yt-dlp, parseo de tiempos y naming
//! `clip-[carpeta](x).mp4`.

use std::path::{Path, PathBuf};

const HARDCODED_YTDLP_DIR: &str = r"C:\Users\jonhy\Documents\code-projects\yt-dlp";
const HARDCODED_YTDLP_EXE: &str = r"C:\Users\jonhy\Documents\code-projects\yt-dlp\yt-dlp.exe";

/// Localiza `yt-dlp.exe`: `YTDLP_PATH`/`CANVAS_YTDLP_DIR` → carpeta fijada
/// del usuario → `PATH`. Devuelve la ruta al ejecutable.
pub fn ytdlp_path() -> Option<PathBuf> {
    for key in ["YTDLP_PATH", "CANVAS_YTDLP_DIR", "CANVAS_FFMPEG_DIR"] {
        if let Ok(v) = std::env::var(key) {
            let p = PathBuf::from(&v);
            let cand_exe = p.join("yt-dlp.exe");
            if cand_exe.is_file() {
                return Some(cand_exe);
            }
            let cand = p.join("yt-dlp");
            if cand.is_file() {
                return Some(cand);
            }
            if p.is_file() {
                return Some(p);
            }
        }
    }
    if Path::new(HARDCODED_YTDLP_EXE).is_file() {
        return Some(PathBuf::from(HARDCODED_YTDLP_EXE));
    }
    if canvas_io::media_command("yt-dlp")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
    {
        return Some(PathBuf::from("yt-dlp"));
    }
    None
}

/// Directorio con `ffmpeg` para `--ffmpeg-location` (mismo que yt-dlp).
pub fn ffmpeg_location() -> Option<PathBuf> {
    if let Some(exe) = ytdlp_path() {
        if let Some(parent) = exe.parent() {
            if parent.join("ffmpeg.exe").is_file() || parent.join("ffmpeg").is_file() {
                return Some(parent.to_owned());
            }
        }
    }
    if Path::new(HARDCODED_YTDLP_DIR).is_dir() {
        return Some(PathBuf::from(HARDCODED_YTDLP_DIR));
    }
    None
}

/// Fotogramas por segundo de la vista previa del editor: como mucho
/// `MAX_FRAMES` miniaturas repartidas por el clip, con un máximo de 2 fps.
pub fn preview_fps(duration: f64) -> f64 {
    const MAX_FRAMES: f64 = 120.0;
    if !duration.is_finite() || duration <= 0.0 {
        return 1.0;
    }
    (MAX_FRAMES / duration).min(2.0)
}

/// Limita un trim a `[0, duration]`, con `start <= end` y mínimo 0.1 s.
/// `duration <= 0` = aún desconocida: solo ordena y exige mínimo.
pub fn clamp_trim(start: f64, end: f64, duration: f64) -> (f64, f64) {
    let (mut s, mut e) = if start <= end {
        (start, end)
    } else {
        (end, start)
    };
    if duration > 0.0 {
        s = s.clamp(0.0, duration);
        e = e.clamp(0.0, duration);
    } else {
        s = s.max(0.0);
        e = e.max(0.0);
    }
    if e - s < 0.1 {
        if duration > 0.0 {
            if s + 0.1 > duration {
                e = duration;
                s = (e - 0.1).max(0.0);
            } else {
                e = s + 0.1;
            }
        } else {
            e = s + 0.1;
        }
    }
    (s, e)
}

/// Parte `https://…` válidas del texto libre (una por línea o por espacios).
pub fn split_urls(text: &str) -> Vec<String> {
    text.split(|c: char| c.is_whitespace() || c == ',')
        .map(str::trim)
        .filter(|s| s.starts_with("http://") || s.starts_with("https://"))
        .map(str::to_owned)
        .collect()
}

/// ¿Parece URL de playlist de YouTube?
pub fn is_playlist_url(url: &str) -> bool {
    url.contains("list=")
}

/// `HH:MM:SS`, `MM:SS` o segundos sueltos → segundos. Vacío = `None`.
pub fn parse_time(text: &str) -> Option<f64> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    if let Ok(v) = t.parse::<f64>() {
        return if v >= 0.0 { Some(v) } else { None };
    }
    let parts: Vec<&str> = t.split(':').collect();
    let mut total = 0.0;
    for p in parts {
        let v: f64 = p.trim().parse().ok()?;
        if !(0.0..60.0).contains(&v) && total > 0.0 {
            // Minutos/horas pueden pasar de 60, segundos no tras `:`.
        }
        if v < 0.0 {
            return None;
        }
        total = total * 60.0 + v;
    }
    Some(total)
}

/// Formatea segundos como `H:MM:SS` para `--download-sections "*a-b"`.
pub fn fmt_section_time(secs: f64) -> String {
    let total = secs.max(0.0).floor() as u64;
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// `*inicio-fin` para `--download-sections` (`inf` = hasta el final).
pub fn section_arg(start: Option<f64>, end: Option<f64>) -> Option<String> {
    match (start, end) {
        (None, None) => None,
        (s, e) => {
            let a = s.map(fmt_section_time).unwrap_or_else(|| "0:00".to_owned());
            let b = e.map(fmt_section_time).unwrap_or_else(|| "inf".to_owned());
            Some(format!("*{a}-{b}"))
        }
    }
}

/// Carpeta de Vídeos del usuario para descargar sin contexto (diseño nuevo
/// en blanco). `None` si el SO no la expone.
pub fn default_download_dir() -> Option<PathBuf> {
    directories::UserDirs::new()?
        .video_dir()
        .map(|d| d.to_owned())
}

/// Destino del Download: contexto vivo (baraja/archivo/galería) → última
/// carpeta usada → Vídeos del usuario. Así un diseño nuevo también descarga.
pub fn resolve_download_dir(
    deck: Option<PathBuf>,
    file_parent: Option<PathBuf>,
    gallery: Option<PathBuf>,
    last: Option<PathBuf>,
) -> Option<PathBuf> {
    deck.filter(|d| d.is_dir())
        .or(file_parent)
        .or(gallery)
        .or(last)
        .or_else(default_download_dir)
}

/// `clip-[nombre-carpeta]`: el stem base del archivo descargado.
pub fn clip_stem(folder: &Path) -> String {
    let name = folder
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| "clip".to_owned());
    format!("clip-{name}")
}

/// Reserva `clip-Nombre.mp4`, `clip-Nombre (2).mp4`… con `create_new`
/// (misma disciplina anti-TOCTOU que `reserve_unique_path`).
pub fn reserve_clip_path(folder: &Path, stem: &str) -> Result<PathBuf, std::io::Error> {
    for n in 0..10_000u32 {
        let name = if n == 0 {
            format!("{stem}.mp4")
        } else {
            format!("{stem} ({}).mp4", n + 1)
        };
        let candidate = folder.join(&name);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(_) => return Ok(candidate),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::other("too many name collisions"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_times() {
        assert_eq!(parse_time(""), None);
        assert_eq!(parse_time("90"), Some(90.0));
        assert_eq!(parse_time("1:30"), Some(90.0));
        assert_eq!(parse_time("1:02:03"), Some(3723.0));
        assert_eq!(parse_time("0:10"), Some(10.0));
        assert_eq!(parse_time("abc"), None);
    }

    #[test]
    fn splits_urls() {
        let urls = split_urls("https://youtu.be/a\nhttps://youtu.be/b not-a-url");
        assert_eq!(urls.len(), 2);
    }

    #[test]
    fn section_args() {
        assert_eq!(section_arg(None, None), None);
        assert_eq!(section_arg(Some(75.0), None), Some("*1:15-inf".to_owned()));
        assert_eq!(
            section_arg(Some(10.0), Some(25.0)),
            Some("*0:10-0:25".to_owned())
        );
    }

    #[test]
    fn clip_naming_walks_past_collisions() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p1 = reserve_clip_path(dir.path(), "clip-Joanna").expect("primero");
        assert_eq!(p1, dir.path().join("clip-Joanna.mp4"));
        let p2 = reserve_clip_path(dir.path(), "clip-Joanna").expect("segundo");
        assert_eq!(p2, dir.path().join("clip-Joanna (2).mp4"));
    }

    #[test]
    fn preview_fps_caps_frame_count() {
        assert_eq!(preview_fps(0.0), 1.0);
        assert_eq!(preview_fps(30.0), 2.0);
        assert_eq!(preview_fps(120.0), 1.0);
        assert_eq!(preview_fps(600.0), 0.2);
    }

    #[test]
    fn clamp_trim_orders_and_clamps() {
        assert_eq!(clamp_trim(5.0, 2.0, 10.0), (2.0, 5.0));
        assert_eq!(clamp_trim(-3.0, 99.0, 10.0), (0.0, 10.0));
        assert_eq!(clamp_trim(4.0, 4.0, 10.0), (4.0, 4.1));
        assert_eq!(clamp_trim(7.0, 3.0, 0.0), (3.0, 7.0));
    }

    #[test]
    fn trim_never_extends_past_the_video_end() {
        assert_eq!(clamp_trim(10.0, 10.0, 10.0), (9.9, 10.0));
        assert_eq!(clamp_trim(0.0, 0.0, 0.05), (0.0, 0.05));
    }

    #[test]
    fn long_previews_stay_within_the_thumbnail_budget() {
        for duration in [600.0, 3600.0, 86_400.0] {
            assert!(preview_fps(duration) * duration <= 120.0);
        }
    }

    #[test]
    fn download_dir_prefers_live_context_then_last() {
        let deck = tempfile::tempdir().expect("deck");
        let last = tempfile::tempdir().expect("last");
        let deck_path = deck.path().to_owned();
        let last_path = last.path().to_owned();
        // Baraja viva manda; vacía (diseño nuevo) cae a la última.
        assert_eq!(
            resolve_download_dir(Some(deck_path.clone()), None, None, Some(last_path.clone())),
            Some(deck_path)
        );
        assert_eq!(
            resolve_download_dir(None, None, None, Some(last_path.clone())),
            Some(last_path)
        );
        // Sin nada, Vídeos del SO o None (según la máquina; no debe pánico).
        let _ = resolve_download_dir(None, None, None, None);
    }
}
