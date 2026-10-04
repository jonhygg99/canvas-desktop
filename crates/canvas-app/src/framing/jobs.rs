//! Lecturas, blur y escrituras fuera del hilo de UI; cada sesión posee su canal.
use canvas_core::framing::Framing;
use eframe::egui;
use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver},
};

pub(super) enum Outcome {
    Loaded {
        source: canvas_io::LoadedImage,
        background: egui::ColorImage,
        framing: Option<Framing>,
        error: Option<String>,
        video: Option<super::video::Video>,
    },
    Saved {
        value: Framing,
        path: PathBuf,
        sidecar_only: bool,
    },
    Cancelled,
}

fn spawn(
    ctx: egui::Context,
    job: impl FnOnce() -> Result<Outcome, String> + Send + 'static,
) -> Receiver<Result<Outcome, String>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let result = job();
        let _ = tx.send(result);
        ctx.request_repaint();
    });
    rx
}

fn prepared(path: Option<PathBuf>, source: canvas_io::LoadedImage) -> Result<Outcome, String> {
    let video = if let Some(path) = path.as_ref().filter(|p| canvas_io::is_video_file(p)) {
        let (w, h, duration) = canvas_io::probe_video_size(path).map_err(|e| e.to_string())?;
        let duration = duration
            .filter(|d| d.is_finite() && *d > 0.0)
            .ok_or("Could not read the video duration")?;
        Some(super::video::Video::new(path.clone(), (w, h), duration))
    } else {
        None
    };
    let (framing, error) = match path.as_deref().map(canvas_io::read_framing) {
        Some(Ok(framing)) => (framing, None),
        Some(Err(e)) => (None, Some(e.to_string())),
        None => (None, None),
    };
    let background = super::preview::background(&source)?;
    Ok(Outcome::Loaded {
        source,
        background,
        framing,
        error,
        video,
    })
}

pub(super) fn load(path: PathBuf, ctx: egui::Context) -> Receiver<Result<Outcome, String>> {
    spawn(ctx, move || {
        let source = canvas_io::thumbnail(&path, 1280, None).map_err(|e| e.to_string())?;
        prepared(Some(path), source)
    })
}

pub(super) fn prepare(
    path: Option<PathBuf>,
    source: canvas_io::LoadedImage,
    ctx: egui::Context,
) -> Receiver<Result<Outcome, String>> {
    spawn(ctx, move || prepared(path, source))
}

pub(super) fn save(
    path: PathBuf,
    value: Framing,
    ctx: egui::Context,
) -> Receiver<Result<Outcome, String>> {
    spawn(ctx, move || {
        let path = canvas_io::write_framing(&path, value).map_err(|e| e.to_string())?;
        Ok(Outcome::Saved {
            value,
            path,
            sidecar_only: true,
        })
    })
}

pub(super) fn export(
    source: canvas_io::LoadedImage,
    value: Framing,
    suggested: Option<PathBuf>,
    ctx: egui::Context,
) -> Receiver<Result<Outcome, String>> {
    spawn(ctx, move || {
        let mut dialog = rfd::FileDialog::new()
            .add_filter("PNG", &["png"])
            .set_file_name("Short.png");
        if let Some(path) = &suggested {
            if let Some(parent) = path.parent() {
                dialog = dialog.set_directory(parent);
            }
            if let Some(stem) = path.file_stem() {
                dialog = dialog.set_file_name(format!("{}-short.png", stem.to_string_lossy()));
            }
        }
        let Some(path) = dialog.save_file() else {
            return Ok(Outcome::Cancelled);
        };
        let path = path.with_extension("png");
        export_pair(&path, source, value)?;
        Ok(Outcome::Saved {
            value,
            path,
            sidecar_only: false,
        })
    })
}

pub(crate) fn export_pair(
    path: &std::path::Path,
    source: canvas_io::LoadedImage,
    value: Framing,
) -> Result<(), String> {
    use std::fs::OpenOptions;
    let sidecar = canvas_io::framing_path(path).map_err(|e| e.to_string())?;
    if sidecar.exists() {
        return Err("A framing already exists for that name. Choose a new name.".into());
    }
    // Reservar evita sobrescribir una imagen o una exportación de otra ventana.
    let reservation = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| format!("Choose a new output name: {e}"))?;
    drop(reservation);
    let reserved_sidecar = (|| {
        std::fs::create_dir_all(sidecar.parent().ok_or("Invalid framing output directory")?)
            .map_err(|e| e.to_string())?;
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&sidecar)
            .map_err(|e| format!("Choose a new framing output name: {e}"))
    })();
    match reserved_sidecar {
        Ok(reservation) => drop(reservation),
        Err(error) => {
            let _ = std::fs::remove_file(path);
            return Err(error);
        }
    }
    let result = canvas_io::save_rgba(path, source.rgba, source.width, source.height, 92, None)
        .and_then(|_| canvas_io::write_framing(path, value))
        .map_err(|e| e.to_string());
    if result.is_err() {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(sidecar);
    }
    result.map(|_| ())
}
