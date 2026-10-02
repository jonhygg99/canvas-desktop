//! Helpers de video: localización de `ffmpeg`/`ffprobe` y extracción de
//! poster / dimensiones vía el binario de `yt-dlp\ffmpeg` si existe.

use std::path::{Path, PathBuf};
use std::process::Command;

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

const HARDCODED_FFMPEG_DIR: &str = r"C:\Users\jonhy\Documents\code-projects\yt-dlp";
const HARDCODED_FFMPEG_EXE: &str = r"C:\Users\jonhy\Documents\code-projects\yt-dlp\ffmpeg.exe";
const HARDCODED_FFPROBE_EXE: &str = r"C:\Users\jonhy\Documents\code-projects\yt-dlp\ffprobe.exe";

fn exe_exists(p: &str) -> bool {
    Path::new(p).is_file()
}

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

pub fn ffmpeg_path() -> Option<PathBuf> {
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
    if exe_exists(HARDCODED_FFMPEG_EXE) {
        return Some(PathBuf::from(HARDCODED_FFMPEG_EXE));
    }
    if exe_exists(&format!("{HARDCODED_FFMPEG_DIR}\\ffmpeg")) {
        return Some(PathBuf::from(format!("{HARDCODED_FFMPEG_DIR}\\ffmpeg")));
    }
    // Fallback a PATH
    let probe = media_command("ffmpeg").arg("-version").output();
    if probe.is_ok_and(|o| o.status.success()) {
        return Some(PathBuf::from("ffmpeg"));
    }
    None
}

pub fn ffprobe_path() -> Option<PathBuf> {
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
            // env era ffmpeg.exe -> probar ffprobe en mismo dir
            if let Some(parent) = dir.parent() {
                let c = parent.join("ffprobe.exe");
                if c.is_file() {
                    return Some(c);
                }
            }
        }
    }
    if exe_exists(HARDCODED_FFPROBE_EXE) {
        return Some(PathBuf::from(HARDCODED_FFPROBE_EXE));
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
        message: format!(
            "ffprobe not found (tried {} and PATH). Install ffmpeg or set CANVAS_FFMPEG_DIR",
            HARDCODED_FFPROBE_EXE
        ),
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
                    .and_then(|x| x.as_str())
                    .and_then(|s| s.parse::<f64>().ok())
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

/// Extrae un frame como `LoadedImage` vía `ffmpeg -ss t -i input -vframes 1 png pipe`.
pub fn load_video_frame(path: &Path, time_secs: f64) -> Result<LoadedImage, IoError> {
    let ffmpeg = ffmpeg_path().ok_or_else(|| IoError::Message {
        message: format!(
            "ffmpeg not found (tried {} and PATH). Install ffmpeg or set CANVAS_FFMPEG_DIR",
            HARDCODED_FFMPEG_EXE
        ),
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
