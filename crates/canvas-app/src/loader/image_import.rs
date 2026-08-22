//! Añadir una imagen como capa nueva, o reemplazar la de una capa existente
//! (desde disco o descargada de una URL).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::Sender;
use std::time::{SystemTime, UNIX_EPOCH};

use canvas_core::LayerId;
use eframe::egui;

use super::AppMsg;

/// Como `spawn_load_image`, pero el resultado se añade como capa nueva al
/// documento abierto en vez de sustituirlo.
pub fn spawn_load_image_as_layer(path: PathBuf, tx: Sender<AppMsg>, ctx: egui::Context) {
    std::thread::spawn(move || {
        let result = canvas_io::load_image(&path).map_err(|e| e.to_string());
        let _ = tx.send(AppMsg::ImageLoadedForLayer { path, result });
        ctx.request_repaint();
    });
}

pub fn spawn_pick_replacement_image(
    layer: LayerId,
    start_dir: Option<PathBuf>,
    tx: Sender<AppMsg>,
    ctx: egui::Context,
) {
    std::thread::spawn(move || {
        let mut dialog = rfd::FileDialog::new()
            .set_title("Replace image")
            .add_filter("Images", canvas_io::IMAGE_EXTENSIONS);
        if let Some(dir) = start_dir.filter(|dir| dir.is_dir()) {
            dialog = dialog.set_directory(dir);
        }
        if let Some(path) = dialog.pick_file() {
            let label = image_label_from_path(&path);
            let result = canvas_io::load_image(&path).map_err(|e| e.to_string());
            let _ = tx.send(AppMsg::ImageLoadedForReplace {
                layer,
                label,
                source_path: Some(path),
                result,
            });
        }
        ctx.request_repaint();
    });
}

pub fn spawn_load_replacement_image_from_url(
    layer: LayerId,
    url: String,
    tx: Sender<AppMsg>,
    ctx: egui::Context,
) {
    std::thread::spawn(move || {
        let label = image_label_from_url(&url);
        let result = download_url_to_temp(&url).and_then(|path| {
            let loaded = canvas_io::load_image(&path).map_err(|e| e.to_string());
            let _ = std::fs::remove_file(&path);
            loaded
        });
        let _ = tx.send(AppMsg::ImageLoadedForReplace {
            layer,
            label,
            source_path: None,
            result,
        });
        ctx.request_repaint();
    });
}

fn image_label_from_path(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| "Image".to_owned())
}

fn image_label_from_url(url: &str) -> String {
    url.split(['?', '#'])
        .next()
        .and_then(|clean| clean.rsplit('/').next())
        .map(Path::new)
        .map(image_label_from_path)
        .filter(|name| name != "Image")
        .unwrap_or_else(|| "Internet image".to_owned())
}

fn extension_from_url(url: &str) -> &str {
    let clean = url.split(['?', '#']).next().unwrap_or(url);
    let ext = clean
        .rsplit('/')
        .next()
        .and_then(|name| Path::new(name).extension());
    ext.and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .filter(|value| canvas_io::IMAGE_EXTENSIONS.contains(&value.as_str()))
        .map(|value| match value.as_str() {
            "jpg" => "jpg",
            "jpeg" => "jpeg",
            "webp" => "webp",
            "gif" => "gif",
            "bmp" => "bmp",
            "svg" => "svg",
            _ => "png",
        })
        .unwrap_or("png")
}

fn download_url_to_temp(url: &str) -> Result<PathBuf, String> {
    let trimmed = url.trim();
    if !(trimmed.starts_with("http://") || trimmed.starts_with("https://")) {
        return Err("Use an http:// or https:// image URL".to_owned());
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis();
    let path = std::env::temp_dir().join(format!(
        "canvas-desktop-replace-{stamp}.{}",
        extension_from_url(trimmed)
    ));
    let out = path.to_string_lossy().into_owned();

    // `-Command` no vincula argumentos extra del proceso a `$args` (eso solo
    // pasa con `-File script.ps1 arg1 arg2`) — se pegarían sueltos al final
    // del script, dejando `$args[0]`/`$args[1]` siempre en `$null`. La URL y
    // la ruta van incrustadas como literales `'...'` del propio comando, con
    // la comilla simple escapada duplicándola (`''`), la forma estándar de
    // PowerShell — a diferencia de comillas dobles, no interpola variables.
    #[cfg(windows)]
    let output = {
        use std::os::windows::process::CommandExt;
        // `CREATE_NO_WINDOW`: sin esto, `powershell.exe` (subsistema de
        // consola) abre y parpadea su propia ventana un instante aunque
        // este binario sea GUI.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let escaped_url = trimmed.replace('\'', "''");
        let escaped_out = out.replace('\'', "''");
        Command::new("powershell")
            .arg("-NoProfile")
            .arg("-Command")
            .arg(format!(
                "Invoke-WebRequest -Uri '{escaped_url}' -OutFile '{escaped_out}'"
            ))
            .creation_flags(CREATE_NO_WINDOW)
            .output()
    };

    #[cfg(not(windows))]
    let output = Command::new("curl")
        .arg("--location")
        .arg("--fail")
        .arg("--silent")
        .arg("--show-error")
        .arg("--output")
        .arg(&out)
        .arg(trimmed)
        .output();

    let output = output.map_err(|e| format!("Could not start downloader: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let message = if stderr.is_empty() {
            format!("Downloader exited with {}", output.status)
        } else {
            stderr
        };
        let _ = std::fs::remove_file(&path);
        return Err(message);
    }
    Ok(path)
}
