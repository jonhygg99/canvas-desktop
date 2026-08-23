//! Comandos que cambian la geometria: la transformada de una capa, su
//! recorte, y el tamano de la pagina.

use crate::document::Document;
use crate::error::CoreError;
use crate::layer::{LayerId, Transform};

use super::Command;

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
