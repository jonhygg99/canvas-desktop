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

/// Cambia el bloque completo de efectos de una capa (los sliders de ajuste
/// de color se consolidan en un solo paso con este comando).
#[derive(Debug)]
pub struct SetEffects {
    pub layer: LayerId,
    pub before: crate::layer::Effects,
    pub after: crate::layer::Effects,
}

impl Command for SetEffects {
    fn label(&self) -> &str {
        "Ajustes"
    }

    fn apply(&mut self, doc: &mut Document) -> Result<(), CoreError> {
        doc.layer_mut(self.layer)?.effects = self.after;
        Ok(())
    }

    fn revert(&mut self, doc: &mut Document) -> Result<(), CoreError> {
        doc.layer_mut(self.layer)?.effects = self.before;
        Ok(())
    }
}

/// Sustituye el contenido completo de una capa (edición de texto o de las
/// propiedades de una forma, consolidada en un paso).
#[derive(Debug)]
pub struct SetContent {
    pub layer: LayerId,
    pub before: crate::layer::LayerContent,
    pub after: crate::layer::LayerContent,
}

impl Command for SetContent {
    fn label(&self) -> &str {
        "Editar contenido"
    }

    fn apply(&mut self, doc: &mut Document) -> Result<(), CoreError> {
        doc.layer_mut(self.layer)?.content = self.after.clone();
        Ok(())
    }

    fn revert(&mut self, doc: &mut Document) -> Result<(), CoreError> {
        doc.layer_mut(self.layer)?.content = self.before.clone();
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
        if let crate::layer::LayerContent::Image(content) = &mut layer.content {
            content.crop = value;
        }
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

/// Quita una capa de la página, CON todo su subárbol si es un grupo
/// (recordando dónde estaba para poder rehacer).
#[derive(Debug)]
pub struct RemoveLayer {
    pub layer: LayerId,
    removed: Option<(usize, Vec<Layer>)>,
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
            .index_of(self.layer)
            .ok_or(CoreError::LayerNotFound(self.layer))?;
        let len = 1 + page.subtree_len(pos);
        let removed: Vec<Layer> = page.layers.drain(pos..pos + len).collect();
        self.removed = Some((pos, removed));
        Ok(())
    }

    fn revert(&mut self, doc: &mut Document) -> Result<(), CoreError> {
        let (index, layers) = self
            .removed
            .take()
            .ok_or(CoreError::LayerNotFound(self.layer))?;
        let page = doc.page_mut()?;
        let index = index.min(page.layers.len());
        page.layers.splice(index..index, layers);
        Ok(())
    }
}

/// Mueve una capa (con su subárbol, si es un grupo) a otra posición y/o a
/// otro grupo. Es el comando del arrastre en el panel de capas.
#[derive(Debug)]
pub struct Reorder {
    pub layer: LayerId,
    pub new_parent: Option<LayerId>,
    /// Posición entre los hijos directos del destino, contada SIN la capa
    /// movida (0 = el más bajo).
    pub new_index: usize,
    before: Option<(Option<LayerId>, usize)>,
}

impl Reorder {
    pub fn new(layer: LayerId, new_parent: Option<LayerId>, new_index: usize) -> Self {
        Self {
            layer,
            new_parent,
            new_index,
            before: None,
        }
    }
}

impl Command for Reorder {
    fn label(&self) -> &str {
        "Reordenar capas"
    }

    fn apply(&mut self, doc: &mut Document) -> Result<(), CoreError> {
        let page = doc.page_mut()?;
        let parent = page
            .layer(self.layer)
            .ok_or(CoreError::LayerNotFound(self.layer))?
            .parent_id;
        let index = page.sibling_index(self.layer).unwrap_or(0);
        self.before = Some((parent, index));
        page.move_subtree(self.layer, self.new_parent, self.new_index)
    }

    fn revert(&mut self, doc: &mut Document) -> Result<(), CoreError> {
        let (parent, index) = self
            .before
            .take()
            .ok_or(CoreError::LayerNotFound(self.layer))?;
        doc.page_mut()?.move_subtree(self.layer, parent, index)
    }
}

/// Mete varias capas en un grupo nuevo, insertado en la posición de la capa
/// seleccionada más alta (las que ya son descendientes de otro miembro del
/// conjunto se descartan: agrupar un grupo ya arrastra a sus hijos consigo).
#[derive(Debug)]
pub struct Group {
    pub layers: Vec<LayerId>,
    /// Id ya reservado con `Document::allocate_layer_id`.
    pub group: LayerId,
    pub name: String,
    before: Vec<(LayerId, Option<LayerId>, usize)>,
}

impl Group {
    pub fn new(layers: Vec<LayerId>, group: LayerId, name: impl Into<String>) -> Self {
        Self {
            layers,
            group,
            name: name.into(),
            before: Vec::new(),
        }
    }
}

impl Command for Group {
    fn label(&self) -> &str {
        "Agrupar"
    }

    fn apply(&mut self, doc: &mut Document) -> Result<(), CoreError> {
        self.before.clear();
        let page = doc.page_mut()?;
        let mut members: Vec<LayerId> = self
            .layers
            .iter()
            .copied()
            .filter(|&id| {
                page.layer(id).is_some()
                    && !self
                        .layers
                        .iter()
                        .any(|&other| other != id && page.is_ancestor(other, id))
            })
            .collect();
        if members.is_empty() {
            return Ok(());
        }
        // Orden de pila (de abajo arriba), para elegir al miembro más alto.
        members.sort_by_key(|&id| page.index_of(id).unwrap_or(usize::MAX));

        let topmost = *members
            .last()
            .unwrap_or_else(|| unreachable!("comprobado arriba"));
        let parent = page.layer(topmost).and_then(|l| l.parent_id);
        let slot = page.sibling_index(topmost).map_or(0, |i| i + 1);
        page.insert_child(Layer::group(self.group, self.name.clone()), parent, slot);

        // Captura la posición ORIGINAL de cada miembro con el grupo YA
        // insertado pero ANTES de mover a ninguno: si se capturase dentro del
        // propio bucle, cada movimiento desplazaría el índice de hermano de
        // los miembros siguientes y el deshacer no restauraría el orden real.
        for &id in &members {
            let before_parent = page.layer(id).and_then(|l| l.parent_id);
            let before_index = page.sibling_index(id).unwrap_or(0);
            self.before.push((id, before_parent, before_index));
        }
        for (k, &id) in members.iter().enumerate() {
            page.move_subtree(id, Some(self.group), k)?;
        }
        page.refresh_group_bounds(self.group);
        Ok(())
    }

    fn revert(&mut self, doc: &mut Document) -> Result<(), CoreError> {
        let page = doc.page_mut()?;
        let mut restore = std::mem::take(&mut self.before);
        // Restaura en orden ASCENDENTE de índice de hermano: cada inserción
        // encuentra ya en su sitio a los hermanos de índice menor.
        restore.sort_by_key(|&(_, _, index)| index);
        for (id, parent, index) in restore {
            page.move_subtree(id, parent, index)?;
        }
        // El grupo queda vacío: se quita entero.
        if let Some(pos) = page.index_of(self.group) {
            page.layers.remove(pos);
        }
        Ok(())
    }
}

/// Disuelve un grupo: sus hijos DIRECTOS ocupan su hueco en la pila, en el
/// mismo orden relativo que tenían dentro de él.
#[derive(Debug)]
pub struct Ungroup {
    pub group: LayerId,
    removed: Option<(Layer, Option<LayerId>, usize, Vec<LayerId>)>,
}

impl Ungroup {
    pub fn new(group: LayerId) -> Self {
        Self {
            group,
            removed: None,
        }
    }
}

impl Command for Ungroup {
    fn label(&self) -> &str {
        "Desagrupar"
    }

    fn apply(&mut self, doc: &mut Document) -> Result<(), CoreError> {
        let page = doc.page_mut()?;
        if !page.is_group(self.group) {
            return Err(CoreError::NotAGroup(self.group));
        }
        let group_layer = page
            .layer(self.group)
            .cloned()
            .ok_or(CoreError::LayerNotFound(self.group))?;
        let parent = group_layer.parent_id;
        let index = page.sibling_index(self.group).unwrap_or(0);
        let children = page.children_of(Some(self.group));
        self.removed = Some((group_layer, parent, index, children.clone()));
        for (k, child) in children.iter().enumerate() {
            page.move_subtree(*child, parent, index + k)?;
        }
        // El grupo queda vacío: se quita entero.
        if let Some(pos) = page.index_of(self.group) {
            page.layers.remove(pos);
        }
        Ok(())
    }

    fn revert(&mut self, doc: &mut Document) -> Result<(), CoreError> {
        let (group_layer, parent, index, children) = self
            .removed
            .take()
            .ok_or(CoreError::LayerNotFound(self.group))?;
        let page = doc.page_mut()?;
        // El grupo vuelve a su hueco ORIGINAL entre sus hermanos (los hijos,
        // que ahora ocupan ese tramo como raíces temporales, se anidan justo
        // después: no hace falta +n, la inserción los desplaza solos).
        page.insert_child(group_layer, parent, index);
        for (k, child) in children.iter().enumerate() {
            page.move_subtree(*child, Some(self.group), k)?;
        }
        Ok(())
    }
}

/// Renombra una capa.
#[derive(Debug)]
pub struct Rename {
    pub layer: LayerId,
    pub before: String,
    pub after: String,
}

impl Command for Rename {
    fn label(&self) -> &str {
        "Renombrar capa"
    }

    fn apply(&mut self, doc: &mut Document) -> Result<(), CoreError> {
        doc.layer_mut(self.layer)?.name = self.after.clone();
        Ok(())
    }

    fn revert(&mut self, doc: &mut Document) -> Result<(), CoreError> {
        doc.layer_mut(self.layer)?.name = self.before.clone();
        Ok(())
    }
}

/// Muestra/oculta una capa (o un grupo entero: la visibilidad se hereda,
/// ver `Page::effective_visible`).
#[derive(Debug)]
pub struct SetVisible {
    pub layer: LayerId,
    pub before: bool,
    pub after: bool,
}

impl Command for SetVisible {
    fn label(&self) -> &str {
        "Mostrar/ocultar capa"
    }

    fn apply(&mut self, doc: &mut Document) -> Result<(), CoreError> {
        doc.layer_mut(self.layer)?.visible = self.after;
        Ok(())
    }

    fn revert(&mut self, doc: &mut Document) -> Result<(), CoreError> {
        doc.layer_mut(self.layer)?.visible = self.before;
        Ok(())
    }
}

/// Bloquea/desbloquea una capa (o un grupo entero: el bloqueo se hereda).
#[derive(Debug)]
pub struct SetLocked {
    pub layer: LayerId,
    pub before: bool,
    pub after: bool,
}

impl Command for SetLocked {
    fn label(&self) -> &str {
        "Bloquear capa"
    }

    fn apply(&mut self, doc: &mut Document) -> Result<(), CoreError> {
        doc.layer_mut(self.layer)?.locked = self.after;
        Ok(())
    }

    fn revert(&mut self, doc: &mut Document) -> Result<(), CoreError> {
        doc.layer_mut(self.layer)?.locked = self.before;
        Ok(())
    }
}

/// Cambia la opacidad de una capa (o de un grupo entero: las opacidades de
/// la cadena se multiplican, ver `Page::effective_opacity`).
#[derive(Debug)]
pub struct SetOpacity {
    pub layer: LayerId,
    pub before: f32,
    pub after: f32,
}

impl Command for SetOpacity {
    fn label(&self) -> &str {
        "Opacidad"
    }

    fn apply(&mut self, doc: &mut Document) -> Result<(), CoreError> {
        doc.layer_mut(self.layer)?.opacity = self.after;
        Ok(())
    }

    fn revert(&mut self, doc: &mut Document) -> Result<(), CoreError> {
        doc.layer_mut(self.layer)?.opacity = self.before;
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

    fn image_content() -> LayerContent {
        LayerContent::Image(ImageContent {
            source_path: None,
            natural_width: 10,
            natural_height: 10,
            crop: None,
        })
    }

    /// Documento con dos capas de imagen raíz: `a` (más abajo), `b` (más
    /// arriba). Sin agrupar todavía: cada test construye el árbol que
    /// necesita con los propios comandos que está probando.
    fn doc_with_two_layers() -> (Document, LayerId, LayerId) {
        let mut doc = Document::new(800.0, 600.0);
        let a = doc
            .add_layer("a", Transform::new(0.0, 0.0, 10.0, 10.0), image_content())
            .unwrap();
        let b = doc
            .add_layer("b", Transform::new(20.0, 20.0, 10.0, 10.0), image_content())
            .unwrap();
        (doc, a, b)
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

    #[test]
    fn reorder_moves_a_layer_and_undo_puts_it_back() {
        let (mut doc, a, b) = doc_with_two_layers();
        let mut history = History::default();
        assert_eq!(doc.page().unwrap().children_of(None), vec![a, b]);

        history
            .apply(&mut doc, Box::new(Reorder::new(a, None, 1)))
            .unwrap();
        assert_eq!(doc.page().unwrap().children_of(None), vec![b, a]);

        history.undo(&mut doc).unwrap();
        assert_eq!(doc.page().unwrap().children_of(None), vec![a, b]);
    }

    #[test]
    fn reorder_reparents_a_whole_subtree() {
        let (mut doc, a, _b) = doc_with_two_layers();
        let mut history = History::default();
        let group_id = doc.allocate_layer_id();
        history
            .apply(&mut doc, Box::new(Group::new(vec![a], group_id, "G")))
            .unwrap();
        let c = doc
            .add_layer("c", Transform::new(40.0, 40.0, 10.0, 10.0), image_content())
            .unwrap();
        let outer_id = doc.allocate_layer_id();
        history
            .apply(&mut doc, Box::new(Group::new(vec![c], outer_id, "Outer")))
            .unwrap();

        history
            .apply(
                &mut doc,
                Box::new(Reorder::new(group_id, Some(outer_id), 0)),
            )
            .unwrap();
        let page = doc.page().unwrap();
        assert_eq!(page.layer(a).unwrap().parent_id, Some(group_id));
        assert_eq!(page.layer(group_id).unwrap().parent_id, Some(outer_id));
        assert!(page.is_ancestor(outer_id, a));

        history.undo(&mut doc).unwrap();
        let page = doc.page().unwrap();
        assert_eq!(page.layer(group_id).unwrap().parent_id, None);
        assert!(!page.is_ancestor(outer_id, a));
    }

    #[test]
    fn group_wraps_the_selection_and_undo_restores_the_order() {
        let (mut doc, a, b) = doc_with_two_layers();
        let group_id = doc.allocate_layer_id();
        let mut history = History::default();
        history
            .apply(
                &mut doc,
                Box::new(Group::new(vec![a, b], group_id, "Group")),
            )
            .unwrap();

        let page = doc.page().unwrap();
        assert_eq!(page.children_of(None), vec![group_id]);
        assert_eq!(page.children_of(Some(group_id)), vec![a, b]);

        history.undo(&mut doc).unwrap();
        let page = doc.page().unwrap();
        assert_eq!(page.children_of(None), vec![a, b]);
        assert!(
            page.layer(group_id).is_none(),
            "el grupo desaparece al deshacer"
        );
    }

    #[test]
    fn group_ignores_children_whose_parent_is_also_selected() {
        let (mut doc, a, b) = doc_with_two_layers();
        let inner_id = doc.allocate_layer_id();
        let mut history = History::default();
        history
            .apply(&mut doc, Box::new(Group::new(vec![a], inner_id, "Inner")))
            .unwrap();

        // Selecciona el grupo interno Y su hijo "a": "a" debe descartarse
        // porque ya es descendiente de "inner_id".
        let outer_id = doc.allocate_layer_id();
        history
            .apply(
                &mut doc,
                Box::new(Group::new(vec![inner_id, a, b], outer_id, "Outer")),
            )
            .unwrap();

        let page = doc.page().unwrap();
        assert_eq!(page.children_of(Some(outer_id)), vec![inner_id, b]);
        assert_eq!(page.layer(a).unwrap().parent_id, Some(inner_id));
    }

    #[test]
    fn ungroup_dissolves_the_group_in_place() {
        let (mut doc, a, b) = doc_with_two_layers();
        let group_id = doc.allocate_layer_id();
        let mut history = History::default();
        history
            .apply(
                &mut doc,
                Box::new(Group::new(vec![a, b], group_id, "Group")),
            )
            .unwrap();

        history
            .apply(&mut doc, Box::new(Ungroup::new(group_id)))
            .unwrap();
        let page = doc.page().unwrap();
        assert_eq!(page.children_of(None), vec![a, b]);
        assert!(page.layer(group_id).is_none());
    }

    #[test]
    fn group_then_ungroup_is_the_identity() {
        let (mut doc, a, b) = doc_with_two_layers();
        let group_id = doc.allocate_layer_id();
        let mut history = History::default();

        history
            .apply(
                &mut doc,
                Box::new(Group::new(vec![a, b], group_id, "Group")),
            )
            .unwrap();
        history
            .apply(&mut doc, Box::new(Ungroup::new(group_id)))
            .unwrap();

        let page = doc.page().unwrap();
        assert_eq!(page.children_of(None), vec![a, b]);
        assert_eq!(page.layer(a).unwrap().parent_id, None);
        assert_eq!(page.layer(b).unwrap().parent_id, None);

        // Y el propio deshacer/rehacer de los dos pasos también cierra bien.
        history.undo(&mut doc).unwrap(); // deshace Ungroup
        assert!(doc.page().unwrap().is_group(group_id));
        history.undo(&mut doc).unwrap(); // deshace Group
        assert_eq!(doc.page().unwrap().children_of(None), vec![a, b]);
        assert!(doc.page().unwrap().layer(group_id).is_none());
    }

    #[test]
    fn rename_roundtrips() {
        let (mut doc, id) = doc_with_layer();
        let mut history = History::default();
        history
            .apply(
                &mut doc,
                Box::new(Rename {
                    layer: id,
                    before: "img".to_owned(),
                    after: "Renamed".to_owned(),
                }),
            )
            .unwrap();
        assert_eq!(doc.layer(id).unwrap().name, "Renamed");
        history.undo(&mut doc).unwrap();
        assert_eq!(doc.layer(id).unwrap().name, "img");
    }

    #[test]
    fn set_visible_locked_and_opacity_roundtrip() {
        let (mut doc, id) = doc_with_layer();
        let mut history = History::default();
        history
            .apply(
                &mut doc,
                Box::new(SetVisible {
                    layer: id,
                    before: true,
                    after: false,
                }),
            )
            .unwrap();
        assert!(!doc.layer(id).unwrap().visible);
        history
            .apply(
                &mut doc,
                Box::new(SetLocked {
                    layer: id,
                    before: false,
                    after: true,
                }),
            )
            .unwrap();
        assert!(doc.layer(id).unwrap().locked);
        history
            .apply(
                &mut doc,
                Box::new(SetOpacity {
                    layer: id,
                    before: 1.0,
                    after: 0.4,
                }),
            )
            .unwrap();
        assert!((doc.layer(id).unwrap().opacity - 0.4).abs() < 1e-6);

        history.undo(&mut doc).unwrap();
        assert!((doc.layer(id).unwrap().opacity - 1.0).abs() < 1e-6);
        history.undo(&mut doc).unwrap();
        assert!(!doc.layer(id).unwrap().locked);
        history.undo(&mut doc).unwrap();
        assert!(doc.layer(id).unwrap().visible);
    }

    #[test]
    fn remove_layer_takes_the_whole_subtree_with_it() {
        let (mut doc, a, b) = doc_with_two_layers();
        let group_id = doc.allocate_layer_id();
        let mut history = History::default();
        history
            .apply(
                &mut doc,
                Box::new(Group::new(vec![a, b], group_id, "Group")),
            )
            .unwrap();

        history
            .apply(&mut doc, Box::new(RemoveLayer::new(group_id)))
            .unwrap();
        assert!(doc.page().unwrap().layers.is_empty());

        history.undo(&mut doc).unwrap();
        let page = doc.page().unwrap();
        assert_eq!(page.children_of(None), vec![group_id]);
        assert_eq!(page.children_of(Some(group_id)), vec![a, b]);
    }

    #[test]
    fn nested_groups_survive_undo_redo() {
        let (mut doc, a, b) = doc_with_two_layers();
        let mut history = History::default();
        let inner = doc.allocate_layer_id();
        history
            .apply(&mut doc, Box::new(Group::new(vec![a], inner, "Inner")))
            .unwrap();
        let middle = doc.allocate_layer_id();
        history
            .apply(
                &mut doc,
                Box::new(Group::new(vec![inner], middle, "Middle")),
            )
            .unwrap();
        let outer = doc.allocate_layer_id();
        history
            .apply(&mut doc, Box::new(Group::new(vec![middle], outer, "Outer")))
            .unwrap();

        let page = doc.page().unwrap();
        assert_eq!(page.depth(a), 3, "a está dentro de outer > middle > inner");
        assert!(page.is_ancestor(outer, a));

        for _ in 0..3 {
            history.undo(&mut doc).unwrap();
        }
        assert_eq!(doc.page().unwrap().children_of(None), vec![a, b]);

        for _ in 0..3 {
            history.redo(&mut doc).unwrap();
        }
        let page = doc.page().unwrap();
        assert!(page.is_ancestor(outer, a));
        assert_eq!(page.depth(a), 3);
    }
}
