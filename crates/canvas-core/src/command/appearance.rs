//! Comandos que cambian el aspecto de UNA capa sin tocar el arbol ni su
//! geometria: efectos, opacidad, visibilidad, bloqueo, nombre y contenido.

use crate::document::Document;
use crate::error::CoreError;
use crate::layer::{LayerId, Shadow};

use super::Command;

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
