//! Nombres secuenciales `{carpeta}-NN` para capas y lienzos nuevos.
//!
//! Todas las vías de entrada (pegar de internet, sidebar web/Unsplash e
//! Instagram, bulk, clips de vídeo y lienzos en blanco) comparten este
//! esquema: el slug de la carpeta del proyecto más un número con dos
//! dígitos mínimos (`edwin-luna-03`, `clip-edwin-luna-01`). Los nombres
//! viejos (`Pasted Image`, `Web · …`) se conservan donde están; solo lo
//! NUEVO nace con este esquema, así ningún sidecar existente cambia.

use std::collections::HashSet;
use std::path::Path;

/// Trocea un texto a un tallo de archivo seguro (minúsculas, guiones,
/// máximo 30 caracteres). Lo usaba la importación masiva para la keyword;
/// ahora también es la base del slug de carpeta.
pub(crate) fn slugify(text: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in text.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
        if out.len() >= 30 {
            break;
        }
    }
    let slug = out.trim_matches('-').to_owned();
    if slug.is_empty() {
        "web".to_owned()
    } else {
        slug
    }
}

/// Slug de la carpeta del proyecto (`Edwin Luna` → `edwin-luna`).
/// `None` sin carpeta (sesión sin guardar): el llamador conserva su
/// nombre heredado en vez de inventar uno.
pub(crate) fn folder_stem(folder: Option<&Path>) -> Option<String> {
    let name = folder?.file_name()?.to_str()?;
    if name.trim().is_empty() {
        return None;
    }
    Some(slugify(name))
}

/// Números ya ocupados con un prefijo entre nombres sin extensión
/// (`edwin-luna-03` → 3 con prefijo `edwin-luna`). Lo heredado que no
/// sigue el esquema se ignora.
pub(crate) fn taken_numbers(
    prefix: &str,
    names: impl IntoIterator<Item = impl AsRef<str>>,
) -> HashSet<u32> {
    names
        .into_iter()
        .filter_map(|name| parse_suffix(prefix, name.as_ref()))
        .collect()
}

/// Números ocupados en disco por tallos `{prefijo}-NN` (cualquier
/// extensión, incluidos los sidecar legacy `foto-02.png.canvas` cuyo tallo
/// aún trae la extensión interior): evita que una capa nueva presuma un
/// número cuyo archivo ya existe. Lo mejor posible: si el directorio no se
/// puede leer, vacío.
pub(crate) fn taken_from_dir(folder: &Path, prefix: &str) -> HashSet<u32> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return HashSet::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let stem = entry.path().file_stem()?.to_str()?.to_owned();
            parse_suffix(prefix, &stem).or_else(|| {
                let inner = Path::new(&stem).file_stem()?.to_str()?;
                parse_suffix(prefix, inner)
            })
        })
        .collect()
}

/// Primer nombre libre con un prefijo (`{prefijo}-01`, `{prefijo}-02`…,
/// con dos dígitos mínimos). Primer hueco libre, igual que las reservas
/// atómicas de `canvas-io`: reutiliza números de archivos borrados.
pub(crate) fn next_name(prefix: &str, taken: &HashSet<u32>) -> String {
    let mut n = 1u32;
    while taken.contains(&n) {
        n += 1;
    }
    format!("{prefix}-{n:02}")
}

/// Nombre para una capa de imagen nueva en `folder` (`edwin-luna-04`):
/// une los números de las capas del documento con los del disco para no
/// prometer un número ya archivado.
pub(crate) fn next_image_name(
    folder: &Path,
    layer_names: impl IntoIterator<Item = impl AsRef<str>>,
) -> String {
    let prefix = folder_stem(Some(folder)).unwrap_or_else(|| "canvas".to_owned());
    let mut taken = taken_numbers(&prefix, layer_names);
    taken.extend(taken_from_dir(folder, &prefix));
    next_name(&prefix, &taken)
}

/// Nombre para una capa pegada en el hilo de UI: solo los números de las
/// capas del documento, sin leer el disco (un `read_dir` en una carpeta de
/// red con cientos de lienzos se nota en cada Ctrl+V). La reserva atómica
/// del guardado ya impide colisiones reales de archivo; esto solo evita
/// prometer un número que el propio documento usa.
pub(crate) fn next_pasted_name(
    folder: Option<&Path>,
    layer_names: impl IntoIterator<Item = impl AsRef<str>>,
) -> String {
    match folder_stem(folder) {
        Some(prefix) => next_name(&prefix, &taken_numbers(&prefix, layer_names)),
        None => "Pasted Image".to_owned(),
    }
}

/// Prefijo de los clips de una carpeta (`clip-edwin-luna`): el número lo
/// pone la reserva del slot (`clip-edwin-luna-01.mp4`). `None` sin carpeta:
/// el llamador conserva el nombre del clip, como antes.
pub(crate) fn clip_prefix(folder: Option<&Path>) -> Option<String> {
    folder_stem(folder).map(|stem| format!("clip-{stem}"))
}

/// Sufijo `-NN` tras un prefijo exacto, sin extensión. Solo dígitos
/// (`03` → 3); el `0` no cuenta como número ocupado.
fn parse_suffix(prefix: &str, name: &str) -> Option<u32> {
    let rest = name.strip_prefix(prefix)?.strip_prefix('-')?;
    if rest.is_empty() || !rest.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    rest.parse::<u32>().ok().filter(|&n| n > 0)
}

#[cfg(test)]
#[path = "naming_tests.rs"]
mod tests;
