//! Deshacer/rehacer con el patron Command: el trait, una familia de
//! comandos por archivo, y la pila `History` que los conduce.

use std::fmt;

use crate::document::Document;
use crate::error::CoreError;

mod appearance;
mod history;
mod structure;
mod transform;

#[cfg(test)]
mod tests;

pub use appearance::{
    Rename, SetBlur, SetContent, SetEffects, SetLocked, SetOpacity, SetShadow, SetVisible,
};
pub use history::{Composite, History};
pub use structure::{Group, InsertLayer, RemoveLayer, Reorder, Ungroup};
pub use transform::{SetCrop, SetPageSize, SetTransform};

/// Un paso de edición reversible (patrón Command).
///
/// Los gestos continuos (arrastrar una capa, mover un slider) NO generan un
/// comando por frame: la UI muta el documento directamente durante el gesto y,
/// al soltarlo, empuja UN comando con el estado inicial y final mediante
/// [`History::push_applied`]. Así arrastrar una capa 200 píxeles es un único
/// paso de deshacer.
pub trait Command: fmt::Debug + Send {
    fn label(&self) -> &str;
    fn apply(&mut self, doc: &mut Document) -> Result<(), CoreError>;
    fn revert(&mut self, doc: &mut Document) -> Result<(), CoreError>;
}
