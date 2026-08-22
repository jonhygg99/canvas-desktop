//! Diseños `.canvas`: sidecar de una imagen o diseño autónomo.
//!
//! El mismo formato de archivo sirve para dos papeles, discriminados por un
//! único campo: junto a `foto.png` se escribe (hoy en `.canvas/foto.png.canvas`,
//! ver más abajo) un sidecar con `image_hash: Some(..)` (preserva la
//! editabilidad — el PNG/JPEG se sobrescribe al guardar, así que sus capas no
//! se pueden recuperar de disco); un diseño nacido en la galería es un
//! `.canvas` autónomo con `image_hash: None`, sin ningún archivo de imagen
//! del que depender. En ambos casos los píxeles de cada capa van embebidos
//! como PNG en base64, y también una miniatura de la página (`preview_png`,
//! solo en diseños autónomos — ver su doc) para que la galería pinte algo
//! sin tener GPU en su hilo de miniaturas.
//!
//! Al reabrir un sidecar de imagen, si el hash coincide se restauran las
//! capas editables; si no (alguien la editó por fuera), el llamador avisa y
//! deja elegir. Un diseño autónomo no tiene nada que contrastar.
//!
//! **Ubicación del sidecar de una imagen** (no aplica a un diseño autónomo,
//! que es un archivo cualquiera con el nombre que el usuario le dio): vive en
//! `<carpeta>/.canvas/foto.png.canvas`, no como hermano directo de la imagen.
//! `.canvas/` se crea oculta (`FILE_ATTRIBUTE_HIDDEN` en Windows; el prefijo
//! `.` no oculta nada ahí, a diferencia de Unix) la primera vez que hace
//! falta. `find_sidecar` sigue leyendo el hermano clásico
//! (`foto.png.canvas`) de una carpeta migrada solo a medias: cualquier
//! sidecar escrito por esta versión aterriza en `.canvas/`, y el próximo
//! guardado de uno legacy lo borra de su sitio antiguo.

use std::path::{Path, PathBuf};

use canvas_core::Document;
use serde::{Deserialize, Serialize};

use crate::{write_atomic, IoError, LoadedImage};

/// Versión del formato. v2 añadió capas de texto/forma/SVG; v3 añade capas
/// de grupo (`LayerContent::Group`, ilegible para un build v2); v4 hace
/// `image_hash` opcional (diseño autónomo) y añade `preview_png`. Los
/// sidecar v1/v2/v3 se siguen leyendo sin migración (los campos nuevos
/// tienen serde(default)); `parent_id` en particular es `serde(default)` así
/// que todo lo anterior abre como raíz de la pila.
const SIDECAR_VERSION: u32 = 4;

/// Lado mayor de la miniatura embebida en un `.canvas`. Coincide con el
/// `max_dim` que pide la galería, así que el redimensionado posterior en
/// `thumbnail()` es normalmente un no-op.
pub const PREVIEW_MAX_DIM: u32 = 256;

/// Solo el campo `version`, para decidir si merece la pena parsear el resto
/// del archivo. Parsear `SidecarFile` directamente ante un sidecar más nuevo
/// (con variantes de capa que este build no conoce) fallaría con un genérico
/// "corrupt sidecar" en vez del mensaje amable de versión.
#[derive(Debug, Deserialize)]
struct VersionProbe {
    version: u32,
}

/// Como `VersionProbe`, pero también se queda con la miniatura: permite leer
/// solo eso sin decodificar los píxeles de cada capa (lo que pide la
/// galería, por cada celda, en un hilo sin GPU).
#[derive(Debug, Deserialize)]
struct PreviewProbe {
    #[serde(default)]
    preview_png: Option<String>,
}

/// Como `PreviewProbe`, pero para el tamaño de página: se queda solo con
/// `document.pages[0].{width,height}`, sin decodificar ni la miniatura ni
/// los píxeles de ninguna capa. `serde_json` sigue tokenizando el archivo
/// entero (no hay forma de saltar directamente a un campo en JSON), pero
/// ignora — sin asignar — todo lo que esta forma no declara.
#[derive(Debug, Deserialize)]
struct PageProbe {
    document: DocumentPageProbe,
}

#[derive(Debug, Deserialize)]
struct DocumentPageProbe {
    pages: Vec<PageSizeProbe>,
}

#[derive(Debug, Deserialize)]
struct PageSizeProbe {
    width: f64,
    height: f64,
}

/// Píxeles de una capa a embeber: (id crudo, RGBA, ancho, alto).
pub type LayerPixels = (u64, Vec<u8>, u32, u32);

/// Nombre de la carpeta oculta donde viven los sidecar de una carpeta de
/// imágenes.
pub const SIDECAR_DIR: &str = ".canvas";

/// Carpeta de sidecars de `folder` (no crea nada: solo compone la ruta).
pub fn sidecar_dir(folder: &Path) -> PathBuf {
    folder.join(SIDECAR_DIR)
}

/// Ruta ACTUAL del sidecar de una imagen — dentro de `.canvas/`, no como
/// hermano directo. Es la única ruta que usan los llamadores que van a
/// ESCRIBIR (`write_sidecar`, y quien vaya a copiar/renombrar un sidecar ya
/// migrado); para LEER, usar `find_sidecar`, que también contempla el
/// hermano legacy.
pub fn sidecar_path(image_path: &Path) -> PathBuf {
    let folder = image_path.parent().unwrap_or_else(|| Path::new(""));
    sidecar_dir(folder).join(sidecar_file_name(image_path))
}

/// Ruta LEGACY del sidecar (hermano directo: `foto.png` → `foto.png.canvas`),
/// de antes de que los sidecar se escondieran en `.canvas/`. Solo para lectura
/// de compatibilidad — nunca se escribe un sidecar nuevo aquí.
fn legacy_sidecar_path(image_path: &Path) -> PathBuf {
    let mut name = image_path.as_os_str().to_owned();
    name.push(".");
    name.push(crate::CANVAS_EXTENSION);
    PathBuf::from(name)
}

fn sidecar_file_name(image_path: &Path) -> String {
    let mut name = image_path
        .file_name()
        .map(|n| n.to_owned())
        .unwrap_or_default();
    name.push(".");
    name.push(crate::CANVAS_EXTENSION);
    name.to_string_lossy().into_owned()
}

/// Sidecar existente de `image_path`, si lo hay: primero la ubicación actual
/// (`.canvas/foto.png.canvas`), luego el hermano legacy
/// (`foto.png.canvas`) para no perder de vista lo que una versión anterior ya
/// había escrito. `None` si ninguno de los dos existe.
pub fn find_sidecar(image_path: &Path) -> Option<PathBuf> {
    let current = sidecar_path(image_path);
    if current.is_file() {
        return Some(current);
    }
    let legacy = legacy_sidecar_path(image_path);
    legacy.is_file().then_some(legacy)
}

/// Crea `<folder>/.canvas` si no existe y, en Windows, la marca oculta con
/// `FILE_ATTRIBUTE_HIDDEN` — el prefijo `.` del nombre no oculta nada ahí, a
/// diferencia de Unix. Solo marca el atributo justo cuando ACABA de crear la
/// carpeta: si el usuario la hizo visible a mano después, esta función no se
/// lo revierte en cada guardado.
pub fn ensure_sidecar_dir(folder: &Path) -> Result<PathBuf, IoError> {
    let dir = sidecar_dir(folder);
    match std::fs::create_dir(&dir) {
        Ok(()) => {
            hide_dir(&dir);
            Ok(dir)
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(dir),
        Err(source) => Err(IoError::Write {
            path: dir,
            message: format!("creando la carpeta de diseños: {source}"),
        }),
    }
}

/// Mejor esfuerzo: no ocultar la carpeta nunca es motivo para fallar el
/// guardado, solo un fastidio visual.
#[cfg(windows)]
fn hide_dir(dir: &Path) {
    use windows::core::HSTRING;
    use windows::Win32::Storage::FileSystem::{SetFileAttributesW, FILE_ATTRIBUTE_HIDDEN};
    let result =
        unsafe { SetFileAttributesW(&HSTRING::from(dir.as_os_str()), FILE_ATTRIBUTE_HIDDEN) };
    if let Err(e) = result {
        tracing::warn!("no se pudo ocultar {}: {e}", dir.display());
    }
}

#[cfg(not(windows))]
fn hide_dir(_dir: &Path) {
    // El prefijo `.` del nombre ya la oculta por convención Unix.
}

/// Papelera propia del proyecto — deshacer un borrado ("Delete" del editor)
/// mueve el archivo aquí en vez de a la del sistema operativo: un
/// `std::fs::rename` no depende de ninguna API de plataforma (a diferencia
/// de restaurar de la papelera real, que en el crate `trash` solo funciona
/// en Windows/Linux, no en macOS). Vive dentro de `.canvas/`, que ya se
/// esconde entera — no hace falta ocultarla aparte.
pub fn trash_dir(folder: &Path) -> PathBuf {
    sidecar_dir(folder).join("trash")
}

/// Ruta que tendría `original` si estuviera en la papelera del proyecto:
/// mismo nombre de archivo, sin desambiguar — un archivo no puede estar
/// borrado dos veces a la vez en la misma carpeta (la segunda vez ya lo
/// habría movido o purgado la primera), así que no hace falta un sufijo.
pub fn local_trash_path(original: &Path) -> PathBuf {
    let folder = original.parent().unwrap_or_else(|| Path::new(""));
    let name = original.file_name().unwrap_or_default();
    trash_dir(folder).join(name)
}

/// Mueve `path` a la papelera del proyecto (creándola si hace falta) y
/// devuelve dónde quedó — deshacible con `restore_from_local_trash`.
pub fn move_to_local_trash(path: &Path) -> Result<PathBuf, IoError> {
    let folder = path.parent().unwrap_or_else(|| Path::new(""));
    ensure_sidecar_dir(folder)?;
    let dir = trash_dir(folder);
    if let Err(source) = std::fs::create_dir(&dir) {
        if source.kind() != std::io::ErrorKind::AlreadyExists {
            return Err(IoError::Write {
                path: dir,
                message: format!("creando la papelera del proyecto: {source}"),
            });
        }
    }
    let staged = local_trash_path(path);
    std::fs::rename(path, &staged).map_err(|source| IoError::Write {
        path: path.to_path_buf(),
        message: format!("moviendo a la papelera del proyecto: {source}"),
    })?;
    Ok(staged)
}

/// Deshace `move_to_local_trash`: mueve `staged` de vuelta a `original`.
/// Rechaza si `original` ya existe (alguien creó otro archivo con ese mismo
/// nombre mientras tanto) en vez de sobrescribirlo en silencio — mismo
/// criterio que `rename_with_sidecar` en `canvas-app`.
pub fn restore_from_local_trash(staged: &Path, original: &Path) -> Result<(), IoError> {
    if original.exists() {
        return Err(IoError::Write {
            path: original.to_path_buf(),
            message: "a file already exists at the original location".to_owned(),
        });
    }
    std::fs::rename(staged, original).map_err(|source| IoError::Write {
        path: staged.to_path_buf(),
        message: format!("restaurando desde la papelera del proyecto: {source}"),
    })
}

/// Borra de verdad (mejor esfuerzo, sin fallar si `trash/` no existe o algún
/// archivo ya no está) todo lo que quedó en la papelera del proyecto sin
/// deshacer — se llama al salir de la carpeta (galería/proyecto) y al
/// volver a entrar en ella, para que un `Ctrl+Z` que nunca llegó, o los
/// restos de una sesión que se cerró en falso, no se queden para siempre.
pub fn purge_local_trash(folder: &Path) {
    let dir = trash_dir(folder);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if let Err(e) = std::fs::remove_file(&path) {
            tracing::warn!("no se pudo purgar {} de la papelera: {e}", path.display());
        }
    }
}

/// FNV-1a de 64 bits: determinista entre ejecuciones y versiones de Rust
/// (el `DefaultHasher` de std no lo garantiza).
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[derive(Debug, Serialize, Deserialize)]
struct SidecarImage {
    layer: u64,
    png_base64: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct SidecarFile {
    version: u32,
    /// FNV-1a 64 (hex) de los bytes de la imagen que acompaña. `None` en un
    /// diseño autónomo: el `.canvas` ES el documento, no hay imagen que
    /// contrastar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    image_hash: Option<String>,
    /// Id crudo de la capa de «fondo desenfocado», si estaba activa.
    #[serde(default)]
    background_layer: Option<u64>,
    /// Miniatura de la página (PNG en base64, lado mayor ≤ `PREVIEW_MAX_DIM`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    preview_png: Option<String>,
    document: Document,
    images: Vec<SidecarImage>,
}

/// Todo lo que se embebe en un `.canvas`, sea sidecar de imagen o diseño
/// autónomo. Dueño de sus datos porque cruza a un hilo de trabajo.
pub struct CanvasPayload {
    pub document: Document,
    pub images: Vec<LayerPixels>,
    pub background_layer: Option<u64>,
    /// Miniatura ya reducida a `PREVIEW_MAX_DIM` (ver `make_preview`).
    /// `None` si el horneado en GPU falló: el diseño se guarda igual.
    pub preview: Option<LoadedImage>,
}

/// Documento restaurado desde un `.canvas`, con los píxeles ya decodificados.
pub struct RestoredDocument {
    pub document: Document,
    /// (id crudo de capa, píxeles RGBA decodificados)
    pub images: Vec<(u64, LoadedImage)>,
    pub background_layer: Option<u64>,
    /// false si la imagen que acompaña cambió por fuera desde el último
    /// guardado. Siempre `true` en un diseño autónomo: no hay nada que
    /// contrastar.
    pub hash_matches: bool,
    /// El `.canvas` no acompaña a ninguna imagen: `Ctrl+S` lo reescribe tal
    /// cual, sin rasterizar.
    pub standalone: bool,
}

/// Escala a la que hornear la página para obtener la miniatura embebida:
/// nunca agranda, y el lado mayor queda en `PREVIEW_MAX_DIM`.
pub fn preview_scale(page_w: f64, page_h: f64) -> f64 {
    let max = page_w.max(page_h).max(1.0);
    (f64::from(PREVIEW_MAX_DIM) / max).min(1.0)
}

/// Reduce un RGBA ya horneado (a cualquier escala) a la miniatura embebida.
/// `None` si `rgba` no coincide con `width × height × 4`.
pub fn make_preview(rgba: &[u8], width: u32, height: u32) -> Option<LoadedImage> {
    let src = image::RgbaImage::from_raw(width, height, rgba.to_vec())?;
    let (tw, th) = crate::thumbs::fit_within(width, height, PREVIEW_MAX_DIM);
    let thumb = image::imageops::thumbnail(&src, tw, th);
    Some(LoadedImage {
        rgba: thumb.into_raw(),
        width: tw,
        height: th,
    })
}

/// Documento en blanco listo para escribir en disco (`write_design`), con
/// una miniatura de fondo sólido ya sintetizada para que la celda de la
/// galería no salga vacía antes del primer guardado real.
pub fn blank_design(width: f64, height: f64) -> CanvasPayload {
    let mut document = Document::new(width, height);
    if let Ok(page) = document.page_mut() {
        page.background = Some([255, 255, 255, 255]);
    }
    let scale = preview_scale(width, height);
    let pw = ((width * scale).round() as u32).max(1);
    let ph = ((height * scale).round() as u32).max(1);
    let preview = Some(LoadedImage {
        rgba: vec![255u8; pw as usize * ph as usize * 4],
        width: pw,
        height: ph,
    });
    CanvasPayload {
        document,
        images: Vec::new(),
        background_layer: None,
        preview,
    }
}

fn encode_payload(
    path: &Path,
    image_hash: Option<String>,
    payload: &CanvasPayload,
) -> Result<Vec<u8>, IoError> {
    let mut encoded = Vec::with_capacity(payload.images.len());
    for (layer, rgba, w, h) in &payload.images {
        let png_base64 = crate::png_codec::encode_layer_png(rgba, *w, *h, path)?;
        encoded.push(SidecarImage {
            layer: *layer,
            png_base64,
        });
    }
    // Solo un diseño autónomo (sin imagen que lo acompañe) necesita su propia
    // miniatura embebida: el hilo de miniaturas de la galería no tiene GPU
    // para hornear la página él mismo (`thumbs::thumbnail`). El sidecar de
    // una imagen es peso muerto aquí — su miniatura sale del propio raster.
    let preview_png = match (&payload.preview, image_hash.is_none()) {
        (Some(p), true) => Some(crate::png_codec::encode_layer_png(
            &p.rgba, p.width, p.height, path,
        )?),
        _ => None,
    };
    let file = SidecarFile {
        version: SIDECAR_VERSION,
        image_hash,
        background_layer: payload.background_layer,
        preview_png,
        document: payload.document.clone(),
        images: encoded,
    };
    serde_json::to_vec_pretty(&file).map_err(|e| IoError::Encode {
        path: path.to_owned(),
        message: format!("serializing the sidecar: {e}"),
    })
}

/// Escribe (atómico) el sidecar de `image_path`, en `.canvas/`. `image_bytes`
/// son los bytes codificados de la imagen recién guardada (para el hash). Si
/// había un hermano legacy (`foto.png.canvas`, de antes de que los sidecar se
/// escondieran), se borra tras escribir el nuevo con éxito — así es como se
/// migra una carpeta, un guardado a la vez.
pub fn write_sidecar(
    image_path: &Path,
    image_bytes: &[u8],
    payload: &CanvasPayload,
) -> Result<(), IoError> {
    let folder = image_path.parent().unwrap_or_else(|| Path::new(""));
    ensure_sidecar_dir(folder)?;
    let path = sidecar_path(image_path);
    let hash = Some(format!("{:016x}", fnv1a64(image_bytes)));
    let json = encode_payload(&path, hash, payload)?;
    write_atomic(&path, &json)?;
    let legacy = legacy_sidecar_path(image_path);
    if legacy.is_file() {
        if let Err(e) = std::fs::remove_file(&legacy) {
            tracing::warn!(
                "no se pudo borrar el sidecar legacy {}: {e}",
                legacy.display()
            );
        }
    }
    Ok(())
}

/// Escribe (atómico) un diseño autónomo en `path`: sin imagen que contrastar.
pub fn write_design(path: &Path, payload: &CanvasPayload) -> Result<(), IoError> {
    let json = encode_payload(path, None, payload)?;
    write_atomic(path, &json)
}

/// (documento + imágenes crudas, píxeles ya decodificados por capa).
type ParsedCanvasFile = (SidecarFile, Vec<(u64, LoadedImage)>);

/// Lee y valida el `.canvas` en `path`: probe de versión, parseo completo,
/// reparación de la invariante de preorden y decodificación de los píxeles
/// de cada capa. Compartido por `read_sidecar` y `read_design`.
fn read_canvas_file(path: &Path) -> Result<Option<ParsedCanvasFile>, IoError> {
    let json = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(IoError::Open {
                path: path.to_owned(),
                source,
            })
        }
    };
    // Comprueba la versión ANTES de intentar parsear el documento completo:
    // un sidecar más nuevo puede traer variantes de capa que este build no
    // conoce, y eso debe reportarse como "versión más reciente", no como
    // "corrupto".
    let probe: VersionProbe = serde_json::from_slice(&json).map_err(|e| IoError::Decode {
        path: path.to_owned(),
        source: image::ImageError::IoError(std::io::Error::other(format!("corrupt sidecar: {e}"))),
    })?;
    if probe.version > SIDECAR_VERSION {
        return Err(IoError::Decode {
            path: path.to_owned(),
            source: image::ImageError::IoError(std::io::Error::other(format!(
                "this file was created with a newer version of Canvas Desktop \
                 (version {}, this app understands up to {SIDECAR_VERSION})",
                probe.version
            ))),
        });
    }
    let mut file: SidecarFile = serde_json::from_slice(&json).map_err(|e| IoError::Decode {
        path: path.to_owned(),
        source: image::ImageError::IoError(std::io::Error::other(format!("corrupt sidecar: {e}"))),
    })?;
    // Repara la invariante de preorden ante un sidecar ajeno o corrupto
    // (padres colgando, ciclos…) antes de que nada más lo toque.
    for page in &mut file.document.pages {
        page.normalize_tree();
    }

    let mut images = Vec::with_capacity(file.images.len());
    for entry in &file.images {
        let (rgba, width, height) = crate::png_codec::decode_layer_png(&entry.png_base64, path)?;
        images.push((
            entry.layer,
            LoadedImage {
                rgba,
                width,
                height,
            },
        ));
    }
    Ok(Some((file, images)))
}

/// Lee el sidecar de `image_path`, si existe (en `.canvas/` o, si esa carpeta
/// nunca se creó, el hermano legacy). Devuelve `Ok(None)` si no hay sidecar;
/// error solo si existe pero está corrupto o es de versión futura. Si la
/// imagen que acompañaba ya no está en disco, no hay nada que contrastar y
/// `hash_matches` sale en `true`.
pub fn read_sidecar(image_path: &Path) -> Result<Option<RestoredDocument>, IoError> {
    let Some(path) = find_sidecar(image_path) else {
        return Ok(None);
    };
    let Some((file, images)) = read_canvas_file(&path)? else {
        return Ok(None);
    };
    let standalone = file.image_hash.is_none();
    let hash_matches = match (&file.image_hash, std::fs::read(image_path)) {
        (Some(h), Ok(bytes)) => format!("{:016x}", fnv1a64(&bytes)) == *h,
        _ => true,
    };
    Ok(Some(RestoredDocument {
        document: file.document,
        images,
        background_layer: file.background_layer,
        hash_matches,
        standalone,
    }))
}

/// Lee un diseño autónomo en `path`. A diferencia de `read_sidecar`, un
/// diseño ausente es un error real (no hay imagen hermana que abrir en su
/// lugar).
pub fn read_design(path: &Path) -> Result<RestoredDocument, IoError> {
    let Some((file, images)) = read_canvas_file(path)? else {
        return Err(IoError::Open {
            path: path.to_owned(),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "design file not found"),
        });
    };
    Ok(RestoredDocument {
        document: file.document,
        images,
        background_layer: file.background_layer,
        hash_matches: true,
        standalone: true,
    })
}

/// Solo la miniatura embebida de un `.canvas`: no decodifica los PNG de las
/// capas. `Ok(None)` si el archivo no existe o no tiene miniatura (p. ej. un
/// diseño creado pero nunca guardado con éxito).
pub fn read_preview(path: &Path) -> Result<Option<LoadedImage>, IoError> {
    let json = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(IoError::Open {
                path: path.to_owned(),
                source,
            })
        }
    };
    let probe: PreviewProbe = serde_json::from_slice(&json).map_err(|e| IoError::Decode {
        path: path.to_owned(),
        source: image::ImageError::IoError(std::io::Error::other(format!("corrupt sidecar: {e}"))),
    })?;
    match probe.preview_png {
        Some(b64) => {
            let (rgba, width, height) = crate::png_codec::decode_layer_png(&b64, path)?;
            Ok(Some(LoadedImage {
                rgba,
                width,
                height,
            }))
        }
        None => Ok(None),
    }
}

/// Tamaño de la primera página de un `.canvas`, sin decodificar miniatura ni
/// píxeles. Usado por `canvas_io::probe_page_size` para apilar la baraja del
/// editor antes de cargar ningún documento entero.
pub(crate) fn read_page_size(path: &Path) -> Result<(f64, f64), IoError> {
    let json = std::fs::read(path).map_err(|source| IoError::Open {
        path: path.to_owned(),
        source,
    })?;
    let probe: PageProbe = serde_json::from_slice(&json).map_err(|e| IoError::Decode {
        path: path.to_owned(),
        source: image::ImageError::IoError(std::io::Error::other(format!("corrupt sidecar: {e}"))),
    })?;
    let page = probe
        .document
        .pages
        .first()
        .ok_or_else(|| IoError::Decode {
            path: path.to_owned(),
            source: image::ImageError::IoError(std::io::Error::other("sidecar has no pages")),
        })?;
    Ok((page.width, page.height))
}

/// Borra el sidecar de `image_path` si existe (guardado con el sidecar
/// desactivado) — en cualquiera de sus dos posibles ubicaciones, para que un
/// hermano legacy no sobreviva y vuelva a avisar de "hash cambiado" más tarde.
pub fn delete_sidecar(image_path: &Path) {
    for path in [sidecar_path(image_path), legacy_sidecar_path(image_path)] {
        if path.is_file() {
            if let Err(e) = std::fs::remove_file(&path) {
                tracing::warn!("no se pudo borrar el sidecar {}: {e}", path.display());
            }
        }
    }
}

/// Escribe un lienzo nuevo en blanco: si `path` es `.canvas`, un diseño
/// autónomo (comportamiento clásico); si no, la imagen raster en blanco más
/// su sidecar — un lienzo nuevo respaldado por un archivo de verdad, visible
/// en el Explorador y en cualquier visor. Sin acceso a GPU: pensado para un
/// hilo de trabajo (`spawn_gallery_op`), no para el camino de guardado normal
/// (que hornea en GPU y por tanto conoce el contenido real de las capas —
/// aquí no hay ninguna).
pub fn write_blank_canvas(
    path: &Path,
    width: f64,
    height: f64,
    jpeg_quality: u8,
) -> Result<(), IoError> {
    let payload = blank_design(width, height);
    if crate::is_canvas_file(path) {
        return write_design(path, &payload);
    }
    let w = width.round().max(1.0) as u32;
    let h = height.round().max(1.0) as u32;
    let rgba = vec![255u8; w as usize * h as usize * 4];
    let bytes = crate::save_rgba(path, rgba, w, h, jpeg_quality, None)?;
    write_sidecar(path, &bytes, &payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use canvas_core::{ImageContent, LayerContent, Transform};

    fn sample_doc() -> (Document, Vec<LayerPixels>) {
        let mut doc = Document::new(200.0, 100.0);
        let id = doc
            .add_layer(
                "img",
                Transform::new(25.0, 10.0, 50.0, 40.0),
                LayerContent::Image(ImageContent {
                    source_path: None,
                    natural_width: 4,
                    natural_height: 2,
                    crop: None,
                }),
            )
            .unwrap();
        doc.layer_mut(id).unwrap().effects.blur_radius = 7.0;
        let rgba: Vec<u8> = (0..4 * 2 * 4).map(|i| (i * 7 % 256) as u8).collect();
        (doc, vec![(id.raw(), rgba, 4, 2)])
    }

    fn sample_payload(
        document: &Document,
        images: &[LayerPixels],
        background_layer: Option<u64>,
    ) -> CanvasPayload {
        CanvasPayload {
            document: document.clone(),
            images: images.to_vec(),
            background_layer,
            preview: None,
        }
    }

    #[test]
    fn roundtrip_restores_document_and_pixels() {
        let dir = tempfile::tempdir().expect("tempdir");
        let image_path = dir.path().join("foto.png");
        let fake_image = b"bytes de la imagen guardada";
        std::fs::write(&image_path, fake_image).unwrap();

        let (doc, images) = sample_doc();
        let payload = sample_payload(&doc, &images, None);
        write_sidecar(&image_path, fake_image, &payload).expect("escribir");
        assert!(sidecar_path(&image_path).exists());

        let restored = read_sidecar(&image_path)
            .expect("leer")
            .expect("hay sidecar");
        assert!(restored.hash_matches);
        assert!(!restored.standalone);
        assert_eq!(restored.document, doc);
        assert_eq!(restored.images.len(), 1);
        let (layer, pixels) = &restored.images[0];
        assert_eq!(*layer, images[0].0);
        assert_eq!((pixels.width, pixels.height), (4, 2));
        assert_eq!(pixels.rgba, images[0].1);
    }

    #[test]
    fn detects_externally_modified_image() {
        let dir = tempfile::tempdir().expect("tempdir");
        let image_path = dir.path().join("foto.png");
        std::fs::write(&image_path, b"original").unwrap();

        let (doc, images) = sample_doc();
        let payload = sample_payload(&doc, &images, None);
        write_sidecar(&image_path, b"original", &payload).expect("escribir");

        // Alguien edita la imagen por fuera.
        std::fs::write(&image_path, b"modificada por otro programa").unwrap();
        let restored = read_sidecar(&image_path)
            .expect("leer")
            .expect("hay sidecar");
        assert!(!restored.hash_matches);
    }

    #[test]
    fn groups_survive_a_sidecar_roundtrip() {
        use canvas_core::Layer;

        let dir = tempfile::tempdir().expect("tempdir");
        let image_path = dir.path().join("foto.png");
        let fake_image = b"bytes de la imagen guardada";
        std::fs::write(&image_path, fake_image).unwrap();

        let (mut doc, images) = sample_doc();
        let child = doc
            .layer(canvas_core::LayerId::from_raw(images[0].0))
            .unwrap()
            .id;
        let group_id = doc.allocate_layer_id();
        {
            let page = doc.page_mut().unwrap();
            page.insert_child(Layer::group(group_id, "Group"), None, 1);
            page.move_subtree(child, Some(group_id), 0).unwrap();
        }

        let payload = sample_payload(&doc, &images, None);
        write_sidecar(&image_path, fake_image, &payload).expect("escribir");
        let restored = read_sidecar(&image_path)
            .expect("leer")
            .expect("hay sidecar");
        assert_eq!(restored.document, doc);
        assert_eq!(
            restored.document.layer(child).unwrap().parent_id,
            Some(group_id)
        );
        assert!(restored.document.page().unwrap().is_group(group_id));
    }

    #[test]
    fn version_probe_rejects_a_newer_sidecar_before_parsing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let image_path = dir.path().join("foto.png");
        std::fs::write(&image_path, b"x").unwrap();

        // Documento con un campo de version futura y contenido que este
        // build no sabria deserializar (para probar que el rechazo ocurre
        // ANTES de intentar parsear `SidecarFile`).
        let fake_future = serde_json::json!({
            "version": SIDECAR_VERSION + 1,
            "algo_que_no_existe_todavia": { "esto": "no es un Document" },
        });
        let path = sidecar_path(&image_path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, serde_json::to_vec(&fake_future).unwrap()).unwrap();

        match read_sidecar(&image_path) {
            Err(IoError::Decode { .. }) => {}
            Err(other) => panic!("se esperaba IoError::Decode, se obtuvo otro error: {other}"),
            Ok(_) => panic!("un sidecar de version futura no deberia leerse con exito"),
        }
    }

    #[test]
    fn missing_sidecar_is_none_and_delete_is_quiet() {
        let dir = tempfile::tempdir().expect("tempdir");
        let image_path = dir.path().join("foto.png");
        std::fs::write(&image_path, b"x").unwrap();
        assert!(read_sidecar(&image_path).expect("leer").is_none());
        delete_sidecar(&image_path); // no explota sin sidecar

        let (doc, images) = sample_doc();
        let payload = sample_payload(&doc, &images, Some(7));
        write_sidecar(&image_path, b"x", &payload).expect("escribir");
        let restored = read_sidecar(&image_path).unwrap().unwrap();
        assert_eq!(restored.background_layer, Some(7));
        delete_sidecar(&image_path);
        assert!(!sidecar_path(&image_path).exists());
    }

    #[test]
    fn design_roundtrips_without_any_image() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("Untitled.canvas");

        let (doc, images) = sample_doc();
        let payload = sample_payload(&doc, &images, None);
        write_design(&path, &payload).expect("escribir diseño");

        let restored = read_design(&path).expect("leer diseño");
        assert!(restored.standalone);
        assert!(restored.hash_matches);
        assert_eq!(restored.document, doc);
        assert_eq!(restored.images.len(), 1);
        assert_eq!(restored.images[0].1.rgba, images[0].1);
    }

    #[test]
    fn design_preview_roundtrips() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("Untitled.canvas");

        let (doc, images) = sample_doc();
        let mut payload = sample_payload(&doc, &images, None);
        let preview_rgba: Vec<u8> = (0..8 * 4 * 4).map(|i| (i * 3 % 256) as u8).collect();
        payload.preview = Some(LoadedImage {
            rgba: preview_rgba.clone(),
            width: 8,
            height: 4,
        });
        write_design(&path, &payload).expect("escribir diseño");

        let preview = read_preview(&path)
            .expect("leer preview")
            .expect("hay preview");
        assert_eq!((preview.width, preview.height), (8, 4));
        assert_eq!(preview.rgba, preview_rgba);

        // Sin miniatura embebida: `Ok(None)`, no error.
        let no_preview_path = dir.path().join("SinPreview.canvas");
        let mut payload_sin = sample_payload(&doc, &images, None);
        payload_sin.preview = None;
        write_design(&no_preview_path, &payload_sin).expect("escribir diseño sin preview");
        assert!(read_preview(&no_preview_path)
            .expect("leer preview")
            .is_none());
    }

    /// Red de compatibilidad hacia atrás: un sidecar v3 (sin `preview_png`,
    /// `image_hash` obligatorio) escrito a mano se sigue leyendo tal cual.
    #[test]
    fn v3_sidecar_still_restores() {
        let dir = tempfile::tempdir().expect("tempdir");
        let image_path = dir.path().join("foto.png");
        let fake_image = b"bytes de la imagen guardada";
        std::fs::write(&image_path, fake_image).unwrap();

        let (doc, images) = sample_doc();
        let encoded_images: Vec<_> = images
            .iter()
            .map(|(layer, rgba, w, h)| {
                serde_json::json!({
                    "layer": layer,
                    "png_base64": crate::png_codec::encode_layer_png(
                        rgba,
                        *w,
                        *h,
                        Path::new("test"),
                    )
                    .unwrap(),
                })
            })
            .collect();
        let v3_json = serde_json::json!({
            "version": 3,
            "image_hash": format!("{:016x}", fnv1a64(fake_image)),
            "background_layer": null,
            "document": doc,
            "images": encoded_images,
        });
        let path = sidecar_path(&image_path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, serde_json::to_vec(&v3_json).unwrap()).unwrap();

        let restored = read_sidecar(&image_path)
            .expect("leer sidecar v3")
            .expect("hay sidecar");
        assert!(!restored.standalone);
        assert!(restored.hash_matches);
        assert_eq!(restored.document, doc);
    }

    #[test]
    fn sidecar_survives_a_missing_companion_image() {
        let dir = tempfile::tempdir().expect("tempdir");
        let image_path = dir.path().join("foto.png");
        let fake_image = b"bytes de la imagen guardada";
        std::fs::write(&image_path, fake_image).unwrap();

        let (doc, images) = sample_doc();
        let payload = sample_payload(&doc, &images, None);
        write_sidecar(&image_path, fake_image, &payload).expect("escribir");

        // Alguien borra la imagen original; el sidecar sigue ahí.
        std::fs::remove_file(&image_path).unwrap();
        let restored = read_sidecar(&image_path)
            .expect("leer")
            .expect("hay sidecar aunque falte la imagen");
        assert!(restored.hash_matches);
        assert!(!restored.standalone);
    }

    #[test]
    fn an_absent_hash_marks_the_file_standalone() {
        let dir = tempfile::tempdir().expect("tempdir");
        let image_path = dir.path().join("foto.png");
        std::fs::write(&image_path, b"imagen real").unwrap();

        // Un `.canvas` con el nombre de sidecar de `foto.png`, pero escrito
        // como diseño autónomo (sin `image_hash`).
        let (doc, images) = sample_doc();
        let payload = sample_payload(&doc, &images, None);
        let path = sidecar_path(&image_path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        write_design(&path, &payload).expect("escribir diseño");

        let restored = read_sidecar(&image_path)
            .expect("leer")
            .expect("hay archivo en la ruta del sidecar");
        assert!(restored.standalone);
        assert!(restored.hash_matches);
    }

    #[test]
    fn write_sidecar_hides_the_dot_canvas_folder_and_never_leaves_it_next_to_the_image() {
        let dir = tempfile::tempdir().expect("tempdir");
        let image_path = dir.path().join("foto.png");
        let fake_image = b"bytes de la imagen guardada";
        std::fs::write(&image_path, fake_image).unwrap();

        let (doc, images) = sample_doc();
        let payload = sample_payload(&doc, &images, None);
        write_sidecar(&image_path, fake_image, &payload).expect("escribir");

        assert!(sidecar_path(&image_path).exists());
        assert_eq!(
            sidecar_path(&image_path).parent().unwrap(),
            sidecar_dir(dir.path())
        );
        assert!(!legacy_sidecar_path(&image_path).exists());
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
            let attrs = std::fs::metadata(sidecar_dir(dir.path()))
                .unwrap()
                .file_attributes();
            assert!(
                attrs & FILE_ATTRIBUTE_HIDDEN != 0,
                "la carpeta debe quedar oculta"
            );
        }
    }

    #[test]
    fn find_sidecar_falls_back_to_the_legacy_sibling() {
        let dir = tempfile::tempdir().expect("tempdir");
        let image_path = dir.path().join("foto.png");
        std::fs::write(&image_path, b"x").unwrap();

        // Sidecar escrito a la manera antigua (hermano directo), sin pasar
        // por `write_sidecar`: simula una carpeta de antes de este cambio.
        let (doc, images) = sample_doc();
        let payload = sample_payload(&doc, &images, None);
        let legacy = legacy_sidecar_path(&image_path);
        let hash = format!("{:016x}", fnv1a64(b"x"));
        let json = encode_payload(&legacy, Some(hash), &payload).unwrap();
        std::fs::write(&legacy, json).unwrap();

        assert_eq!(find_sidecar(&image_path), Some(legacy.clone()));
        let restored = read_sidecar(&image_path)
            .expect("leer")
            .expect("se encuentra el sidecar legacy");
        assert_eq!(restored.document, doc);

        // Guardar de nuevo migra: el legacy desaparece, el nuevo existe.
        write_sidecar(&image_path, b"x", &payload).expect("escribir");
        assert!(
            !legacy.exists(),
            "el sidecar legacy debe borrarse al migrar"
        );
        assert!(sidecar_path(&image_path).exists());
        assert_eq!(find_sidecar(&image_path), Some(sidecar_path(&image_path)));
    }

    #[test]
    fn delete_sidecar_removes_both_locations() {
        let dir = tempfile::tempdir().expect("tempdir");
        let image_path = dir.path().join("foto.png");
        std::fs::write(&image_path, b"x").unwrap();

        std::fs::create_dir_all(sidecar_dir(dir.path())).unwrap();
        std::fs::write(sidecar_path(&image_path), b"{}").unwrap();
        std::fs::write(legacy_sidecar_path(&image_path), b"{}").unwrap();

        delete_sidecar(&image_path);
        assert!(!sidecar_path(&image_path).exists());
        assert!(!legacy_sidecar_path(&image_path).exists());
    }

    #[test]
    fn preview_png_is_only_embedded_for_a_standalone_design() {
        let (doc, images) = sample_doc();
        let mut payload = sample_payload(&doc, &images, None);
        payload.preview = Some(LoadedImage {
            rgba: vec![255u8; 4 * 2 * 4],
            width: 4,
            height: 2,
        });

        // Sidecar de imagen (`image_hash: Some(..)`): sin miniatura embebida.
        let with_hash = encode_payload(
            Path::new("test.canvas"),
            Some("deadbeef".to_owned()),
            &payload,
        )
        .unwrap();
        let file: SidecarFile = serde_json::from_slice(&with_hash).unwrap();
        assert!(file.preview_png.is_none());

        // Diseño autónomo (`image_hash: None`): miniatura embebida.
        let standalone = encode_payload(Path::new("test.canvas"), None, &payload).unwrap();
        let file: SidecarFile = serde_json::from_slice(&standalone).unwrap();
        assert!(file.preview_png.is_some());
    }

    #[test]
    fn write_blank_canvas_png_produces_a_real_image_and_its_sidecar() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("Untitled.png");
        write_blank_canvas(&path, 40.0, 20.0, 92).expect("crear lienzo en blanco");

        let decoded = image::open(&path).expect("el PNG debe ser decodificable");
        assert_eq!((decoded.width(), decoded.height()), (40, 20));

        let restored = read_sidecar(&path)
            .expect("leer sidecar")
            .expect("hay sidecar");
        assert!(!restored.standalone);
        assert!(restored.hash_matches);
        assert_eq!(restored.document.page().unwrap().layers.len(), 0);
    }

    #[test]
    fn write_blank_canvas_dot_canvas_still_makes_a_standalone_design() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("Untitled.canvas");
        write_blank_canvas(&path, 40.0, 20.0, 92).expect("crear diseño en blanco");

        let restored = read_design(&path).expect("leer diseño");
        assert!(restored.standalone);
        assert!(
            !path.with_extension("").is_file(),
            "no debe crear ninguna imagen"
        );
    }

    #[test]
    fn moving_to_local_trash_and_restoring_round_trips_the_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("foto.png");
        std::fs::write(&path, b"pixels").unwrap();

        let staged = move_to_local_trash(&path).expect("mover a la papelera");
        assert!(!path.exists(), "ya no debe estar en su sitio original");
        assert!(staged.exists());
        assert_eq!(staged, trash_dir(dir.path()).join("foto.png"));
        assert_eq!(staged, local_trash_path(&path));

        restore_from_local_trash(&staged, &path).expect("restaurar");
        assert!(path.exists());
        assert!(!staged.exists());
        assert_eq!(std::fs::read(&path).unwrap(), b"pixels");
    }

    #[test]
    fn restoring_refuses_to_overwrite_a_file_that_reappeared_at_the_original_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("foto.png");
        std::fs::write(&path, b"original").unwrap();
        let staged = move_to_local_trash(&path).expect("mover a la papelera");

        // Algo (o alguien) creó un archivo nuevo con el mismo nombre
        // mientras tanto.
        std::fs::write(&path, b"nuevo").unwrap();

        let err = restore_from_local_trash(&staged, &path);
        assert!(err.is_err(), "no debe sobrescribir en silencio");
        assert_eq!(std::fs::read(&path).unwrap(), b"nuevo");
        assert!(staged.exists(), "lo movido sigue a salvo en la papelera");
    }

    #[test]
    fn purge_removes_everything_left_in_the_trash_but_not_the_folder_itself() {
        let dir = tempfile::tempdir().expect("tempdir");
        let a = dir.path().join("a.png");
        let b = dir.path().join("b.png");
        std::fs::write(&a, b"a").unwrap();
        std::fs::write(&b, b"b").unwrap();
        move_to_local_trash(&a).expect("mover a");
        move_to_local_trash(&b).expect("mover b");

        purge_local_trash(dir.path());

        assert!(!local_trash_path(&a).exists());
        assert!(!local_trash_path(&b).exists());
    }

    #[test]
    fn purge_of_a_folder_with_no_trash_yet_does_not_panic() {
        let dir = tempfile::tempdir().expect("tempdir");
        purge_local_trash(dir.path());
    }
}
