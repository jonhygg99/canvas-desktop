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
//!
//! Dividido en submódulos por responsabilidad: `paths` (rutas del sidecar y
//! su carpeta), `trash` (papelera propia del proyecto), `payload`
//! (construcción/encode del contenido embebido) e `io` (lectura/escritura
//! real a disco).

mod io;
mod paths;
mod payload;
mod trash;

use std::path::{Path, PathBuf};

use canvas_core::Document;
use serde::{Deserialize, Serialize};

pub(crate) use io::read_page_size;
pub use io::{
    delete_sidecar, read_design, read_preview, read_sidecar, write_blank_canvas, write_design,
    write_sidecar,
};
pub use paths::{ensure_sidecar_dir, find_sidecar, sidecar_dir, sidecar_path, SIDECAR_DIR};
pub use payload::{
    blank_design, fnv1a64, make_preview, preview_scale, CanvasPayload, LayerPixels,
    RestoredDocument,
};
pub use trash::{
    local_trash_path, move_to_local_trash, purge_local_trash, restore_from_local_trash, trash_dir,
};

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

/// Ruta LEGACY del sidecar (hermano directo: `foto.png` → `foto.png.canvas`),
/// de antes de que los sidecar se escondieran en `.canvas/`. Solo para lectura
/// de compatibilidad — nunca se escribe un sidecar nuevo aquí.
fn legacy_sidecar_path(image_path: &Path) -> PathBuf {
    let mut name = image_path.as_os_str().to_owned();
    name.push(".");
    name.push(crate::CANVAS_EXTENSION);
    PathBuf::from(name)
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

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
