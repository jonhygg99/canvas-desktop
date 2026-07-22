use std::fmt;

use crate::document::Document;
use crate::error::CoreError;
use crate::layer::{Layer, LayerId, Shadow, Transform};

/// Un paso de edición reversible (patrón Command).
///
/// Los gestos continuos (arrastrar una capa, mover un slider) NO generan un
/// comando por frame: la UI muta el documento directamente durante el gesto y,
/// al soltarlo, empuja UN comando con el estado inicial y final mediante
/// [`History::push_applied`]. Así arrastrar una capa 200 píxeles es un único
/// paso de deshacer.
pub trait Command: fmt::Debug + Send {
    fn label(&self) -> &str;
    fn apply(&mut self, doc: &mut Document) -> Result<(), CoreError>;
    fn revert(&mut self, doc: &mut Document) -> Result<(), CoreError>;
}

/// Cambia posición/tamaño/rotación de una capa (mover, redimensionar, alinear).
#[derive(Debug)]
pub struct SetTransform {
    pub layer: LayerId,
    pub before: Transform,
    pub after: Transform,
}

impl Command for SetTransform {
    fn label(&self) -> &str {
        "Transformar capa"
    }

    fn apply(&mut self, doc: &mut Document) -> Result<(), CoreError> {
        doc.layer_mut(self.layer)?.transform = self.after;
        Ok(())
    }

    fn revert(&mut self, doc: &mut Document) -> Result<(), CoreError> {
        doc.layer_mut(self.layer)?.transform = self.before;
        Ok(())
    }
}

/// Cambia el radio de desenfoque (no destructivo) de una capa.
#[derive(Debug)]
pub struct SetBlur {
    pub layer: LayerId,
    pub before: f32,
    pub after: f32,
}

impl Command for SetBlur {
    fn label(&self) -> &str {
        "Desenfoque"
    }

    fn apply(&mut self, doc: &mut Document) -> Result<(), CoreError> {
        doc.layer_mut(self.layer)?.effects.blur_radius = self.after;
        Ok(())
    }

    fn revert(&mut self, doc: &mut Document) -> Result<(), CoreError> {
        doc.layer_mut(self.layer)?.effects.blur_radius = self.before;
        Ok(())
    }
}

/// Activa/desactiva/ajusta la sombra proyectada de una capa.
#[derive(Debug)]
pub struct SetShadow {
    pub layer: LayerId,
    pub before: Option<Shadow>,
    pub after: Option<Shadow>,
}

impl Command for SetShadow {
    fn label(&self) -> &str {
        "Sombra"
    }

    fn apply(&mut self, doc: &mut Document) -> Result<(), CoreError> {
        doc.layer_mut(self.layer)?.effects.shadow = self.after;
        Ok(())
    }

    fn revert(&mut self, doc: &mut Document) -> Result<(), CoreError> {
        doc.layer_mut(self.layer)?.effects.shadow = self.before;
        Ok(())
    }
}

/// Cambia el recorte no destructivo de una capa de imagen.
#[derive(Debug)]
pub struct SetCrop {
    pub layer: LayerId,
    pub before: Option<crate::layer::CropRect>,
    pub after: Option<crate::layer::CropRect>,
}

impl SetCrop {
    fn set(
        &self,
        doc: &mut Document,
        value: Option<crate::layer::CropRect>,
    ) -> Result<(), CoreError> {
        let layer = doc.layer_mut(self.layer)?;
        let crate::layer::LayerContent::Image(content) = &mut layer.content;
        content.crop = value;
        Ok(())
    }
}

impl Command for SetCrop {
    fn label(&self) -> &str {
        "Recortar"
    }

    fn apply(&mut self, doc: &mut Document) -> Result<(), CoreError> {
        self.set(doc, self.after)
    }

    fn revert(&mut self, doc: &mut Document) -> Result<(), CoreError> {
        self.set(doc, self.before)
    }
}

/// Agrupa varios comandos en UN solo paso de deshacer: se aplican en orden y
/// se revierten en orden inverso.
#[derive(Debug)]
pub struct Composite {
    label: String,
    commands: Vec<Box<dyn Command>>,
}

impl Composite {
    pub fn new(label: impl Into<String>, commands: Vec<Box<dyn Command>>) -> Self {
        Self {
            label: label.into(),
            commands,
        }
    }
}

impl Command for Composite {
    fn label(&self) -> &str {
        &self.label
    }

    fn apply(&mut self, doc: &mut Document) -> Result<(), CoreError> {
        for cmd in &mut self.commands {
            cmd.apply(doc)?;
        }
        Ok(())
    }

    fn revert(&mut self, doc: &mut Document) -> Result<(), CoreError> {
        for cmd in self.commands.iter_mut().rev() {
            cmd.revert(doc)?;
        }
        Ok(())
    }
}

/// Cambia el tamaño (resolución) de la página activa.
#[derive(Debug)]
pub struct SetPageSize {
    pub before: (f64, f64),
    pub after: (f64, f64),
}

impl SetPageSize {
    fn set(doc: &mut Document, (w, h): (f64, f64)) -> Result<(), CoreError> {
        let page = doc.page_mut()?;
        page.width = w.max(1.0);
        page.height = h.max(1.0);
        Ok(())
    }
}

impl Command for SetPageSize {
    fn label(&self) -> &str {
        "Cambiar resolución"
    }

    fn apply(&mut self, doc: &mut Document) -> Result<(), CoreError> {
        Self::set(doc, self.after)
    }

    fn revert(&mut self, doc: &mut Document) -> Result<(), CoreError> {
        Self::set(doc, self.before)
    }
}

/// Inserta una capa ya construida en la posición dada de la pila (0 = fondo).
#[derive(Debug)]
pub struct InsertLayer {
    pub index: usize,
    pub layer: Layer,
}

impl Command for InsertLayer {
    fn label(&self) -> &str {
        "Añadir capa"
    }

    fn apply(&mut self, doc: &mut Document) -> Result<(), CoreError> {
        let page = doc.page_mut()?;
        let index = self.index.min(page.layers.len());
        page.layers.insert(index, self.layer.clone());
        Ok(())
    }

    fn revert(&mut self, doc: &mut Document) -> Result<(), CoreError> {
        let id = self.layer.id;
        let page = doc.page_mut()?;
        let pos = page
            .layers
            .iter()
            .position(|l| l.id == id)
            .ok_or(CoreError::LayerNotFound(id))?;
        page.layers.remove(pos);
        Ok(())
    }
}

/// Quita una capa de la página (recordando dónde estaba para poder rehacer).
#[derive(Debug)]
pub struct RemoveLayer {
    pub layer: LayerId,
    removed: Option<(usize, Layer)>,
}

impl RemoveLayer {
    pub fn new(layer: LayerId) -> Self {
        Self {
            layer,
            removed: None,
        }
    }
}

impl Command for RemoveLayer {
    fn label(&self) -> &str {
        "Quitar capa"
    }

    fn apply(&mut self, doc: &mut Document) -> Result<(), CoreError> {
        let page = doc.page_mut()?;
        let pos = page
            .layers
            .iter()
            .position(|l| l.id == self.layer)
            .ok_or(CoreError::LayerNotFound(self.layer))?;
        self.removed = Some((pos, page.layers.remove(pos)));
        Ok(())
    }

    fn revert(&mut self, doc: &mut Document) -> Result<(), CoreError> {
        let (index, layer) = self
            .removed
            .take()
            .ok_or(CoreError::LayerNotFound(self.layer))?;
        let page = doc.page_mut()?;
        let index = index.min(page.layers.len());
        page.layers.insert(index, layer);
        Ok(())
    }
}

/// Historial de deshacer/rehacer basado en comandos, con marca de guardado
/// para derivar el estado sucio (dirty) sin un flag manual.
pub struct History {
    undo: Vec<Box<dyn Command>>,
    redo: Vec<Box<dyn Command>>,
    /// Longitud de la pila de undo en el último guardado. `None` si el estado
    /// guardado ya no es alcanzable deshaciendo/rehaciendo.
    saved_depth: Option<usize>,
    limit: usize,
}

impl Default for History {
    fn default() -> Self {
        Self::with_limit(200)
    }
}

impl History {
    pub fn with_limit(limit: usize) -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            saved_depth: Some(0),
            limit: limit.max(1),
        }
    }

    /// Aplica el comando al documento y lo apila.
    pub fn apply(
        &mut self,
        doc: &mut Document,
        mut cmd: Box<dyn Command>,
    ) -> Result<(), CoreError> {
        cmd.apply(doc)?;
        self.push_applied(cmd);
        Ok(())
    }

    /// Apila un comando cuyo efecto YA está reflejado en el documento (final
    /// de un gesto continuo).
    pub fn push_applied(&mut self, cmd: Box<dyn Command>) {
        // Si el punto de guardado quedaba por delante (en la pila de redo que
        // vamos a vaciar), deja de ser alcanzable.
        if self.saved_depth.is_some_and(|d| d > self.undo.len()) {
            self.saved_depth = None;
        }
        self.redo.clear();
        self.undo.push(cmd);
        if self.undo.len() > self.limit {
            self.undo.remove(0);
            self.saved_depth = match self.saved_depth {
                Some(0) | None => None,
                Some(d) => Some(d - 1),
            };
        }
    }

    /// Deshace el último comando. Devuelve `false` si no había nada que
    /// deshacer.
    pub fn undo(&mut self, doc: &mut Document) -> Result<bool, CoreError> {
        let Some(cmd) = self.undo.last_mut() else {
            return Ok(false);
        };
        cmd.revert(doc)?;
        let cmd = self.undo.pop().unwrap_or_else(|| unreachable!());
        self.redo.push(cmd);
        Ok(true)
    }

    /// Rehace el último comando deshecho. Devuelve `false` si no había nada.
    pub fn redo(&mut self, doc: &mut Document) -> Result<bool, CoreError> {
        let Some(cmd) = self.redo.last_mut() else {
            return Ok(false);
        };
        cmd.apply(doc)?;
        let cmd = self.redo.pop().unwrap_or_else(|| unreachable!());
        self.undo.push(cmd);
        Ok(true)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Marca el estado actual como guardado en disco.
    pub fn mark_saved(&mut self) {
        self.saved_depth = Some(self.undo.len());
    }

    /// ¿Hay cambios sin guardar respecto al último `mark_saved`?
    pub fn is_dirty(&self) -> bool {
        self.saved_depth != Some(self.undo.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer::{ImageContent, LayerContent};

    fn doc_with_layer() -> (Document, LayerId) {
        let mut doc = Document::new(800.0, 600.0);
        let id = doc
            .add_layer(
                "img",
                Transform::new(10.0, 20.0, 100.0, 50.0),
                LayerContent::Image(ImageContent {
                    source_path: None,
                    natural_width: 100,
                    natural_height: 50,
                    crop: None,
                }),
            )
            .expect("documento recién creado tiene página");
        (doc, id)
    }

    fn move_cmd(layer: LayerId, before: Transform, x: f64, y: f64) -> Box<dyn Command> {
        Box::new(SetTransform {
            layer,
            before,
            after: Transform { x, y, ..before },
        })
    }

    #[test]
    fn apply_undo_redo_roundtrip() {
        let (mut doc, id) = doc_with_layer();
        let before = doc.layer(id).unwrap().transform;
        let mut history = History::default();

        history
            .apply(&mut doc, move_cmd(id, before, 200.0, 300.0))
            .unwrap();
        assert_eq!(doc.layer(id).unwrap().transform.x, 200.0);

        assert!(history.undo(&mut doc).unwrap());
        assert_eq!(doc.layer(id).unwrap().transform, before);

        assert!(history.redo(&mut doc).unwrap());
        assert_eq!(doc.layer(id).unwrap().transform.x, 200.0);
        assert_eq!(doc.layer(id).unwrap().transform.y, 300.0);
    }

    #[test]
    fn undo_on_empty_history_is_noop() {
        let (mut doc, _) = doc_with_layer();
        let mut history = History::default();
        assert!(!history.undo(&mut doc).unwrap());
        assert!(!history.redo(&mut doc).unwrap());
    }

    #[test]
    fn new_command_clears_redo() {
        let (mut doc, id) = doc_with_layer();
        let before = doc.layer(id).unwrap().transform;
        let mut history = History::default();

        history
            .apply(&mut doc, move_cmd(id, before, 200.0, 300.0))
            .unwrap();
        history.undo(&mut doc).unwrap();
        assert!(history.can_redo());

        history
            .apply(&mut doc, move_cmd(id, before, 50.0, 60.0))
            .unwrap();
        assert!(!history.can_redo());
        assert_eq!(doc.layer(id).unwrap().transform.x, 50.0);
    }

    #[test]
    fn drag_coalesces_into_single_undo_step() {
        let (mut doc, id) = doc_with_layer();
        let start = doc.layer(id).unwrap().transform;
        let mut history = History::default();

        // Simula un arrastre de 200 frames: mutación directa, sin comandos.
        for i in 1..=200 {
            doc.layer_mut(id).unwrap().transform.x = start.x + f64::from(i);
        }
        let end = doc.layer(id).unwrap().transform;
        history.push_applied(Box::new(SetTransform {
            layer: id,
            before: start,
            after: end,
        }));

        // UN solo paso de deshacer devuelve al estado inicial.
        assert!(history.undo(&mut doc).unwrap());
        assert_eq!(doc.layer(id).unwrap().transform, start);
        assert!(!history.can_undo());
    }

    #[test]
    fn dirty_tracks_saved_position() {
        let (mut doc, id) = doc_with_layer();
        let before = doc.layer(id).unwrap().transform;
        let mut history = History::default();
        assert!(
            !history.is_dirty(),
            "documento recién abierto no está sucio"
        );

        history
            .apply(&mut doc, move_cmd(id, before, 1.0, 1.0))
            .unwrap();
        assert!(history.is_dirty());

        history.undo(&mut doc).unwrap();
        assert!(
            !history.is_dirty(),
            "deshacer hasta el estado guardado limpia el sucio"
        );

        history.redo(&mut doc).unwrap();
        assert!(history.is_dirty());

        history.mark_saved();
        assert!(!history.is_dirty());

        history.undo(&mut doc).unwrap();
        assert!(
            history.is_dirty(),
            "deshacer por detrás del guardado ensucia"
        );
    }

    #[test]
    fn saved_state_unreachable_after_diverging() {
        let (mut doc, id) = doc_with_layer();
        let before = doc.layer(id).unwrap().transform;
        let mut history = History::default();

        history
            .apply(&mut doc, move_cmd(id, before, 1.0, 1.0))
            .unwrap();
        history.mark_saved();
        history.undo(&mut doc).unwrap();
        // Nueva rama: el estado guardado ya no es alcanzable.
        history
            .apply(&mut doc, move_cmd(id, before, 2.0, 2.0))
            .unwrap();
        assert!(history.is_dirty());
        history.undo(&mut doc).unwrap();
        assert!(
            history.is_dirty(),
            "ni siquiera igualando la longitud de pila"
        );
    }

    #[test]
    fn composite_applies_in_order_and_reverts_in_reverse() {
        let (mut doc, id) = doc_with_layer();
        let start = doc.layer(id).unwrap().transform;
        let mut history = History::default();

        // Dos pasos encadenados: el segundo parte del resultado del primero.
        let step1 = Transform { x: 100.0, ..start };
        let step2 = Transform { y: 200.0, ..step1 };
        history
            .apply(
                &mut doc,
                Box::new(Composite::new(
                    "mover dos veces",
                    vec![
                        Box::new(SetTransform {
                            layer: id,
                            before: start,
                            after: step1,
                        }),
                        Box::new(SetTransform {
                            layer: id,
                            before: step1,
                            after: step2,
                        }),
                    ],
                )),
            )
            .unwrap();
        assert_eq!(doc.layer(id).unwrap().transform, step2);

        // UN solo deshacer revierte todo el grupo, en orden inverso.
        history.undo(&mut doc).unwrap();
        assert_eq!(doc.layer(id).unwrap().transform, start);
        assert!(!history.can_undo());

        history.redo(&mut doc).unwrap();
        assert_eq!(doc.layer(id).unwrap().transform, step2);
    }

    #[test]
    fn set_shadow_roundtrips() {
        let (mut doc, id) = doc_with_layer();
        let mut history = History::default();
        let shadow = crate::Shadow::default();

        history
            .apply(
                &mut doc,
                Box::new(SetShadow {
                    layer: id,
                    before: None,
                    after: Some(shadow),
                }),
            )
            .unwrap();
        assert_eq!(doc.layer(id).unwrap().effects.shadow, Some(shadow));

        history.undo(&mut doc).unwrap();
        assert_eq!(doc.layer(id).unwrap().effects.shadow, None);
    }

    #[test]
    fn set_page_size_roundtrips() {
        let (mut doc, _) = doc_with_layer();
        let mut history = History::default();
        history
            .apply(
                &mut doc,
                Box::new(SetPageSize {
                    before: (800.0, 600.0),
                    after: (1920.0, 1080.0),
                }),
            )
            .unwrap();
        let page = doc.page().unwrap();
        assert_eq!((page.width, page.height), (1920.0, 1080.0));

        history.undo(&mut doc).unwrap();
        let page = doc.page().unwrap();
        assert_eq!((page.width, page.height), (800.0, 600.0));
    }

    #[test]
    fn insert_and_remove_layer_undo_redo() {
        let (mut doc, existing) = doc_with_layer();
        let mut history = History::default();

        // Inserta una capa nueva en el fondo (índice 0).
        let id = doc.allocate_layer_id();
        let layer = crate::Layer::new(
            id,
            "fondo",
            Transform::new(0.0, 0.0, 10.0, 10.0),
            LayerContent::Image(ImageContent {
                source_path: None,
                natural_width: 10,
                natural_height: 10,
                crop: None,
            }),
        );
        history
            .apply(&mut doc, Box::new(InsertLayer { index: 0, layer }))
            .unwrap();
        assert_eq!(doc.page().unwrap().layers[0].id, id, "insertada al fondo");
        assert_eq!(doc.page().unwrap().layers.len(), 2);

        history.undo(&mut doc).unwrap();
        assert_eq!(doc.page().unwrap().layers.len(), 1);
        assert_eq!(doc.page().unwrap().layers[0].id, existing);

        history.redo(&mut doc).unwrap();
        assert_eq!(doc.page().unwrap().layers[0].id, id);

        // Y ahora quitarla, con deshacer que la devuelve a su sitio.
        history
            .apply(&mut doc, Box::new(RemoveLayer::new(id)))
            .unwrap();
        assert!(doc.layer(id).is_err());
        history.undo(&mut doc).unwrap();
        assert_eq!(doc.page().unwrap().layers[0].id, id, "vuelve al índice 0");
        history.redo(&mut doc).unwrap();
        assert!(doc.layer(id).is_err());
    }

    #[test]
    fn history_limit_drops_oldest() {
        let (mut doc, id) = doc_with_layer();
        let before = doc.layer(id).unwrap().transform;
        let mut history = History::with_limit(5);

        for i in 0..8 {
            history
                .apply(&mut doc, move_cmd(id, before, f64::from(i), 0.0))
                .unwrap();
        }
        let mut undone = 0;
        while history.undo(&mut doc).unwrap() {
            undone += 1;
        }
        assert_eq!(undone, 5);
        assert!(
            history.is_dirty(),
            "el estado inicial se perdió del historial"
        );
    }
}
