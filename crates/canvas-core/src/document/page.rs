//! La pagina: el struct `Page`, los accesos por id y el hit-testing. El
//! recorrido del arbol de grupos vive aparte, en `tree`.

use serde::{Deserialize, Serialize};

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

    /// ¿La capa en `id` es un grupo?
    pub fn is_group(&self, id: LayerId) -> bool {
        self.layer(id)
            .is_some_and(|l| matches!(l.content, LayerContent::Group(_)))
    }

    /// Posición de la capa entre sus hermanos (0 = el más bajo).
    pub fn sibling_index(&self, id: LayerId) -> Option<usize> {
        let parent = self.layer(id)?.parent_id;
        self.children_of(parent).iter().position(|&c| c == id)
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
}
