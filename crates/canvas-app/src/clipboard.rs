//! Portapapeles: cortar/copiar/pegar/duplicar capas y borrar la selección.
//!
//! El portapapeles INTERNO (capas serializadas en JSON) vive en una ranura
//! de sesión, a propósito NO en el portapapeles del sistema operativo: eso
//! machacaría el portapapeles de texto del usuario. El portapapeles del
//! sistema solo se toca para pegar una imagen externa (Win+Shift+S, "Copiar
//! imagen"…) cuando el interno está vacío, y solo en un Paste explícito
//! (nunca por frame: en Windows abre el portapapeles del SO y puede fallar
//! mientras otra app lo tiene).

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};

use canvas_core::{Command, Composite, InsertLayer, LayerId, RemoveLayer, Selection};
use canvas_render::image_data_from_rgba;

use crate::editor::EditorState;

fn slot() -> &'static Mutex<Option<String>> {
    static SLOT: OnceLock<Mutex<Option<String>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
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
        *s = Some(json);
    }
}

/// Corta la selección: copia y borra, en un solo movimiento (el borrado ya
/// es su propio paso de deshacer).
pub fn cut(state: &mut EditorState) {
    copy(state);
    delete_selected(state);
}

/// Borra las capas seleccionadas (sin tocar las bloqueadas), como un solo
/// paso de deshacer.
pub fn delete_selected(state: &mut EditorState) {
    let Ok(page) = state.doc.page() else {
        return;
    };
    let roots: Vec<LayerId> = state
        .selection
        .roots(page)
        .into_iter()
        .filter(|&id| !page.effective_locked(id))
        .collect();
    if roots.is_empty() {
        return;
    }
    let mut cmds: Vec<Box<dyn Command>> = Vec::new();
    for id in roots {
        let mut cmd = RemoveLayer::new(id);
        if cmd.apply(&mut state.doc).is_ok() {
            cmds.push(Box::new(cmd));
        }
    }
    if !cmds.is_empty() {
        state
            .history
            .push_applied(Box::new(Composite::new("Quitar capas", cmds)));
    }
    state.forget_deleted_selection();
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
    state
        .history
        .push_applied(Box::new(Composite::new("Pegar capas", cmds)));

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
/// imagen del portapapeles del SISTEMA operativo.
pub fn paste(state: &mut EditorState) {
    let json = slot().lock().ok().and_then(|s| s.clone());
    if let Some(json) = json {
        if let Ok(clip) = canvas_io::read_clipboard(&json) {
            if let Some(ids) = paste_doc(state, clip) {
                select_ids(state, &ids);
                return;
            }
        }
    }
    if let Some(img) = system_image() {
        state.add_image_layer("Pasted Image", None, img);
    }
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
    (width > 0 && height > 0 && rgba.len() == width as usize * height as usize * 4).then_some(())?;
    Some(canvas_io::LoadedImage {
        rgba,
        width,
        height,
    })
}
