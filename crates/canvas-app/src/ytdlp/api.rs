//! Resolución del binario yt-dlp, parseo de tiempos y naming
//! `clip-[carpeta](x).mp4`.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Localiza `yt-dlp.exe`: `YTDLP_PATH`/`CANVAS_YTDLP_DIR` → `PATH`.
/// Devuelve la ruta al ejecutable.
///
/// Se resuelve UNA vez por proceso (caché): el fallback a `PATH` lanza
/// `yt-dlp --version` en un proceso hijo y esta función se llama en cada
/// frame con la pestaña Download visible — sin caché, un yt-dlp PyInstaller
/// (arranque de cientos de ms) deja la UI a ~1 fps. Cambiar el entorno
/// requiere reiniciar la app (el propio panel ya lo indica).
pub fn ytdlp_path() -> Option<PathBuf> {
    static CACHE: OnceLock<Option<PathBuf>> = OnceLock::new();
    CACHE.get_or_init(resolve_ytdlp_path).clone()
}

fn resolve_ytdlp_path() -> Option<PathBuf> {
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
    let millis = (secs.max(0.0) * 1000.0).round() as u64;
    let total = millis / 1000;
    let fraction = if millis.is_multiple_of(1000) {
        String::new()
    } else {
        format!(".{:03}", millis % 1000)
    };
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}{fraction}")
    } else {
        format!("{m}:{s:02}{fraction}")
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

/// Carpeta temporal donde caen TODAS las descargas de yt-dlp
/// (`clip(2).mp4`, con sus `.part` de reanudación): el proyecto nunca recibe
/// el mp4 completo. La lista de Download es su dueña hasta que el clip se
/// borra desde ahí (papelera del SO).
pub fn staging_dir_path() -> PathBuf {
    std::env::temp_dir().join("canvas-desktop-downloads")
}

/// Destino del Download: siempre el temporal. Antes era el contexto vivo
/// (baraja/archivo/galería) → última carpeta → Vídeos, y el mp4 completo
/// acababa dentro del proyecto. SIN crear: un download real la vuelve a
/// crear (`download_one`), y el panel no debe dejar una carpeta vacía
/// tras cada «Move all».
pub fn resolve_download_dir() -> PathBuf {
    staging_dir_path()
}

/// La única tabla de extensiones de descarga. La usa el LISTADO del
/// sidebar (clips terminados) y la PODA de la carpeta (clips + parciales):
/// una extensión nueva (`.webm` de un «mute» sin audio que fusionar) se da
/// de alta aquí y ambos caminos dejan de ser copias que divergen.
const CLIP_EXTS: [&str; 3] = ["mp4", "webm", "m4a"];
const PART_EXTS: [&str; 2] = ["part", "ytdl"];

/// ¿Listaría el sidebar esta ruta como clip descargado? Solo los
/// contenedores de una descarga TERMINADA (los parciales no).
pub fn is_clip_name(path: &Path) -> bool {
    path.extension().is_some_and(|ext| {
        CLIP_EXTS
            .iter()
            .any(|known| ext.eq_ignore_ascii_case(known))
    })
}

/// ¿Esta ruta mantiene viva la carpeta staging? Clips más parciales y
/// `.ytdl` de reanudación (pueden pertenecer a una descarga en vuelo de
/// otra ventana).
pub fn is_download_name(path: &Path) -> bool {
    is_clip_name(path)
        || path.extension().is_some_and(|ext| {
            PART_EXTS
                .iter()
                .any(|known| ext.eq_ignore_ascii_case(known))
        })
}

/// Borra del staging los mp4 (y sus `.part`/`.ytdl` de reanudación) más
/// viejos que `older_than` (lo mejor posible, sin fallar). La app lo llama
/// una vez al arrancar, en un hilo de fondo: la lista de Download es solo de
/// sesión, así que lo viejo es por definición huérfano de sesiones
/// anteriores — lo reciente (esta sesión) no se toca.
pub fn sweep_stale_downloads(older_than: std::time::Duration) {
    sweep_stale_downloads_in(&staging_dir_path(), older_than);
}

fn sweep_stale_downloads_in(dir: &Path, older_than: std::time::Duration) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let now = std::time::SystemTime::now();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_clip = name.ends_with(".mp4") || name.ends_with(".part") || name.ends_with(".ytdl");
        if !is_clip {
            continue;
        }
        let stale = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age >= older_than);
        if stale {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Reserva `clip-Nombre.mp4`, `clip-Nombre (2).mp4`… con `create_new`
/// (misma disciplina anti-TOCTOU que `reserve_unique_path`).
pub fn reserve_clip_path(folder: &Path, stem: &str) -> Result<PathBuf, std::io::Error> {
    // Un parcial pertenece a su URL: otra descarga no debe ocuparlo. Los
    // parciales se listan UNA vez: re-leer la carpeta por candidato
    // multiplicaba el coste con el staging lleno.
    let partials: Vec<String> = std::fs::read_dir(folder)
        .ok()
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .filter(|name| name.ends_with(".part") || name.ends_with(".ytdl"))
                .collect()
        })
        .unwrap_or_default();
    for n in 0..10_000u32 {
        let name = if n == 0 {
            format!("{stem}.mp4")
        } else {
            format!("{stem} ({}).mp4", n + 1)
        };
        let candidate = folder.join(&name);
        let prefix = format!(
            "{}.",
            candidate.file_stem().unwrap_or_default().to_string_lossy()
        );
        if partials.iter().any(|name| name.starts_with(&prefix)) {
            continue;
        }
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
    fn clip_naming_skips_names_with_resume_partials() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("clip-Joanna.mp4.part"), b"x").unwrap();
        // El parcial pertenece a su URL: la reserva lo salta aunque el .mp4
        // final aún no exista.
        let p = reserve_clip_path(dir.path(), "clip-Joanna").expect("reserva");
        assert_eq!(p, dir.path().join("clip-Joanna (2).mp4"));
    }

    #[test]
    fn preview_fps_caps_frame_count() {
        assert_eq!(preview_fps(0.0), 1.0);
        assert_eq!(preview_fps(30.0), 2.0);
        assert_eq!(preview_fps(120.0), 1.0);
        assert_eq!(preview_fps(600.0), 0.2);
    }

    #[test]
    fn stale_sweep_keeps_fresh_clips_and_drops_old_ones() {
        let dir = tempfile::tempdir().expect("tempdir");
        for name in ["clip-a.mp4", "clip-a.mp4.part", "notes.txt"] {
            std::fs::write(dir.path().join(name), b"x").unwrap();
        }
        // Umbral enorme: nada es viejo, todo sobrevive (el .txt ni siquiera
        // es candidato a descarga).
        sweep_stale_downloads_in(dir.path(), std::time::Duration::from_secs(30 * 24 * 3600));
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 3);
        // Umbral cero: los clips caen; lo que no es descarga sobrevive.
        sweep_stale_downloads_in(dir.path(), std::time::Duration::ZERO);
        assert!(dir.path().join("notes.txt").is_file());
        assert!(!dir.path().join("clip-a.mp4").exists());
        assert!(!dir.path().join("clip-a.mp4.part").exists());
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
    fn download_dir_is_always_the_staging_temp() {
        // El destino ya no depende del contexto: el proyecto nunca recibe el
        // mp4. Estable entre llamadas y siempre bajo el temporal del usuario.
        // `resolve_download_dir` NO crea la carpeta (lo hace el primer
        // download real), así que aquí solo identidad, nunca existencia:
        // en una máquina limpia (CI) staging aún no existe.
        let first = resolve_download_dir();
        let second = resolve_download_dir();
        assert_eq!(first, second);
        assert_eq!(first, staging_dir_path());
        assert!(first
            .file_name()
            .is_some_and(|n| n == "canvas-desktop-downloads"));
    }
}
