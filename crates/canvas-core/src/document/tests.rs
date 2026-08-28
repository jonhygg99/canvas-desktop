//! Tests de `document`: sobre todo el invariante de preorden y la herencia
//! de propiedades por el arbol de grupos.

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

// ---- Test de propiedad del invariante de preorden ----
// Aleatoriedad reproducible con un xorshift64* propio: basta para un test
// de propiedad y evita añadir `proptest` (y su árbol de deps) al workspace.
struct XorShift(u64);

impl XorShift {
    fn new(seed: u64) -> Self {
        Self(seed.max(1)) // 0 es un punto fijo del xorshift
    }
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }
}

/// Orden preorden del bosque implícito en `parent_id`: raíces en orden de
/// Vec, hijos en orden de Vec. Solo válido si `parent_id` es un bosque sin
/// ciclos — garantizado cuando las mutaciones pasan por la API de tree.rs.
fn expected_preorder(page: &Page) -> Vec<LayerId> {
    fn visit(page: &Page, parent: Option<LayerId>, out: &mut Vec<LayerId>) {
        for layer in page.layers.iter().filter(|l| l.parent_id == parent) {
            out.push(layer.id);
            if page.is_group(layer.id) {
                visit(page, Some(layer.id), out);
            }
        }
    }
    let mut out = Vec::with_capacity(page.layers.len());
    visit(page, None, &mut out);
    out
}

/// La lista plana está en preorden Y cada grupo cubre en su tramo contiguo
/// exactamente a sus descendientes según la cadena de padres (calculada con
/// `is_ancestor`, que no depende del orden del Vec).
fn assert_preorder_invariant(page: &Page) {
    let actual: Vec<LayerId> = page.layers.iter().map(|l| l.id).collect();
    assert_eq!(
        actual,
        expected_preorder(page),
        "la lista plana debe estar en preorden"
    );
    for i in 0..page.layers.len() {
        let id = page.layers[i].id;
        if !page.is_group(id) {
            continue;
        }
        let span: Vec<LayerId> = page.layers[i + 1..i + 1 + page.subtree_len(i)]
            .iter()
            .map(|l| l.id)
            .collect();
        for other in &page.layers {
            let inside = page.is_ancestor(id, other.id);
            assert_eq!(
                span.contains(&other.id),
                inside,
                "el tramo contiguo de {id:?} no coincide con sus descendientes"
            );
        }
    }
}

#[test]
fn random_moves_and_inserts_preserve_the_preorder_invariant() {
    const OPS: usize = 300;
    let mut rng = XorShift::new(0xC0FF_EE01);
    let mut doc = Document::new(800.0, 600.0);
    for i in 0..6 {
        doc.add_layer(
            format!("l{i}"),
            Transform::new(0.0, 0.0, 10.0, 10.0),
            image_content(),
        )
        .unwrap();
    }
    for i in 0..3 {
        let g = doc.allocate_layer_id();
        let page = doc.page_mut().unwrap();
        let at = rng.below(page.layers.len() + 1);
        page.insert_child(Layer::group(g, format!("g{i}")), None, at);
    }

    for _ in 0..OPS {
        match rng.below(3) {
            // Mover un subárbol cualquiera a un sitio cualquiera. Los Err
            // son rechazos legítimos (ciclo, padre que no es grupo).
            0 => {
                let (id, parent, index) = random_target(&mut rng, &doc);
                doc.page_mut().unwrap().move_subtree(id, parent, index).ok();
            }
            // Insertar un grupo nuevo en un sitio aleatorio.
            1 => {
                let (parent, index) = random_target_parent(&mut rng, &doc);
                let g = doc.allocate_layer_id();
                doc.page_mut()
                    .unwrap()
                    .insert_child(Layer::group(g, "dyn"), parent, index);
            }
            // Añadir una hoja al tope y moverla de inmediato.
            _ => {
                let leaf = doc
                    .add_layer("dyn", Transform::new(0.0, 0.0, 5.0, 5.0), image_content())
                    .unwrap();
                let (parent, index) = random_target_parent(&mut rng, &doc);
                doc.page_mut()
                    .unwrap()
                    .move_subtree(leaf, parent, index)
                    .ok();
            }
        }
        assert_preorder_invariant(doc.page().unwrap());
    }

    // `normalize_tree` es idempotente sobre un árbol ya sano.
    let before: Vec<(LayerId, Option<LayerId>)> = doc
        .page()
        .unwrap()
        .layers
        .iter()
        .map(|l| (l.id, l.parent_id))
        .collect();
    doc.page_mut().unwrap().normalize_tree();
    let after: Vec<(LayerId, Option<LayerId>)> = doc
        .page()
        .unwrap()
        .layers
        .iter()
        .map(|l| (l.id, l.parent_id))
        .collect();
    assert_eq!(before, after, "normalizar un árbol sano no debe cambiarlo");
}

fn random_target_parent(rng: &mut XorShift, doc: &Document) -> (Option<LayerId>, usize) {
    let page = doc.page().unwrap();
    let groups: Vec<LayerId> = page
        .layers
        .iter()
        .filter(|l| page.is_group(l.id))
        .map(|l| l.id)
        .collect();
    let parent = if rng.below(2) == 0 || groups.is_empty() {
        None
    } else {
        Some(groups[rng.below(groups.len())])
    };
    let index = rng.below(page.children_of(parent).len() + 1);
    (parent, index)
}

fn random_target(rng: &mut XorShift, doc: &Document) -> (LayerId, Option<LayerId>, usize) {
    let page = doc.page().unwrap();
    let id = page.layers[rng.below(page.layers.len())].id;
    let (parent, index) = random_target_parent(rng, doc);
    (id, parent, index)
}

#[test]
fn normalize_tree_repairs_a_scrambled_flat_list() {
    let (mut doc, group, a, b) = doc_with_group();
    // Rompe el orden a mano (el hijo pasa a preceder a su grupo, el orden
    // entre hermanos se invierte) y deja que `normalize_tree` lo repare.
    doc.page_mut().unwrap().layers.reverse();
    doc.page_mut().unwrap().normalize_tree();
    assert_preorder_invariant(doc.page().unwrap());
    // La jerarquía se conserva; el orden relativo entre hermanos sigue el
    // orden de encuentro tras el barajado (b, a).
    let page = doc.page().unwrap();
    assert_eq!(page.children_of(Some(group)), vec![b, a]);
    assert_eq!(page.children_of(None), vec![group]);
}
