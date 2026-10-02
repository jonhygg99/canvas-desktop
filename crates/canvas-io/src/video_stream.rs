//! Decodificación continua: un proceso FFmpeg por reproducción, sin PNG intermedios.
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Stdio};

use crate::{ffmpeg_path, media_command, IoError, LoadedImage};

pub struct VideoFrameStream {
    child: Child,
    stdout: ChildStdout,
    path: PathBuf,
    size: (u32, u32),
    frame_bytes: usize,
}

impl VideoFrameStream {
    pub fn open(path: &Path, time: f64, size: (u32, u32), fps: u32) -> Result<Self, IoError> {
        let frame_bytes = (size.0 as usize)
            .checked_mul(size.1 as usize)
            .and_then(|n| n.checked_mul(4))
            .filter(|n| *n > 0)
            .ok_or_else(|| IoError::Message {
                message: "Invalid video dimensions".into(),
            })?;
        if fps == 0 || !time.is_finite() {
            return Err(IoError::Message {
                message: "Invalid video frame rate or time".into(),
            });
        }
        let ffmpeg = ffmpeg_path().ok_or_else(|| IoError::Message {
            message: "ffmpeg not found. Install FFmpeg or set CANVAS_FFMPEG_DIR".into(),
        })?;
        let mut child = media_command(ffmpeg)
            .args([
                "-v",
                "error",
                "-nostdin",
                "-ss",
                &format!("{:.6}", time.max(0.0)),
                "-i",
            ])
            .arg(path)
            .args([
                "-map",
                "0:v:0",
                "-an",
                "-sn",
                "-dn",
                "-vf",
                &format!("fps={fps},scale={}:{}", size.0, size.1),
                "-pix_fmt",
                "rgba",
                "-f",
                "rawvideo",
                "pipe:1",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|source| IoError::Io {
                path: path.to_owned(),
                source,
            })?;
        let stdout = child.stdout.take().expect("stdout piped");
        Ok(Self {
            child,
            stdout,
            path: path.to_owned(),
            size,
            frame_bytes,
        })
    }

    pub fn next_frame(&mut self) -> Result<Option<LoadedImage>, IoError> {
        let mut rgba = vec![0; self.frame_bytes];
        // Una lectura inicial distingue EOF limpio de un fotograma truncado.
        let n = self
            .stdout
            .read(&mut rgba[..1])
            .map_err(|source| IoError::Io {
                path: self.path.clone(),
                source,
            })?;
        if n == 0 {
            let status = self.child.wait().map_err(|source| IoError::Io {
                path: self.path.clone(),
                source,
            })?;
            return if status.success() {
                Ok(None)
            } else {
                Err(IoError::Message {
                    message: format!("FFmpeg could not play {} ({status})", self.path.display()),
                })
            };
        }
        self.stdout
            .read_exact(&mut rgba[1..])
            .map_err(|source| IoError::Io {
                path: self.path.clone(),
                source,
            })?;
        Ok(Some(LoadedImage {
            rgba,
            width: self.size.0,
            height: self.size.1,
        }))
    }
}

impl Drop for VideoFrameStream {
    fn drop(&mut self) {
        // Cancela también un decodificador que esté bloqueado escribiendo en pipe.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
