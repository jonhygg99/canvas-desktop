//! Lectura y escritura real a disco de un `.canvas` (sidecar o diseño
//! autónomo).

use std::path::Path;

use serde::Deserialize;

use crate::{write_atomic, IoError, LoadedImage};

use std::borrow::Cow;

use super::container;
use super::paths::{ensure_sidecar_dir, sidecar_path};
use super::payload::{encode_payload, CanvasPayload, RestoredDocument};
use super::{find_sidecar, fnv1a64, legacy_sidecar_path, SidecarFile, SIDECAR_VERSION};

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

/// (documento + imágenes crudas, píxeles ya decodificados por capa).
type ParsedCanvasFile = (SidecarFile, Vec<(u64, LoadedImage)>);

/// Tope de tamaño de un `.canvas` en disco (512 MiB). El JSON embebe los
/// píxeles de todas las capas en base64, así que un documento con varias
/// fotos grandes es legítimamente pesado; el tope solo corta antes de
/// volcar a memoria un archivo absurdo (corrupto o malintencionado) de
/// varios GB.
const MAX_CANVAS_BYTES: u64 = 512 * 1024 * 1024;

/// `fs::read` con tope de tamaño: comprueba la longitud ANTES de leer para
/// no asignar el archivo entero si ya nació fuera de límite.
fn read_capped(path: &Path) -> std::io::Result<Vec<u8>> {
    let meta = std::fs::metadata(path)?;
    if meta.len() > MAX_CANVAS_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::FileTooLarge,
            format!("canvas file exceeds the {MAX_CANVAS_BYTES}-byte read limit"),
        ));
    }
    std::fs::read(path)
}

/// (cabecera JSON, blobs) de un `.canvas` en cualquiera de los dos
/// formatos: contenedor v5 o JSON puro v1–v4.
type SplitCanvas<'a> = (Cow<'a, [u8]>, Vec<Vec<u8>>);

/// Separa (cabecera JSON, blobs) de un `.canvas` en cualquiera de los dos
/// formatos: contenedor v5 (mágica `CANVAS5`) o JSON puro v1–v4 (el archivo
/// entero es el JSON, sin blobs).
fn split_canvas_file<'a>(bytes: &'a [u8], path: &Path) -> Result<SplitCanvas<'a>, IoError> {
    if container::is_container(bytes) {
        let (json, blobs) = container::split_container(bytes, path)?;
        Ok((
            Cow::Borrowed(json),
            blobs.into_iter().map(<[u8]>::to_vec).collect(),
        ))
    } else {
        Ok((Cow::Borrowed(bytes), Vec::new()))
    }
}

/// Lee y valida el `.canvas` en `path`: probe de versión, parseo completo,
/// reparación de la invariante de preorden y decodificación de los píxeles
/// de cada capa. Compartido por `read_sidecar` y `read_design`.
fn read_canvas_file(path: &Path) -> Result<Option<ParsedCanvasFile>, IoError> {
    let bytes = match read_capped(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(IoError::Open {
                path: path.to_owned(),
                source,
            })
        }
    };
    let (json, blobs) = split_canvas_file(&bytes, path)?;
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
        // v1–v4: PNG en base64 dentro del JSON. v5: blob binario por índice.
        let (rgba, width, height) = match (&entry.png_base64, entry.blob) {
            (Some(png_base64), _) => crate::png_codec::decode_layer_png(png_base64, path)?,
            (None, Some(blob_index)) => {
                let Some(png) = blobs.get(blob_index as usize) else {
                    return Err(IoError::Decode {
                        path: path.to_owned(),
                        source: image::ImageError::IoError(std::io::Error::other(format!(
                            "corrupt sidecar: blob index {blob_index} out of range"
                        ))),
                    });
                };
                crate::png_codec::decode_png_bytes(png, path)?
            }
            (None, None) => {
                return Err(IoError::Decode {
                    path: path.to_owned(),
                    source: image::ImageError::IoError(std::io::Error::other(
                        "corrupt sidecar: layer pixels missing",
                    )),
                })
            }
        };
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
    let bytes = match read_capped(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(IoError::Open {
                path: path.to_owned(),
                source,
            })
        }
    };
    // Solo la cabecera: la miniatura vive en el JSON (v5 y legacy).
    let (json, _) = split_canvas_file(&bytes, path)?;
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
    let bytes = read_capped(path).map_err(|source| IoError::Open {
        path: path.to_owned(),
        source,
    })?;
    // Solo la cabecera: el tamaño de página vive en el JSON (v5 y legacy).
    let (json, _) = split_canvas_file(&bytes, path)?;
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
    let payload = super::blank_design(width, height);
    if crate::is_canvas_file(path) {
        return write_design(path, &payload);
    }
    let w = width.round().max(1.0) as u32;
    let h = height.round().max(1.0) as u32;
    let rgba = vec![255u8; w as usize * h as usize * 4];
    let bytes = crate::save_rgba(path, rgba, w, h, jpeg_quality, None)?;
    write_sidecar(path, &bytes, &payload)
}
