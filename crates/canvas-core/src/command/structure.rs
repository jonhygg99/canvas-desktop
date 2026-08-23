//! Comandos que mutan el ARBOL de capas: insertar, quitar, reordenar,
//! agrupar y desagrupar. Todos deben preservar el invariante de preorden de
//! `Page::layers` (ver `document::tree`).

use crate::document::Document;
use crate::error::CoreError;
use crate::layer::{Layer, LayerId};

use super::Command;

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
