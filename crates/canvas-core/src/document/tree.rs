//! El arbol de grupos sobre la lista plana `Page::layers`: recorrido,
//! herencia de visibilidad/bloqueo/opacidad, y las UNICAS mutaciones que
//! preservan el invariante de preorden (`move_subtree`/`insert_child`).

use std::collections::{HashMap, HashSet};

use crate::error::CoreError;
use crate::layer::{Layer, LayerContent, LayerId};

use super::Page;

impl Page {
    /// Número de descendientes contiguos de la capa en `index` (0 si no es
    /// grupo o no tiene hijos). Se apoya en la invariante de preorden.
    pub fn subtree_len(&self, index: usize) -> usize {
        let Some(root) = self.layers.get(index).map(|l| l.id) else {
            return 0;
        };
        self.layers[index + 1..]
            .iter()
            .take_while(|l| self.is_ancestor(root, l.id))
            .count()
    }

    /// Hijos DIRECTOS de `parent` (`None` = raíces de la pila), de abajo
    /// arriba.
    pub fn children_of(&self, parent: Option<LayerId>) -> Vec<LayerId> {
        self.layers
            .iter()
            .filter(|l| l.parent_id == parent)
            .map(|l| l.id)
            .collect()
    }

    /// Todos los descendientes a cualquier profundidad, de abajo arriba.
    pub fn descendants(&self, id: LayerId) -> Vec<LayerId> {
        let Some(index) = self.index_of(id) else {
            return Vec::new();
        };
        let len = self.subtree_len(index);
        self.layers[index + 1..index + 1 + len]
            .iter()
            .map(|l| l.id)
            .collect()
    }

    /// ¿`ancestor` está en la cadena de padres de `id`? (`false` si son
    /// iguales o si `id` no tiene ancestros). Acotado a `layers.len()` saltos
    /// para no colgarse ante una cadena corrupta o cíclica.
    pub fn is_ancestor(&self, ancestor: LayerId, id: LayerId) -> bool {
        let mut current = id;
        for _ in 0..self.layers.len() {
            let Some(parent) = self.layer(current).and_then(|l| l.parent_id) else {
                return false;
            };
            if parent == ancestor {
                return true;
            }
            current = parent;
        }
        false
    }

    /// Profundidad de anidamiento (0 = raíz). Acotada a `layers.len()`
    /// saltos.
    pub fn depth(&self, id: LayerId) -> usize {
        let mut current = id;
        let mut depth = 0;
        for _ in 0..self.layers.len() {
            let Some(parent) = self.layer(current).and_then(|l| l.parent_id) else {
                break;
            };
            depth += 1;
            current = parent;
        }
        depth
    }

    /// La capa y TODOS sus ancestros son visibles. Una cadena que no termina
    /// en una raíz en `layers.len()` pasos se trata como invisible (seguro
    /// por defecto).
    pub fn effective_visible(&self, id: LayerId) -> bool {
        let mut current = id;
        for _ in 0..self.layers.len() {
            let Some(layer) = self.layer(current) else {
                return false;
            };
            if !layer.visible {
                return false;
            }
            let Some(parent) = layer.parent_id else {
                return true;
            };
            current = parent;
        }
        false
    }

    /// La capa o CUALQUIER ancestro está bloqueado.
    pub fn effective_locked(&self, id: LayerId) -> bool {
        let mut current = id;
        for _ in 0..self.layers.len() {
            let Some(layer) = self.layer(current) else {
                return true;
            };
            if layer.locked {
                return true;
            }
            let Some(parent) = layer.parent_id else {
                return false;
            };
            current = parent;
        }
        true
    }

    /// Producto de las opacidades de la cadena (capa y todos sus ancestros).
    pub fn effective_opacity(&self, id: LayerId) -> f32 {
        let mut result = 1.0f32;
        let mut current = id;
        for _ in 0..self.layers.len() {
            let Some(layer) = self.layer(current) else {
                return 0.0;
            };
            result *= layer.opacity.clamp(0.0, 1.0);
            let Some(parent) = layer.parent_id else {
                return result;
            };
            current = parent;
        }
        0.0
    }

    /// Índice ABSOLUTO donde insertar para quedar como hijo número `index` de
    /// `parent`, contando la lista de hijos actual (SIN ninguna capa que ya
    /// se haya extraído de `self.layers` para moverla).
    fn insertion_index(&self, parent: Option<LayerId>, index: usize) -> usize {
        let children = self.children_of(parent);
        match children.get(index) {
            Some(&next) => self.index_of(next).unwrap_or(self.layers.len()),
            None => match children.last() {
                Some(&last) => {
                    let i = self.index_of(last).unwrap_or(0);
                    i + 1 + self.subtree_len(i)
                }
                None => parent.and_then(|p| self.index_of(p)).map_or(0, |i| i + 1),
            },
        }
    }

    /// Mueve `layer` (con todo su subárbol) a ser el hijo número `index` de
    /// `parent` (`None` = raíz de la pila). Rechaza convertir un grupo en su
    /// propio descendiente y exige que `parent` sea de verdad un grupo.
    pub fn move_subtree(
        &mut self,
        layer: LayerId,
        parent: Option<LayerId>,
        index: usize,
    ) -> Result<(), CoreError> {
        let from = self
            .index_of(layer)
            .ok_or(CoreError::LayerNotFound(layer))?;
        if let Some(p) = parent {
            if !self.is_group(p) {
                return Err(CoreError::NotAGroup(p));
            }
            if p == layer || self.is_ancestor(layer, p) {
                return Err(CoreError::CycleWouldForm {
                    child: layer,
                    parent: p,
                });
            }
        }
        let len = 1 + self.subtree_len(from);
        let mut moved: Vec<Layer> = self.layers.drain(from..from + len).collect();
        moved[0].parent_id = parent; // los internos del subárbol no cambian
        let at = self.insertion_index(parent, index); // ya SIN la capa movida
        self.layers.splice(at..at, moved);
        Ok(())
    }

    /// Inserta una capa ya construida como hijo número `index` de `parent`.
    pub fn insert_child(&mut self, mut layer: Layer, parent: Option<LayerId>, index: usize) {
        layer.parent_id = parent;
        let at = self.insertion_index(parent, index);
        self.layers.insert(at, layer);
    }

    /// Repara la invariante tras leer un sidecar ajeno o corrupto: descarta
    /// `parent_id` colgando o apuntando a una capa que no es grupo, rompe
    /// ciclos y reordena el `Vec` a preorden. Idempotente sobre un documento
    /// ya sano.
    pub fn normalize_tree(&mut self) {
        let ids: HashSet<LayerId> = self.layers.iter().map(|l| l.id).collect();
        for layer in &mut self.layers {
            if layer.parent_id.is_some_and(|p| !ids.contains(&p)) {
                layer.parent_id = None;
            }
        }
        let groups: HashSet<LayerId> = self
            .layers
            .iter()
            .filter(|l| matches!(l.content, LayerContent::Group(_)))
            .map(|l| l.id)
            .collect();
        for layer in &mut self.layers {
            if layer.parent_id.is_some_and(|p| !groups.contains(&p)) {
                layer.parent_id = None;
            }
        }
        // Rompe ciclos: si subir por `parent_id` no llega a una raíz en, como
        // mucho, tantos pasos como capas hay, se corta la capa en la raíz.
        let by_parent: HashMap<LayerId, Option<LayerId>> =
            self.layers.iter().map(|l| (l.id, l.parent_id)).collect();
        let limit = self.layers.len();
        let mut broken = HashSet::new();
        for layer in &self.layers {
            let mut current = layer.id;
            let mut steps = 0usize;
            while let Some(parent) = by_parent.get(&current).copied().flatten() {
                if steps > limit {
                    broken.insert(layer.id);
                    break;
                }
                current = parent;
                steps += 1;
            }
        }
        for layer in &mut self.layers {
            if broken.contains(&layer.id) {
                layer.parent_id = None;
            }
        }
        self.layers = preorder(std::mem::take(&mut self.layers));
    }
}

/// Reordena un conjunto de capas (con `parent_id` ya libre de ciclos y de
/// padres inexistentes o que no son grupo) a preorden del bosque.
fn preorder(layers: Vec<Layer>) -> Vec<Layer> {
    fn visit(remaining: &mut Vec<Layer>, parent: Option<LayerId>, out: &mut Vec<Layer>) {
        let mut i = 0;
        while i < remaining.len() {
            if remaining[i].parent_id == parent {
                let child = remaining.remove(i);
                let id = child.id;
                let is_group = matches!(child.content, LayerContent::Group(_));
                out.push(child);
                if is_group {
                    visit(remaining, Some(id), out);
                }
            } else {
                i += 1;
            }
        }
    }
    let mut remaining = layers;
    let mut out = Vec::with_capacity(remaining.len());
    visit(&mut remaining, None, &mut out);
    out
}
