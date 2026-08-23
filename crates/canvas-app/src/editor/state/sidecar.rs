//! Ida y vuelta con el sidecar (`foto.png.canvas`): reconstruir el estado a
//! partir de un documento restaurado, y volcarlo otra vez a la carga que se
//! guarda junto a la imagen.

use std::path::PathBuf;

use canvas_core::{LayerId, Selection};
use canvas_render::{image_data_from_rgba, ImageMap};

use super::EditorState;

impl EditorState {
    /// Documento restaurado desde un sidecar `.canvas`: las capas siguen
    /// siendo editables tal y como se guardaron (nada de fondo aplanado).
    pub fn from_restored(path: PathBuf, restored: canvas_io::RestoredDocument) -> Self {
        let mut doc = restored.document;
        doc.source_path = Some(path);
        let mut images = ImageMap::new();
        for (raw, pixels) in restored.images {
            images.insert(
                LayerId::from_raw(raw),
                image_data_from_rgba(pixels.rgba, pixels.width, pixels.height),
            );
        }
        let background_layer = restored.background_layer.map(LayerId::from_raw);
        // Selecciona la capa más alta que no sea el fondo desenfocado.
        let selected = doc.page().ok().and_then(|p| {
            p.layers
                .iter()
                .rev()
                .find(|l| Some(l.id) != background_layer)
                .or_else(|| p.layers.last())
                .map(|l| l.id)
        });
        let selection = selected.map_or_else(Selection::default, Selection::single);
        Self::base(doc, images, selection, background_layer)
    }

    /// Diseño autónomo restaurado desde su propio `.canvas`: como
    /// `from_restored`, salvo que aquí `path` es el diseño mismo, no la
    /// imagen que acompaña. Un `.canvas` duplicado puede traer un
    /// `source_path` incrustado que sigue apuntando al original — inocuo,
    /// porque `from_restored` lo sobrescribe con la ruta realmente abierta.
    pub fn from_design(path: PathBuf, restored: canvas_io::RestoredDocument) -> Self {
        let mut state = Self::from_restored(path, restored);
        state.is_design = true;
        state
    }

    /// Datos para que el hilo de guardado escriba el `.canvas`: documento
    /// clonado y píxeles RGBA de cada capa. `preview` queda en `None`: este
    /// método no tiene acceso a la GPU, así que quien la necesite (el
    /// horneado de guardado) la rellena después.
    pub fn sidecar_payload(&self) -> canvas_io::CanvasPayload {
        let images = self
            .images
            .iter()
            .map(|(id, data)| (id.raw(), data.data.data().to_vec(), data.width, data.height))
            .collect();
        canvas_io::CanvasPayload {
            document: self.doc.clone(),
            images,
            background_layer: self.background_layer.map(|id| id.raw()),
            preview: None,
        }
    }
}
