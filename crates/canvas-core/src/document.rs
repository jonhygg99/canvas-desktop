use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::CoreError;
use crate::layer::{Layer, LayerContent, LayerId, Transform};

/// Una página del documento: un lienzo con su pila de capas, de abajo arriba.
///
/// **Invariante de preorden.** `layers` es un recorrido en preorden del
/// bosque que forman los grupos: los descendientes de una capa de grupo
/// ocupan el tramo contiguo `[i+1, i+subtree_len(i)]` justo por encima de
/// ella (entre hermanos, índice menor = más abajo). La cabecera del grupo va
/// en el índice más bajo de su tramo porque el renderizador recorre el `Vec`
/// hacia delante y necesita abrir la capa de grupo antes que sus hijos. Todas
/// las mutaciones del árbol deben pasar por `move_subtree`/`insert_child`
/// para no romperla.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Page {
    pub width: f64,
    pub height: f64,
    /// Color de fondo RGBA; `None` = transparente.
    pub background: Option<[u8; 4]>,
    /// De abajo (índice 0) hacia arriba.
    pub layers: Vec<Layer>,
}

impl Page {
    pub fn new(width: f64, height: f64) -> Self {
        Self {
            width,
            height,
            background: None,
            layers: Vec::new(),
        }
    }

    pub fn layer(&self, id: LayerId) -> Option<&Layer> {
        self.layers.iter().find(|l| l.id == id)
    }

    pub fn layer_mut(&mut self, id: LayerId) -> Option<&mut Layer> {
        self.layers.iter_mut().find(|l| l.id == id)
    }

    /// Capa visible más alta bajo el punto dado (coordenadas de página),
    /// teniendo en cuenta la rotación de cada capa. Los grupos nunca se
    /// seleccionan por clic (se gestionan desde el panel de capas) y la
    /// visibilidad/bloqueo se comprueban en toda la cadena de ancestros.
    pub fn layer_at(&self, x: f64, y: f64) -> Option<LayerId> {
        self.layers
            .iter()
            .rev()
            .filter(|l| !matches!(l.content, LayerContent::Group(_)))
            .find(|l| {
                self.effective_visible(l.id)
                    && !self.effective_locked(l.id)
                    && l.transform.contains_point(x, y)
            })
            .map(|l| l.id)
    }

    /// Posición absoluta de una capa en el `Vec`, si existe.
    pub fn index_of(&self, id: LayerId) -> Option<usize> {
        self.layers.iter().position(|l| l.id == id)
    }

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

    /// ¿La capa en `id` es un grupo?
    pub fn is_group(&self, id: LayerId) -> bool {
        self.layer(id)
            .is_some_and(|l| matches!(l.content, LayerContent::Group(_)))
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

    /// Posición de la capa entre sus hermanos (0 = el más bajo).
    pub fn sibling_index(&self, id: LayerId) -> Option<usize> {
        let parent = self.layer(id)?.parent_id;
        self.children_of(parent).iter().position(|&c| c == id)
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

    /// Recalcula la caja envolvente de un grupo a partir de sus hijos
    /// DIRECTOS (que si son grupos ya traen su propia caja recalculada). Solo
    /// alimenta el recuadro de selección del panel; no afecta al renderizado.
    pub fn refresh_group_bounds(&mut self, group: LayerId) {
        let children = self.children_of(Some(group));
        if children.is_empty() {
            if let Some(layer) = self.layer_mut(group) {
                layer.transform = Transform::new(0.0, 0.0, 0.0, 0.0);
            }
            return;
        }
        let (mut min_x, mut min_y) = (f64::INFINITY, f64::INFINITY);
        let (mut max_x, mut max_y) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
        for child in children {
            let Some(layer) = self.layer(child) else {
                continue;
            };
            for (x, y) in layer.transform.corners() {
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
        if let Some(layer) = self.layer_mut(group) {
            layer.transform = Transform::new(
                min_x,
                min_y,
                (max_x - min_x).max(0.0),
                (max_y - min_y).max(0.0),
            );
        }
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Document {
    pub pages: Vec<Page>,
    /// Archivo de origen en disco (la imagen que se abrió), si lo hay.
    pub source_path: Option<PathBuf>,
    next_layer_id: u64,
}

impl Document {
    /// Documento de una página con las dimensiones dadas.
    pub fn new(page_width: f64, page_height: f64) -> Self {
        Self {
            pages: vec![Page::new(page_width, page_height)],
            source_path: None,
            next_layer_id: 1,
        }
    }

    /// La página activa (esta entrega trabaja con una sola).
    pub fn page(&self) -> Result<&Page, CoreError> {
        self.pages.first().ok_or(CoreError::NoPages)
    }

    pub fn page_mut(&mut self) -> Result<&mut Page, CoreError> {
        self.pages.first_mut().ok_or(CoreError::NoPages)
    }

    /// Reserva un id de capa único (para construir capas que luego se
    /// insertan mediante comandos deshacibles).
    pub fn allocate_layer_id(&mut self) -> LayerId {
        let id = LayerId::new(self.next_layer_id);
        self.next_layer_id += 1;
        id
    }

    /// Añade una capa encima de las existentes en la página activa y devuelve
    /// su id.
    pub fn add_layer(
        &mut self,
        name: impl Into<String>,
        transform: Transform,
        content: LayerContent,
    ) -> Result<LayerId, CoreError> {
        let id = self.allocate_layer_id();
        let layer = Layer::new(id, name, transform, content);
        self.page_mut()?.layers.push(layer);
        Ok(id)
    }

    pub fn layer(&self, id: LayerId) -> Result<&Layer, CoreError> {
        self.pages
            .iter()
            .find_map(|p| p.layer(id))
            .ok_or(CoreError::LayerNotFound(id))
    }

    pub fn layer_mut(&mut self, id: LayerId) -> Result<&mut Layer, CoreError> {
        self.pages
            .iter_mut()
            .find_map(|p| p.layer_mut(id))
            .ok_or(CoreError::LayerNotFound(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer::ImageContent;

    fn image_content() -> LayerContent {
        LayerContent::Image(ImageContent {
            source_path: None,
            natural_width: 100,
            natural_height: 50,
            crop: None,
        })
    }

    /// Documento con un grupo que contiene dos capas de imagen: `a` (hermano
    /// 0, más abajo) y `b` (hermano 1, más arriba).
    fn doc_with_group() -> (Document, LayerId, LayerId, LayerId) {
        let mut doc = Document::new(800.0, 600.0);
        let a = doc
            .add_layer("a", Transform::new(0.0, 0.0, 10.0, 10.0), image_content())
            .unwrap();
        let b = doc
            .add_layer("b", Transform::new(20.0, 20.0, 10.0, 10.0), image_content())
            .unwrap();
        let group_id = doc.allocate_layer_id();
        let page = doc.page_mut().unwrap();
        let top = page.children_of(None).len();
        page.insert_child(Layer::group(group_id, "Group"), None, top);
        page.move_subtree(a, Some(group_id), 0).unwrap();
        page.move_subtree(b, Some(group_id), 1).unwrap();
        (doc, group_id, a, b)
    }

    #[test]
    fn add_layer_assigns_unique_ids() {
        let mut doc = Document::new(800.0, 600.0);
        let a = doc
            .add_layer("a", Transform::new(0.0, 0.0, 10.0, 10.0), image_content())
            .unwrap();
        let b = doc
            .add_layer("b", Transform::new(0.0, 0.0, 10.0, 10.0), image_content())
            .unwrap();
        assert_ne!(a, b);
        assert_eq!(doc.page().unwrap().layers.len(), 2);
        assert_eq!(doc.layer(a).unwrap().name, "a");
        assert_eq!(doc.layer(b).unwrap().name, "b");
    }

    #[test]
    fn layer_at_returns_topmost_visible_hit() {
        let mut doc = Document::new(800.0, 600.0);
        let bottom = doc
            .add_layer(
                "bottom",
                Transform::new(0.0, 0.0, 100.0, 100.0),
                image_content(),
            )
            .unwrap();
        let top = doc
            .add_layer(
                "top",
                Transform::new(50.0, 50.0, 100.0, 100.0),
                image_content(),
            )
            .unwrap();

        let page = doc.page().unwrap();
        // Zona solapada: gana la de arriba.
        assert_eq!(page.layer_at(75.0, 75.0), Some(top));
        // Zona solo de la de abajo.
        assert_eq!(page.layer_at(10.0, 10.0), Some(bottom));
        // Vacío.
        assert_eq!(page.layer_at(500.0, 500.0), None);
    }

    #[test]
    fn layer_at_skips_hidden_layers() {
        let mut doc = Document::new(800.0, 600.0);
        let id = doc
            .add_layer("a", Transform::new(0.0, 0.0, 100.0, 100.0), image_content())
            .unwrap();
        doc.layer_mut(id).unwrap().visible = false;
        assert_eq!(doc.page().unwrap().layer_at(50.0, 50.0), None);
    }

    #[test]
    fn layer_lookup_fails_for_unknown_id() {
        let mut doc = Document::new(800.0, 600.0);
        let id = doc
            .add_layer("a", Transform::new(0.0, 0.0, 10.0, 10.0), image_content())
            .unwrap();
        let missing = LayerId::new(id.raw() + 99);
        assert_eq!(doc.layer(missing), Err(CoreError::LayerNotFound(missing)));
    }

    #[test]
    fn children_of_lists_direct_children_bottom_up() {
        let (doc, group, a, b) = doc_with_group();
        let page = doc.page().unwrap();
        assert_eq!(page.children_of(Some(group)), vec![a, b]);
        assert_eq!(page.children_of(None), vec![group]);
    }

    #[test]
    fn descendants_walks_arbitrary_depth() {
        let (mut doc, group, a, b) = doc_with_group();
        let inner = doc.allocate_layer_id();
        {
            let page = doc.page_mut().unwrap();
            page.insert_child(Layer::group(inner, "Inner"), Some(group), 0);
            page.move_subtree(a, Some(inner), 0).unwrap();
        }
        let page = doc.page().unwrap();
        let descendants = page.descendants(group);
        assert_eq!(descendants.len(), 3, "grupo interno + a + b");
        assert!(descendants.contains(&inner));
        assert!(descendants.contains(&a));
        assert!(descendants.contains(&b));
    }

    #[test]
    fn is_ancestor_detects_the_whole_chain() {
        let (mut doc, group, a, _b) = doc_with_group();
        let inner = doc.allocate_layer_id();
        {
            let page = doc.page_mut().unwrap();
            page.insert_child(Layer::group(inner, "Inner"), Some(group), 0);
            page.move_subtree(a, Some(inner), 0).unwrap();
        }
        let page = doc.page().unwrap();
        assert!(page.is_ancestor(group, a));
        assert!(page.is_ancestor(inner, a));
        assert!(!page.is_ancestor(a, group));
    }

    #[test]
    fn effective_visible_inherits_from_ancestors() {
        let (mut doc, group, a, b) = doc_with_group();
        assert!(doc.page().unwrap().effective_visible(a));
        doc.layer_mut(group).unwrap().visible = false;
        let page = doc.page().unwrap();
        assert!(!page.effective_visible(a));
        assert!(!page.effective_visible(b));
    }

    #[test]
    fn effective_locked_inherits_from_ancestors() {
        let (mut doc, group, a, b) = doc_with_group();
        assert!(!doc.page().unwrap().effective_locked(a));
        doc.layer_mut(group).unwrap().locked = true;
        let page = doc.page().unwrap();
        assert!(page.effective_locked(a));
        assert!(page.effective_locked(b));
    }

    #[test]
    fn effective_opacity_multiplies_the_chain() {
        let (mut doc, group, a, _b) = doc_with_group();
        doc.layer_mut(group).unwrap().opacity = 0.5;
        doc.layer_mut(a).unwrap().opacity = 0.5;
        let page = doc.page().unwrap();
        assert!((page.effective_opacity(a) - 0.25).abs() < 1e-6);
    }

    #[test]
    fn layer_at_skips_groups_and_layers_hidden_by_their_group() {
        let (mut doc, group, a, _b) = doc_with_group();
        // El grupo no tiene geometría propia: un punto dentro de "a" (que
        // ocupa 0,0..10,10) debe seleccionar la hoja, no el grupo.
        assert_eq!(doc.page().unwrap().layer_at(5.0, 5.0), Some(a));
        doc.layer_mut(group).unwrap().visible = false;
        assert_eq!(doc.page().unwrap().layer_at(5.0, 5.0), None);
    }

    #[test]
    fn move_subtree_keeps_children_next_to_their_group() {
        let (doc, group, a, b) = doc_with_group();
        let page = doc.page().unwrap();
        let gi = page.index_of(group).unwrap();
        assert_eq!(page.subtree_len(gi), 2);
        let ai = page.index_of(a).unwrap();
        let bi = page.index_of(b).unwrap();
        assert!(ai > gi && ai <= gi + 2);
        assert!(bi > gi && bi <= gi + 2);
    }

    #[test]
    fn move_subtree_rejects_dropping_a_group_into_itself() {
        let (mut doc, group, _a, _b) = doc_with_group();
        let page = doc.page_mut().unwrap();
        assert_eq!(
            page.move_subtree(group, Some(group), 0),
            Err(CoreError::CycleWouldForm {
                child: group,
                parent: group
            })
        );
    }

    #[test]
    fn move_subtree_rejects_a_parent_that_is_not_a_group() {
        let (mut doc, _group, a, b) = doc_with_group();
        let page = doc.page_mut().unwrap();
        assert_eq!(
            page.move_subtree(a, Some(b), 0),
            Err(CoreError::NotAGroup(b))
        );
    }

    #[test]
    fn normalize_tree_breaks_cycles_and_drops_orphan_parents() {
        let mut doc = Document::new(800.0, 600.0);
        let a = doc
            .add_layer("a", Transform::new(0.0, 0.0, 10.0, 10.0), image_content())
            .unwrap();
        let b = doc
            .add_layer("b", Transform::new(0.0, 0.0, 10.0, 10.0), image_content())
            .unwrap();
        {
            let page = doc.page_mut().unwrap();
            // "a" apunta a un grupo que no existe; "b" apunta a "a", que no
            // es un grupo. Ambos deben quedar como raíz tras normalizar.
            page.layer_mut(a).unwrap().parent_id = Some(LayerId::from_raw(9999));
            page.layer_mut(b).unwrap().parent_id = Some(a);
        }
        doc.page_mut().unwrap().normalize_tree();
        let page = doc.page().unwrap();
        assert_eq!(page.layer(a).unwrap().parent_id, None);
        assert_eq!(page.layer(b).unwrap().parent_id, None);
    }

    #[test]
    fn depth_is_bounded_with_a_corrupt_parent_chain() {
        let mut doc = Document::new(800.0, 600.0);
        let a = doc
            .add_layer("a", Transform::new(0.0, 0.0, 10.0, 10.0), image_content())
            .unwrap();
        doc.layer_mut(a).unwrap().parent_id = Some(a); // ciclo directo a sí misma
        let page = doc.page().unwrap();
        // No debe colgarse: acotado a layers.len() (== 1) pasos.
        assert_eq!(page.depth(a), 1);
    }
}
