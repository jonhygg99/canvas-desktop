//! Modelo del documento: páginas, capas, historial de comandos.
//!
//! Este crate no sabe nada de la UI ni del sistema operativo, y es testeable
//! sin abrir ninguna ventana.

mod align;
mod command;
mod document;
mod error;
mod layer;

pub use align::{
    align_horizontal, align_vertical, cover_transform, resize_from_corner, Corner, HAlign, VAlign,
};
pub use command::{
    Command, Composite, History, InsertLayer, RemoveLayer, SetBlur, SetPageSize, SetShadow,
    SetTransform,
};
pub use document::{Document, Page};
pub use error::CoreError;
pub use layer::{Effects, ImageContent, Layer, LayerContent, LayerId, Shadow, Transform};
