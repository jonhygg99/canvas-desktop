//! Caché de fotogramas extraídos para el editor de vídeo.
//!
//! Extraer la tira de preview (`spawn_ytdlp_frames`) es un pase completo de
//! ffmpeg por el archivo (decenas de segundos en clips largos) que además
//! dejaba su directorio `canvas-frames-*` abandonado en el temporal. Sin
//! caché, cada apertura de «Edit video» repetía el trabajo y la basura se
//! acumulaba sin techo (~230 MB encontrados en una máquina de desarrollo).
//! La caché reutiliza los PNG ya extraídos si el clip no cambió (mismo
//! `mtime`), con techo de entradas (las expulsadas borran su directorio) y
//! barrido de restos antiguos al extraer.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Entradas vivas a la vez (4 clips × ~120 PNG de 320 px ≈ 120 MB como mucho).
const MAX_ENTRIES: usize = 4;
/// Prefijo de los directorios de extracción en el temporal.
const FRAMES_PREFIX: &str = "canvas-frames-";

/// Lo que `VideoEdit::set_frames` + `set_source_fps` necesitan para arrancar
/// sin extraer.
#[derive(Clone)]
pub(crate) struct CachedFrames {
    pub files: Vec<PathBuf>,
    pub fps: f64,
    pub source_fps: f64,
    pub duration: f64,
    pub video_size: Option<(f64, f64)>,
}

struct Entry {
    mtime: SystemTime,
    dir: PathBuf,
    cached: CachedFrames,
}

#[derive(Default)]
pub(crate) struct FramesCache {
    entries: HashMap<PathBuf, Entry>,
    order: VecDeque<PathBuf>,
}

impl FramesCache {
    /// Fotogramas reutilizables para `path`, o `None` (el clip cambió, se
    /// borró o su directorio ya no existe: hay que extraer de nuevo). Las
    /// entradas caducadas se podan con su directorio.
    pub fn get(&mut self, path: &Path) -> Option<CachedFrames> {
        let live = std::fs::metadata(path)
            .and_then(|meta| meta.modified())
            .ok()?;
        let entry = self.entries.get(path)?;
        if entry.mtime != live || !entry.dir.is_dir() {
            self.remove(path);
            return None;
        }
        Some(entry.cached.clone())
    }

    /// Guarda el resultado de una extracción; expulsa (borrando su
    /// directorio) las entradas más viejas por encima del techo.
    pub fn put(&mut self, path: PathBuf, mtime: SystemTime, dir: PathBuf, cached: CachedFrames) {
        self.remove(&path);
        self.order.push_back(path.clone());
        self.entries.insert(path, Entry { mtime, dir, cached });
        while self.order.len() > MAX_ENTRIES {
            if let Some(oldest) = self.order.pop_front() {
                self.remove(&oldest);
            }
        }
    }

    fn remove(&mut self, path: &Path) {
        if let Some(entry) = self.entries.remove(path) {
            self.order.retain(|p| p != path);
            let _ = std::fs::remove_dir_all(&entry.dir);
        }
    }
}

/// Al cerrar la sesión (o la app) no queda nada en el temporal: las
/// entradas vivas borran su directorio. Los `TextureHandle` ya subidos
/// sobreviven en GPU; los PNG solo se releen al abrir otro Edit, que
/// extrae de nuevo si su caché ya no existe.
impl Drop for FramesCache {
    fn drop(&mut self) {
        for entry in self.entries.values() {
            let _ = std::fs::remove_dir_all(&entry.dir);
        }
    }
}

/// Borra del temporal los `canvas-frames-*` más viejos que `older_than`
/// (lo mejor posible, sin fallar). La app lo llama al extraer (hilo worker):
/// limpia los restos de sesiones anteriores sin tocar la extracción en curso
/// (siempre reciente).
pub fn sweep_stale_frame_dirs(older_than: Duration) {
    sweep_stale_frame_dirs_in(&std::env::temp_dir(), older_than);
}

fn sweep_stale_frame_dirs_in(dir: &Path, older_than: Duration) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let now = SystemTime::now();
    for entry in entries.flatten() {
        let name = entry.file_name();
        if name.to_str().is_none_or(|n| !n.starts_with(FRAMES_PREFIX)) {
            continue;
        }
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let stale = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age >= older_than);
        if stale {
            let _ = std::fs::remove_dir_all(&path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(dir: &Path, name: &str) -> (PathBuf, PathBuf, SystemTime) {
        let frames = dir.join(format!("{FRAMES_PREFIX}test"));
        std::fs::create_dir_all(&frames).unwrap();
        let clip = dir.join(name);
        std::fs::write(&clip, b"fake").unwrap();
        let mtime = std::fs::metadata(&clip).unwrap().modified().unwrap();
        (clip, frames, mtime)
    }

    fn cached() -> CachedFrames {
        CachedFrames {
            files: vec![PathBuf::from("frame-001.png")],
            fps: 1.0,
            source_fps: 30.0,
            duration: 10.0,
            video_size: Some((320.0, 180.0)),
        }
    }

    #[test]
    fn reusa_si_el_clip_no_cambio() {
        let dir = tempfile::tempdir().unwrap();
        let clip = dir.path().join("clip.mp4");
        std::fs::write(&clip, b"fake").unwrap();
        let frames = dir.path().join(format!("{FRAMES_PREFIX}test"));
        std::fs::create_dir_all(&frames).unwrap();
        let mtime = std::fs::metadata(&clip).unwrap().modified().unwrap();
        let mut cache = FramesCache::default();
        cache.put(
            clip.clone(),
            mtime,
            frames,
            CachedFrames {
                files: vec![PathBuf::from("frame-001.png")],
                fps: 1.0,
                source_fps: 30.0,
                duration: 10.0,
                video_size: Some((320.0, 180.0)),
            },
        );
        let hit = cache.get(&clip).expect("debería reutilizar");
        assert_eq!(hit.duration, 10.0);
        assert_eq!(hit.files.len(), 1);
    }

    #[test]
    fn caduca_si_el_clip_cambio() {
        let dir = tempfile::tempdir().unwrap();
        let (clip, frames, _) = fixture(dir.path(), "clip.mp4");
        let mut cache = FramesCache::default();
        // `mtime` deliberadamente viejo: el archivo real es más nuevo.
        cache.put(
            clip.clone(),
            SystemTime::UNIX_EPOCH,
            frames.clone(),
            cached(),
        );
        assert!(cache.get(&clip).is_none(), "mtime distinto = extraer");
        assert!(!frames.is_dir(), "la entrada caducada borra su dir");
    }

    #[test]
    fn caduca_si_el_directorio_desaparecio() {
        let dir = tempfile::tempdir().unwrap();
        let (clip, frames, mtime) = fixture(dir.path(), "clip.mp4");
        let mut cache = FramesCache::default();
        cache.put(clip.clone(), mtime, frames.clone(), cached());
        std::fs::remove_dir_all(&frames).unwrap();
        assert!(cache.get(&clip).is_none());
    }

    #[test]
    fn el_techo_expulsa_borrando_directorios() {
        let dir = tempfile::tempdir().unwrap();
        let mut cache = FramesCache::default();
        let mut first_dir = PathBuf::new();
        for i in 0..MAX_ENTRIES + 2 {
            let clip = dir.path().join(format!("clip{i}.mp4"));
            std::fs::write(&clip, b"fake").unwrap();
            let frames = dir.path().join(format!("{FRAMES_PREFIX}{i}"));
            std::fs::create_dir_all(&frames).unwrap();
            if i == 0 {
                first_dir = frames.clone();
            }
            let mtime = std::fs::metadata(&clip).unwrap().modified().unwrap();
            cache.put(
                clip,
                mtime,
                frames,
                CachedFrames {
                    files: vec![],
                    fps: 1.0,
                    source_fps: 30.0,
                    duration: 10.0,
                    video_size: None,
                },
            );
        }
        assert_eq!(cache.entries.len(), MAX_ENTRIES);
        assert!(!first_dir.is_dir(), "la expulsada borra su dir");
    }

    #[test]
    fn el_barrido_solo_toca_canvas_frames_viejos() {
        let dir = tempfile::tempdir().unwrap();
        let stale = dir.path().join(format!("{FRAMES_PREFIX}old"));
        let fresh = dir.path().join(format!("{FRAMES_PREFIX}new"));
        let other = dir.path().join("other-dir");
        let file = dir.path().join(format!("{FRAMES_PREFIX}file"));
        std::fs::create_dir_all(&stale).unwrap();
        std::fs::create_dir_all(&fresh).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(&file, b"x").unwrap();
        sweep_stale_frame_dirs_in(dir.path(), Duration::ZERO);
        assert!(!stale.exists());
        assert!(!fresh.exists(), "con umbral cero todo canvas-frames cae");
        assert!(other.is_dir());
        assert!(file.is_file(), "los archivos no se tocan");
    }
}
