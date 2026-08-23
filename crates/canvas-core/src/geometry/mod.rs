//! Geometria pura de alineacion, redimensionado y recorte. Sin UI: funciones
//! deterministas y testeables que la app usa para botones y manejadores.

mod align;
mod crop;
mod resize;

#[cfg(test)]
mod tests;

pub use align::{
    align_horizontal, align_vertical, contain_transform, cover_transform, HAlign, VAlign,
};
pub use crop::{trim_crop_from_corner, uncrop_transform};
pub use resize::{resize_around_center, resize_from_corner, resize_rotated_from_corner, Corner};
