use crate::document::Page;
use crate::layer::LayerId;

/// Capas seleccionadas. La PRIMERA (`ids[0]`) es la primaria: la que mandan
/// el panel de propiedades, los manejadores del lienzo y los gestos. El
/// resto solo importa para operaciones en bloque (agrupar, borrar, copiar,
/// Select All), que pasan por `in_stack_order`/`roots`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Selection {
    ids: Vec<LayerId>,
}

impl Selection {
    /// Selección de una sola capa.
    pub fn single(id: LayerId) -> Self {
        Self { ids: vec![id] }
    }

    /// La capa primaria (la que gobierna panel/gestos), si hay alguna.
    pub fn primary(&self) -> Option<LayerId> {
        self.ids.first().copied()
    }

    /// Todas las capas seleccionadas, sin ningún orden garantizado (para eso,
    /// `in_stack_order`).
    pub fn ids(&self) -> &[LayerId] {
        &self.ids
    }

    pub fn len(&self) -> usize {
        self.ids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    pub fn contains(&self, id: LayerId) -> bool {
        self.ids.contains(&id)
    }

    pub fn clear(&mut self) {
        self.ids.clear();
    }

    /// Clic normal: reemplaza la selección entera. `None` deselecciona todo.
    pub fn set(&mut self, id: Option<LayerId>) {
        self.ids.clear();
        if let Some(id) = id {
            self.ids.push(id);
        }
    }

    /// Ctrl+clic: si ya estaba, la quita; si no, la añade como nueva
    /// primaria (al frente).
    pub fn toggle(&mut self, id: LayerId) {
        if let Some(pos) = self.ids.iter().position(|&x| x == id) {
            self.ids.remove(pos);
        } else {
            self.ids.insert(0, id);
        }
    }

    /// Shift+clic: selecciona el tramo de la pila (orden de `page.layers`,
    /// de abajo arriba) entre la primaria actual y `id`, dejando `id` como
    /// nueva primaria. Sin primaria previa, o si alguna de las dos no existe
    /// en `page`, se comporta como un clic normal sobre `id`.
    pub fn extend_range(&mut self, page: &Page, id: LayerId) {
        let Some(a) = self.primary().and_then(|p| page.index_of(p)) else {
            self.set(Some(id));
            return;
        };
        let Some(b) = page.index_of(id) else {
            self.set(Some(id));
            return;
        };
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        let mut ids = vec![id];
        for layer in &page.layers[lo..=hi] {
            if layer.id != id {
                ids.push(layer.id);
            }
        }
        self.ids = ids;
    }

    /// Olvida los ids que ya no existen en `page` (tras deshacer/rehacer un
    /// borrado, por ejemplo). Se llama después de cualquier operación que
    /// pueda haber quitado capas.
    pub fn retain_existing(&mut self, page: &Page) {
        self.ids.retain(|&id| page.layer(id).is_some());
    }

    /// La selección ordenada por posición en la pila (de abajo arriba). Los
    /// ids que ya no existen en `page` se ignoran.
    pub fn in_stack_order(&self, page: &Page) -> Vec<LayerId> {
        let mut ids: Vec<LayerId> = self
            .ids
            .iter()
            .copied()
            .filter(|&id| page.index_of(id).is_some())
            .collect();
        ids.sort_by_key(|&id| page.index_of(id).unwrap_or(usize::MAX));
        ids
    }

    /// La selección sin los ids cuyo ancestro TAMBIÉN está seleccionado:
    /// agrupar o copiar un grupo ya arrastra a sus hijos, así que no hace
    /// falta procesarlos por separado.
    pub fn roots(&self, page: &Page) -> Vec<LayerId> {
        self.ids
            .iter()
            .copied()
            .filter(|&id| {
                !self
                    .ids
                    .iter()
                    .any(|&other| other != id && page.is_ancestor(other, id))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;
    use crate::layer::{ImageContent, Layer, LayerContent, Transform};

    fn image_content() -> LayerContent {
        LayerContent::Image(ImageContent {
            source_path: None,
            natural_width: 10,
            natural_height: 10,
            crop: None,
        })
    }

    /// Documento con tres capas raíz `a, b, c` (de abajo arriba).
    fn doc_with_three_layers() -> (Document, LayerId, LayerId, LayerId) {
        let mut doc = Document::new(800.0, 600.0);
        let a = doc
            .add_layer("a", Transform::new(0.0, 0.0, 10.0, 10.0), image_content())
            .unwrap();
        let b = doc
            .add_layer("b", Transform::new(0.0, 0.0, 10.0, 10.0), image_content())
            .unwrap();
        let c = doc
            .add_layer("c", Transform::new(0.0, 0.0, 10.0, 10.0), image_content())
            .unwrap();
        (doc, a, b, c)
    }

    #[test]
    fn toggle_adds_and_removes_keeping_the_last_as_primary() {
        let mut sel = Selection::single(LayerId::from_raw(1));
        let two = LayerId::from_raw(2);
        sel.toggle(two);
        assert_eq!(sel.primary(), Some(two), "la recién tocada es la primaria");
        assert_eq!(sel.len(), 2);

        sel.toggle(two);
        assert_eq!(sel.len(), 1);
        assert!(!sel.contains(two));
        assert_eq!(sel.primary(), Some(LayerId::from_raw(1)));
    }

    #[test]
    fn extend_range_selects_the_span_between_primary_and_target() {
        let (doc, a, b, c) = doc_with_three_layers();
        let page = doc.page().unwrap();
        let mut sel = Selection::single(a);
        sel.extend_range(page, c);
        assert_eq!(sel.primary(), Some(c));
        assert_eq!(sel.len(), 3);
        assert!(sel.contains(a) && sel.contains(b) && sel.contains(c));
    }

    #[test]
    fn roots_drops_layers_whose_group_is_also_selected() {
        let (mut doc, a, b, _c) = doc_with_three_layers();
        let group_id = doc.allocate_layer_id();
        {
            let page = doc.page_mut().unwrap();
            page.insert_child(Layer::group(group_id, "Group"), None, 0);
            page.move_subtree(a, Some(group_id), 0).unwrap();
        }
        let page = doc.page().unwrap();
        let mut sel = Selection::single(group_id);
        sel.toggle(a);
        sel.toggle(b);
        let roots = sel.roots(page);
        assert!(roots.contains(&group_id));
        assert!(roots.contains(&b));
        assert!(
            !roots.contains(&a),
            "a ya viaja dentro de group_id, no hace falta procesarla aparte"
        );
    }

    #[test]
    fn retain_existing_forgets_deleted_layers() {
        let (mut doc, a, b, _c) = doc_with_three_layers();
        let mut sel = Selection::single(a);
        sel.toggle(b);
        doc.page_mut().unwrap().layers.retain(|l| l.id != a);
        sel.retain_existing(doc.page().unwrap());
        assert_eq!(sel.ids(), &[b]);
    }

    #[test]
    fn in_stack_order_sorts_bottom_to_top() {
        let (doc, a, b, c) = doc_with_three_layers();
        let page = doc.page().unwrap();
        let mut sel = Selection::single(c);
        sel.toggle(a);
        sel.toggle(b);
        // El orden de inserción no es el de pila: toggle deja a `b` como
        // primaria (la última tocada), pero `in_stack_order` reordena.
        assert_eq!(sel.in_stack_order(page), vec![a, b, c]);
    }
}
