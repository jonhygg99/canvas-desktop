//! Modelo del documento: páginas, capas, historial de comandos.
//!
//! Este crate no sabe nada de la UI ni del sistema operativo, y es testeable
//! sin abrir ninguna ventana.

mod command;
mod document;
mod error;
mod geometry;
mod layer;
mod rounded_path;
mod selection;
mod shape_geom;
mod snap;

pub use command::{
    Command, Composite, Group, History, InsertLayer, RemoveLayer, Rename, Reorder, SetBlur,
    SetContent, SetCrop, SetEffects, SetLocked, SetOpacity, SetPageSize, SetShadow, SetTransform,
    SetVisible, Ungroup,
};
pub use document::{Document, Page};
pub use error::CoreError;
pub use geometry::{
    align_horizontal, align_vertical, contain_transform, cover_transform, resize_around_center,
    resize_from_corner, resize_rotated_from_corner, trim_crop_from_corner, uncrop_transform,
    Corner, HAlign, VAlign,
};
pub use layer::{
    CropRect, Effects, GroupContent, ImageContent, Layer, LayerContent, LayerId, Shadow,
    ShapeContent, ShapeKind, SvgContent, TextAlign, TextContent, TextLine, Transform,
};
pub use rounded_path::{rounded_polygon_path, RoundedPath};
pub use selection::Selection;
pub use shape_geom::{
    arrow_head_points, arrow_head_rounded, arrow_shaft_end_x, cross_points, diamond_points,
    heart_points, regular_polygon_points, star_points, triangle_points,
};
pub use snap::{snap_translation, SnapResult};
