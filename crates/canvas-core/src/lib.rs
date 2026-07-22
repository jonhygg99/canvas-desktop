//! Modelo del documento: páginas, capas, historial de comandos.
//!
//! Este crate no sabe nada de la UI ni del sistema operativo, y es testeable
//! sin abrir ninguna ventana.

mod align;
mod command;
mod document;
mod error;
mod layer;
mod snap;

pub use align::{
    align_horizontal, align_vertical, cover_transform, resize_from_corner,
    resize_rotated_from_corner, trim_crop_from_corner, uncrop_transform, Corner, HAlign, VAlign,
};
pub use command::{
    Command, Composite, History, InsertLayer, RemoveLayer, SetBlur, SetCrop, SetPageSize,
    SetShadow, SetTransform,
};
pub use document::{Document, Page};
pub use error::CoreError;
pub use layer::{CropRect, Effects, ImageContent, Layer, LayerContent, LayerId, Shadow, Transform};
pub use snap::{snap_translation, SnapResult};
