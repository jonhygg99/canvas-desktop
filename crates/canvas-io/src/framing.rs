//! Sidecars portables de Flashcut-Auto; ausente e inválido son estados distintos.
use crate::IoError;
use canvas_core::framing::{Framing, HEIGHT, WIDTH};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Sidecar {
    schema_version: u32,
    width: u32,
    height: u32,
    framing: Framing,
}

pub fn framing_path(asset: &Path) -> Result<PathBuf, IoError> {
    let name = asset.file_name().ok_or_else(|| IoError::Message {
        message: "Framing requires a file name".into(),
    })?;
    let mut file = name.to_os_string();
    file.push(".json");
    Ok(asset
        .parent()
        .unwrap_or(Path::new("."))
        .join(".framing")
        .join(file))
}

pub fn read_framing(asset: &Path) -> Result<Option<Framing>, IoError> {
    let path = framing_path(asset)?;
    let file = match std::fs::File::open(&path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(IoError::Io { path, source }),
    };
    use std::io::Read;
    let mut bytes = Vec::new();
    file.take(65537)
        .read_to_end(&mut bytes)
        .map_err(|source| IoError::Io {
            path: path.clone(),
            source,
        })?;
    let error = |message: String| IoError::Message {
        message: format!("Invalid framing {}: {message}", path.display()),
    };
    if bytes.len() > 65536 {
        return Err(error("file is too large".into()));
    }
    let sc: Sidecar = serde_json::from_slice(&bytes).map_err(|e| error(e.to_string()))?;
    if sc.schema_version != 1 || sc.width != WIDTH || sc.height != HEIGHT || !sc.framing.is_valid()
    {
        return Err(error(
            "expected v1, 1080 × 1920 and valid position/scale".into(),
        ));
    }
    Ok(Some(sc.framing))
}

pub fn write_framing(asset: &Path, framing: Framing) -> Result<PathBuf, IoError> {
    if !framing.is_valid() {
        return Err(IoError::Message {
            message: "Invalid framing position or scale".into(),
        });
    }
    let path = framing_path(asset)?;
    let bytes = serde_json::to_vec_pretty(&Sidecar {
        schema_version: 1,
        width: WIDTH,
        height: HEIGHT,
        framing,
    })
    .map_err(|e| IoError::Message {
        message: e.to_string(),
    })?;
    let dir = path.parent().expect("framing directory");
    std::fs::create_dir_all(dir).map_err(|source| IoError::Io {
        path: dir.to_owned(),
        source,
    })?;
    crate::write_atomic(&path, &bytes)?;
    Ok(path)
}

#[cfg(test)]
#[path = "framing_tests.rs"]
mod tests;
