//! Helpers de video: localización de `ffmpeg`/`ffprobe` y extracción de
//! poster / dimensiones vía el binario de `yt-dlp\ffmpeg` si existe.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use crate::{IoError, LoadedImage};

/// Los procesos auxiliares de vídeo nunca deben abrir una consola en Windows.
pub fn media_command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    command.stdin(std::process::Stdio::null());
    command
}

/// Variables de entorno que pueden apuntar al directorio de ffmpeg/ffprobe
/// (o al propio ejecutable). Sin rutas fijas en el binario: en un build de
/// release el único origen es el entorno del usuario o el PATH del sistema.
fn env_ffmpeg_dir() -> Option<PathBuf> {
    for key in ["CANVAS_FFMPEG_DIR", "CANVAS_YTDLP_DIR", "YTDLP_PATH"] {
        if let Ok(v) = std::env::var(key) {
            let p = PathBuf::from(&v);
            // Puede ser dir o exe
            if p.is_file() {
                if let Some(parent) = p.parent() {
                    return Some(parent.to_owned());
                }
                return Some(p);
            }
            if p.is_dir() {
                return Some(p);
            }
            // Si es el exe directo
            if v.to_ascii_lowercase().ends_with(".exe") && Path::new(&v).is_file() {
                return Some(PathBuf::from(v));
            }
        }
    }
    None
}

/// Ruta a `ffmpeg`, resuelta una vez por proceso (caché): el fallback a
/// `PATH` lanza `ffmpeg -version` en un proceso hijo y esta función se llama
/// desde varios workers de vídeo. Ver `ytdlp_path` para el porqué del caché.
pub fn ffmpeg_path() -> Option<PathBuf> {
    static CACHE: OnceLock<Option<PathBuf>> = OnceLock::new();
    CACHE.get_or_init(resolve_ffmpeg_path).clone()
}

fn resolve_ffmpeg_path() -> Option<PathBuf> {
    if let Some(dir) = env_ffmpeg_dir() {
        let cand = dir.join("ffmpeg.exe");
        if cand.is_file() {
            return Some(cand);
        }
        let cand2 = dir.join("ffmpeg");
        if cand2.is_file() {
            return Some(cand2);
        }
        // Si el env ya era el exe
        if dir.is_file() {
            return Some(dir);
        }
    }
    // Fallback a PATH
    let probe = media_command("ffmpeg").arg("-version").output();
    if probe.is_ok_and(|o| o.status.success()) {
        return Some(PathBuf::from("ffmpeg"));
    }
    None
}

/// Ruta a `ffprobe`, resuelta una vez por proceso (caché, igual que
/// `ffmpeg_path`).
pub fn ffprobe_path() -> Option<PathBuf> {
    static CACHE: OnceLock<Option<PathBuf>> = OnceLock::new();
    CACHE.get_or_init(resolve_ffprobe_path).clone()
}

fn resolve_ffprobe_path() -> Option<PathBuf> {
    if let Some(dir) = env_ffmpeg_dir() {
        let cand = dir.join("ffprobe.exe");
        if cand.is_file() {
            return Some(cand);
        }
        let cand2 = dir.join("ffprobe");
        if cand2.is_file() {
            return Some(cand2);
        }
        if dir.is_file() {
            // env era ffprobe.exe/ffmpeg.exe -> probar ffprobe en mismo dir
            if let Some(parent) = dir.parent() {
                let c = parent.join("ffprobe.exe");
                if c.is_file() {
                    return Some(c);
                }
            }
        }
    }
    let probe = media_command("ffprobe").arg("-version").output();
    if probe.is_ok_and(|o| o.status.success()) {
        return Some(PathBuf::from("ffprobe"));
    }
    None
}

/// Sondea width/height/duration de un video vía ffprobe. Usa JSON para
/// robustez ante rotación.
pub fn probe_video_size(path: &Path) -> Result<(u32, u32, Option<f64>), IoError> {
    let ffprobe = ffprobe_path().ok_or_else(|| IoError::Message {
        message: "ffprobe not found (tried CANVAS_FFMPEG_DIR/CANVAS_YTDLP_DIR and PATH). \
             Install ffmpeg or set CANVAS_FFMPEG_DIR"
            .to_owned(),
    })?;
    let out = media_command(&ffprobe)
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height,duration,rotate,rotation",
            "-show_entries",
            "stream_side_data=rotation:stream_tags=rotate",
            "-show_entries",
            "format=duration",
            "-of",
            "json",
        ])
        .arg(path)
        .output()
        .map_err(|e| IoError::Io {
            path: path.to_owned(),
            source: e,
        })?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(IoError::Decode {
            path: path.to_owned(),
            source: image::ImageError::IoError(std::io::Error::other(format!(
                "ffprobe failed: {stderr}"
            ))),
        });
    }
    let v: serde_json::Value =
        serde_json::from_slice(&out.stdout).map_err(|e| IoError::Decode {
            path: path.to_owned(),
            source: image::ImageError::IoError(std::io::Error::other(format!("ffprobe json: {e}"))),
        })?;
    let streams = v
        .get("streams")
        .and_then(|s| s.as_array())
        .and_then(|a| a.first());
    let (mut w, mut h) = (0u32, 0u32);
    let mut duration: Option<f64> = None;
    if let Some(s) = streams {
        w = s.get("width").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
        h = s.get("height").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
        if let Some(d) = s
            .get("duration")
            .and_then(|x| x.as_str())
            .and_then(|s| s.parse::<f64>().ok())
        {
            duration = Some(d);
        }
        // rotation tag (puede ser "90", "270", etc en tags)
        let rotate = s
            .get("rotate")
            .and_then(|x| x.as_str())
            .and_then(|s| s.parse::<f64>().ok())
            .or_else(|| {
                s.get("tags")
                    .and_then(|t| t.get("rotate"))
                    .and_then(|x| x.as_str())
                    .and_then(|s| s.parse::<f64>().ok())
            })
            .or_else(|| {
                s.get("rotation")
                    .and_then(|x| x.as_f64().or_else(|| x.as_str()?.parse::<f64>().ok()))
            })
            .or_else(|| {
                s.get("side_data_list")?
                    .as_array()?
                    .iter()
                    .filter_map(|data| data.get("rotation"))
                    .find_map(|x| x.as_f64().or_else(|| x.as_str()?.parse::<f64>().ok()))
            });
        if let Some(r) = rotate {
            let r = r.rem_euclid(360.0);
            if (r - 90.0).abs() < 1.0 || (r - 270.0).abs() < 1.0 {
                std::mem::swap(&mut w, &mut h);
            }
        }
    }
    if duration.is_none() {
        if let Some(d) = v
            .get("format")
            .and_then(|f| f.get("duration"))
            .and_then(|x| x.as_str())
            .and_then(|s| s.parse::<f64>().ok())
        {
            duration = Some(d);
        }
    }
    if w == 0 || h == 0 {
        return Err(IoError::Decode {
            path: path.to_owned(),
            source: image::ImageError::IoError(std::io::Error::other("ffprobe: zero dimensions")),
        });
    }
    Ok((w, h, duration))
}

/// Composición de un lienzo de vídeo para renderizar a mp4: la página de
/// `page_w`×`page_h`, el clip (`src`, tramo `[start_s, end_s]`) colocado en
/// `fg` (píxeles de página) sobre un fondo que cubre la página con blur.
/// Todo en píxeles enteros y pares (x264/yuv420p lo exigen), ya redondeados
/// por el llamador.
pub struct VideoComposition {
    pub src: PathBuf,
    pub start_s: f64,
    pub end_s: Option<f64>,
    pub page_w: u32,
    pub page_h: u32,
    /// El rectángulo puede asomar fuera de la página (el usuario desplazó el
    /// clip): el `overlay` lo recorta igual que el editor, así que van con
    /// signo en vez de recortarse aquí y falsear la composición.
    pub fg_x: i32,
    pub fg_y: i32,
    pub fg_w: u32,
    pub fg_h: u32,
    /// Radio del blur del fondo en px de página (0 = fondo negro).
    pub bg_blur: f32,
    pub dst: PathBuf,
}

/// Lo que devuelve un renderizado: la duración (para resetear el trim, que
/// tras un guardado en el sitio pasa a ser el archivo entero) y el tamaño en
/// píxeles del archivo, que siempre es la página (`page_w`×`page_h`: el fondo
/// sale a ese tamaño y el clip se superpone encima). El llamador lo necesita
/// para re-anclar la capa: sus píxeles ya no son el clip original sino la
/// composición, así que su `natural` pasa a ser la página — si conserva el
/// natural viejo, la reproducción re-escala el archivo al tamaño obsoleto y
/// el clip se ve distorsionado hasta recargar.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VideoRenderInfo {
    pub duration_secs: f64,
    pub width: u32,
    pub height: u32,
}

/// Renderiza la composición a `dst` con ffmpeg y devuelve su duración y su
/// tamaño (siempre la página, ver `VideoRenderInfo`).
///
/// No es un simple corte por tiempo: reproduce lo que se ve en el editor
/// (página + fondo con blur + clip colocado y recortado). El fondo se anima
/// con el propio vídeo (una sola entrada); el editor muestra el póster
/// estático, indistinguible una vez el blur cubre el detalle. Re-codifica
/// H.264 (corte exacto al In/Out) y el audio a AAC tras el mismo recorte
/// (`-c copy` caería en keyframes y desincronizaría). Escritura atómica con
/// sustitución (`replace_file_with_tmp`): vale para guardar en el sitio, el
/// temporal nunca es el archivo que se está leyendo.
pub fn export_video_composition(comp: &VideoComposition) -> Result<VideoRenderInfo, IoError> {
    if comp.page_w < 2 || comp.page_h < 2 || comp.fg_w < 2 || comp.fg_h < 2 {
        return Err(IoError::Message {
            message: format!(
                "invalid video composition: page {}x{} fg {}x{} at ({},{})",
                comp.page_w, comp.page_h, comp.fg_w, comp.fg_h, comp.fg_x, comp.fg_y
            ),
        });
    }
    let ffmpeg = ffmpeg_path().ok_or_else(|| IoError::Message {
        message: "ffmpeg not found (tried CANVAS_FFMPEG_DIR/CANVAS_YTDLP_DIR and PATH). \
             Install ffmpeg or set CANVAS_FFMPEG_DIR"
            .to_owned(),
    })?;
    let src_dur = probe_video_size(&comp.src)?
        .2
        .ok_or_else(|| IoError::Message {
            message: format!("could not probe the duration of {}", comp.src.display()),
        })?;
    let start = comp.start_s.max(0.0);
    let end = comp.end_s.unwrap_or(src_dur);
    if end <= start {
        return Err(IoError::Message {
            message: format!("invalid video trim: end ({end}) is not after start ({start})"),
        });
    }
    // La rama de audio del grafo y su mapeo solo existen con pista real: un
    // `-map [a]` o un `-c:a` huérfanos hacen fallar a ffmpeg. Se sondea una
    // vez (rápido) en vez de intentar/renunciar a ciegas.
    let audio = has_audio_stream(&comp.src);
    let filter = composition_filter(comp, start, end, audio);
    let mut args: Vec<String> = vec![
        "-v".to_owned(),
        "error".to_owned(),
        "-i".to_owned(),
        comp.src.to_string_lossy().into_owned(),
        "-filter_complex".to_owned(),
        filter,
        "-map".to_owned(),
        "[v]".to_owned(),
    ];
    if audio {
        args.extend(["-map".to_owned(), "[a]".to_owned()]);
    }
    args.extend([
        "-c:v".to_owned(),
        "libx264".to_owned(),
        "-preset".to_owned(),
        "veryfast".to_owned(),
        "-crf".to_owned(),
        "18".to_owned(),
        "-pix_fmt".to_owned(),
        "yuv420p".to_owned(),
        "-movflags".to_owned(),
        "+faststart".to_owned(),
        // El temporal atómico lleva una extensión no estándar (`.exporting`):
        // el formato se fija a mano en vez de dejar que ffmpeg lo adivine.
        "-f".to_owned(),
        "mp4".to_owned(),
        "-y".to_owned(),
    ]);
    if audio {
        args.extend([
            "-c:a".to_owned(),
            "aac".to_owned(),
            "-b:a".to_owned(),
            "128k".to_owned(),
        ]);
    }
    let tmp = comp.dst.with_extension("exporting");
    args.push(tmp.to_string_lossy().into_owned());
    let out = media_command(&ffmpeg)
        .args(&args)
        .output()
        .map_err(|e| IoError::Io {
            path: comp.src.clone(),
            source: e,
        })?;
    if !out.status.success() || !tmp.is_file() {
        let _ = std::fs::remove_file(&tmp);
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(IoError::Decode {
            path: comp.src.clone(),
            source: image::ImageError::IoError(std::io::Error::other(format!(
                "ffmpeg composition render failed: {stderr}"
            ))),
        });
    }
    crate::replace_file_with_tmp(&tmp, &comp.dst)?;
    Ok(VideoRenderInfo {
        duration_secs: end - start,
        width: comp.page_w,
        height: comp.page_h,
    })
}

/// Grafo de filtros que reproduce el lienzo: recorte temporal exacto
/// (`trim`+`setpts`, nunca `-c copy`), el clip escalado a su rectángulo, el
/// fondo en cover con blur (o negro generado si no hay) y el audio recortado
/// al mismo intervalo. Pura (testeable sin ffmpeg).
fn composition_filter(comp: &VideoComposition, start: f64, end: f64, has_audio: bool) -> String {
    let (pw, ph) = (comp.page_w, comp.page_h);
    let time = format!("start={start:.3}:end={end:.3}");
    // Sin fondo con blur no hace falta duplicar la entrada: el negro se
    // genera y el clip va directo a su rectángulo.
    let bg = if comp.bg_blur > 0.0 {
        let radius = comp.bg_blur.round().clamp(1.0, 40.0) as u32;
        format!(
            "[0:v]trim={time},setpts=PTS-STARTPTS,split=2[fgsrc][bgsrc];\
             [fgsrc]scale={fw}:{fh}:flags=lanczos[fg];\
             [bgsrc]scale={pw}:{ph}:force_original_aspect_ratio=increase,\
             crop={pw}:{ph},boxblur={radius}:1[bg]",
            fw = comp.fg_w,
            fh = comp.fg_h,
        )
    } else {
        format!(
            "[0:v]trim={time},setpts=PTS-STARTPTS,\
             scale={fw}:{fh}:flags=lanczos[fg];\
             color=c=black:s={pw}x{ph}:d={dur:.3}[bg]",
            fw = comp.fg_w,
            fh = comp.fg_h,
            dur = end - start,
        )
    };
    let mut graph = format!(
        "{bg};[bg][fg]overlay={x}:{y}:format=yuv420[v]",
        x = comp.fg_x,
        y = comp.fg_y,
    );
    if has_audio {
        graph.push_str(&format!(";[0:a]atrim={time},asetpts=PTS-STARTPTS[a]"));
    }
    graph
}

/// ¿El archivo trae alguna pista de audio? (un sondeo rápido: la rama de
/// audio del grafo solo se arma si existe).
fn has_audio_stream(path: &Path) -> bool {
    let Some(ffprobe) = ffprobe_path() else {
        return false;
    };
    media_command(&ffprobe)
        .args([
            "-v",
            "error",
            "-select_streams",
            "a",
            "-show_entries",
            "stream=index",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .output()
        .is_ok_and(|out| out.status.success() && !out.stdout.is_empty())
}

/// Extrae un frame como `LoadedImage` vía `ffmpeg -ss t -i input -vframes 1 png pipe`.
pub fn load_video_frame(path: &Path, time_secs: f64) -> Result<LoadedImage, IoError> {
    let ffmpeg = ffmpeg_path().ok_or_else(|| IoError::Message {
        message: "ffmpeg not found (tried CANVAS_FFMPEG_DIR/CANVAS_YTDLP_DIR and PATH). \
             Install ffmpeg or set CANVAS_FFMPEG_DIR"
            .to_owned(),
    })?;
    let t = time_secs.max(0.0);
    let time_str = format!("{t:.3}");
    let out = media_command(&ffmpeg)
        .args(["-v", "error", "-ss", &time_str, "-i"])
        .arg(path)
        .args([
            "-vframes",
            "1",
            "-q:v",
            "2",
            "-f",
            "image2pipe",
            "-vcodec",
            "png",
            "-",
        ])
        .output()
        .map_err(|e| IoError::Io {
            path: path.to_owned(),
            source: e,
        })?;
    if !out.status.success() || out.stdout.is_empty() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        // Fallback: sin -ss preciso, probar sin seek
        if !stderr.is_empty() {
            return Err(IoError::Decode {
                path: path.to_owned(),
                source: image::ImageError::IoError(std::io::Error::other(format!(
                    "ffmpeg frame extract failed: {stderr}"
                ))),
            });
        }
        return Err(IoError::Decode {
            path: path.to_owned(),
            source: image::ImageError::IoError(std::io::Error::other("ffmpeg: empty frame")),
        });
    }
    let img = image::load_from_memory(&out.stdout).map_err(|e| IoError::Decode {
        path: path.to_owned(),
        source: e,
    })?;
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    Ok(LoadedImage {
        rgba: rgba.into_raw(),
        width: w,
        height: h,
    })
}
