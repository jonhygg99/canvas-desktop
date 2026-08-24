//! Portapapeles: cortar/copiar/pegar/duplicar capas y borrar la selección.
//!
//! El portapapeles INTERNO (capas serializadas en JSON) vive en una ranura
//! de sesión, a propósito NO en el portapapeles del sistema operativo: eso
//! machacaría el portapapeles de texto del usuario. El portapapeles del
//! sistema solo se toca para pegar una imagen externa (Win+Shift+S, "Copiar
//! imagen"…) en un Paste explícito (nunca por frame: en Windows abre el
//! portapapeles del SO y puede fallar mientras otra app lo tiene). Se
//! prueba antes que la ranura interna cuando su número de secuencia indica
//! que cambió después de la última copia interna (`prefer_system`); si no,
//! se prueba primero la ranura interna y el del sistema queda de reserva.

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};

use canvas_core::{Command, Composite, InsertLayer, LayerId, Selection};
use canvas_render::image_data_from_rgba;

use crate::editor::EditorState;

/// JSON de la copia interna junto con el número de secuencia del
/// portapapeles del SISTEMA en el momento de copiar (ver `prefer_system`).
/// Mensaje para `EditorState::save_error` cuando `paste` no encuentra nada
/// que pegar, ni en la ranura interna ni en el portapapeles del sistema.
pub const PASTE_EMPTY_MSG: &str = "Clipboard has no image or layers to paste";

fn slot() -> &'static Mutex<Option<(String, u32)>> {
    static SLOT: OnceLock<Mutex<Option<(String, u32)>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

/// Número de secuencia del portapapeles del sistema: cambia cada vez que
/// alguna app pone algo ahí, sin abrirlo ni bloquearlo. Solo tiene sentido
/// en Windows — fuera de ahí `prefer_system` siempre compara `0` contra
/// `0` y no altera el orden de prioridad de siempre.
#[cfg(windows)]
fn clipboard_sequence() -> u32 {
    // SAFETY: no toma punteros ni requiere el portapapeles abierto.
    unsafe { windows::Win32::System::DataExchange::GetClipboardSequenceNumber() }
}

#[cfg(not(windows))]
fn clipboard_sequence() -> u32 {
    0
}

/// Si el portapapeles del sistema cambió desde la última copia interna
/// (p.ej. el usuario copió una capa, fue al navegador y copió una imagen),
/// eso es lo que el usuario espera pegar — probarlo antes que la copia
/// interna, más vieja pero todavía en la ranura.
fn prefer_system(slot_seq: u32, now_seq: u32) -> bool {
    now_seq != slot_seq
}

/// El conjunto a copiar: cada raíz de la selección y todos sus
/// descendientes, en orden de pila y sin duplicados (una raíz cuyo
/// descendiente también estuviera en la selección ya lo arrastra consigo).
fn selection_subtree(state: &EditorState) -> Vec<LayerId> {
    let Ok(page) = state.doc.page() else {
        return Vec::new();
    };
    let roots = state.selection.roots(page);
    let mut seen = HashSet::new();
    let mut ids = Vec::new();
    for root in roots {
        if seen.insert(root) {
            ids.push(root);
        }
        for d in page.descendants(root) {
            if seen.insert(d) {
                ids.push(d);
            }
        }
    }
    ids.sort_by_key(|&id| page.index_of(id).unwrap_or(usize::MAX));
    ids
}

/// Serializa el conjunto de ids (ya en orden de pila) a un JSON de
/// portapapeles, con los píxeles de sus capas raster embebidos.
fn encode_layers(state: &EditorState, ids: &[LayerId]) -> Option<String> {
    if ids.is_empty() {
        return None;
    }
    let page = state.doc.page().ok()?;
    let set: HashSet<LayerId> = ids.iter().copied().collect();
    let mut layers = Vec::with_capacity(ids.len());
    let mut images = Vec::new();
    for &id in ids {
        let mut layer = page.layer(id)?.clone();
        // El padre puede haber quedado fuera del conjunto copiado.
        if layer.parent_id.is_some_and(|p| !set.contains(&p)) {
            layer.parent_id = None;
        }
        if let Some(data) = state.images.get(&id) {
            if let Ok(png) = canvas_io::encode_layer_png(data.data.data(), data.width, data.height)
            {
                images.push((id.raw(), png));
            }
        }
        layers.push(layer);
    }
    let doc = canvas_io::ClipboardDoc::new(layers, images);
    canvas_io::write_clipboard(&doc).ok()
}

/// Copia la selección al portapapeles interno de la sesión.
pub fn copy(state: &EditorState) {
    let ids = selection_subtree(state);
    let Some(json) = encode_layers(state, &ids) else {
        return;
    };
    if let Ok(mut s) = slot().lock() {
        *s = Some((json, clipboard_sequence()));
    }
}

/// Corta la selección: copia y borra, en un solo movimiento (el borrado ya
/// es su propio paso de deshacer).
pub fn cut(state: &mut EditorState) {
    copy(state);
    crate::editor::delete_selected(state);
}

/// Pega un `ClipboardDoc` ya deserializado: reasigna ids, desplaza el
/// conjunto 24px y lo inserta al tope de la pila. Deja los píxeles de las
/// capas raster en `state.images` bajo los ids nuevos (si el pegado se
/// deshace, se quedan ahí — mismo criterio que el fondo desenfocado: están
/// indexados por id, así que un redo los vuelve a encontrar). Devuelve los
/// ids nuevos, en el mismo orden que venían.
fn paste_doc(state: &mut EditorState, clip: canvas_io::ClipboardDoc) -> Option<Vec<LayerId>> {
    if clip.layers.is_empty() {
        return None;
    }
    let mut remap: HashMap<LayerId, LayerId> = HashMap::new();
    for layer in &clip.layers {
        remap.insert(layer.id, state.doc.allocate_layer_id());
    }
    let base = state.doc.page().ok()?.layers.len();
    let mut cmds: Vec<Box<dyn Command>> = Vec::new();
    let mut new_ids = Vec::with_capacity(clip.layers.len());
    for (k, layer) in clip.layers.iter().enumerate() {
        let mut n = layer.clone();
        n.id = remap[&layer.id];
        n.parent_id = n.parent_id.and_then(|p| remap.get(&p).copied());
        n.transform.x += 24.0;
        n.transform.y += 24.0;
        new_ids.push(n.id);
        cmds.push(Box::new(InsertLayer {
            index: base + k,
            layer: n,
        }) as Box<dyn Command>);
    }
    for cmd in &mut cmds {
        // `InsertLayer::apply` solo falla si el documento se quedó sin
        // páginas a media inserción, algo que no ocurre en la práctica.
        let _ = cmd.apply(&mut state.doc);
    }
    state.push_undo_step(Box::new(Composite::new("Pegar capas", cmds)));

    for (old_raw, png_base64) in &clip.images {
        let Some(&new_id) = remap.get(&LayerId::from_raw(*old_raw)) else {
            continue;
        };
        if let Ok((rgba, w, h)) = canvas_io::decode_layer_png(png_base64) {
            state
                .images
                .insert(new_id, image_data_from_rgba(rgba, w, h));
        }
    }
    Some(new_ids)
}

fn select_ids(state: &mut EditorState, ids: &[LayerId]) {
    let mut sel = Selection::default();
    for &id in ids {
        sel.toggle(id);
    }
    state.selection = sel;
}

/// Pega desde el portapapeles interno si tiene algo; si no, intenta una
/// imagen del portapapeles del SISTEMA operativo. Si el portapapeles del
/// sistema cambió desde la última copia interna, se prueba primero él (ver
/// `prefer_system`) — si no, "copiar una capa, luego copiar una imagen en
/// el navegador, Ctrl+V" seguiría pegando la capa vieja. Devuelve `false`
/// si no había nada que pegar en ningún sitio.
pub fn paste(state: &mut EditorState) -> bool {
    let internal = slot().lock().ok().and_then(|s| s.clone());
    if let Some((json, slot_seq)) = &internal {
        if prefer_system(*slot_seq, clipboard_sequence()) && paste_system(state) {
            return true;
        }
        if paste_internal(state, json) {
            return true;
        }
    }
    paste_system(state)
}

fn paste_internal(state: &mut EditorState, json: &str) -> bool {
    let Ok(clip) = canvas_io::read_clipboard(json) else {
        return false;
    };
    let Some(ids) = paste_doc(state, clip) else {
        return false;
    };
    select_ids(state, &ids);
    true
}

fn paste_system(state: &mut EditorState) -> bool {
    let Some(img) = system_image() else {
        return false;
    };
    state.add_image_layer("Pasted Image", None, img);
    true
}

/// Duplica la selección: copia en memoria (sin tocar la ranura de sesión,
/// que conserva lo que el usuario copió a propósito) y pega inmediatamente.
pub fn duplicate(state: &mut EditorState) {
    let ids = selection_subtree(state);
    let Some(json) = encode_layers(state, &ids) else {
        return;
    };
    let Ok(clip) = canvas_io::read_clipboard(&json) else {
        return;
    };
    if let Some(new_ids) = paste_doc(state, clip) {
        select_ids(state, &new_ids);
    }
}

/// Selecciona todas las capas raíz (sin el fondo desenfocado, que se
/// gestiona aparte y nunca tiene sentido incluir en una selección masiva).
pub fn select_all(state: &mut EditorState) {
    let Ok(page) = state.doc.page() else {
        return;
    };
    let ids: Vec<LayerId> = page
        .children_of(None)
        .into_iter()
        .filter(|&id| Some(id) != state.background_layer)
        .collect();
    select_ids(state, &ids);
}

/// Imagen del portapapeles del sistema (Win+Shift+S, "Copiar imagen"…).
/// arboard entrega RGBA sin premultiplicar; sus dimensiones vienen en
/// `usize` y hay que comprobar que el buffer cuadra antes de confiar en él.
pub fn system_image() -> Option<canvas_io::LoadedImage> {
    let mut cb = arboard::Clipboard::new().ok()?;
    let img = cb.get_image().ok()?;
    let width = u32::try_from(img.width).ok()?;
    let height = u32::try_from(img.height).ok()?;
    let rgba = img.bytes.into_owned();
    tracing::debug!(
        "system clipboard image: {}x{} ({} bytes)",
        width,
        height,
        rgba.len()
    );
    (width > 0 && height > 0 && rgba.len() == width as usize * height as usize * 4).then_some(())?;
    Some(canvas_io::LoadedImage {
        rgba,
        width,
        height,
    })
}

#[cfg(test)]
mod tests {
    use super::prefer_system;

    #[test]
    fn keeps_internal_first_when_system_clipboard_is_unchanged() {
        assert!(!prefer_system(7, 7));
    }

    #[test]
    fn prefers_system_once_its_sequence_number_moved() {
        assert!(prefer_system(7, 8));
    }

    #[test]
    fn treats_a_lower_sequence_number_as_a_change_too() {
        // El contador puede reiniciarse (p.ej. tras reiniciar sesión); da
        // igual la dirección, cualquier diferencia cuenta como "cambió".
        assert!(prefer_system(8, 7));
    }
}
