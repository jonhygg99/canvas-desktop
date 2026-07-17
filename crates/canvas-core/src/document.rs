use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::CoreError;
use crate::layer::{Layer, LayerContent, LayerId, Transform};

/// Una página del documento: un lienzo con su pila de capas, de abajo arriba.
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

    /// Capa visible más alta bajo el punto dado (coordenadas de página).
    /// La rotación se ignora por ahora (las capas de esta entrega no rotan).
    pub fn layer_at(&self, x: f64, y: f64) -> Option<LayerId> {
        self.layers.iter().rev().find_map(|l| {
            let t = &l.transform;
            let hit =
                l.visible && x >= t.x && x <= t.x + t.width && y >= t.y && y <= t.y + t.height;
            hit.then_some(l.id)
        })
    }
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
        })
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
}
