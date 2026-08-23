//! El documento y sus paginas.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::CoreError;
use crate::layer::{Layer, LayerContent, LayerId, Transform};

mod page;
mod tree;

#[cfg(test)]
mod tests;

pub use page::Page;

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
