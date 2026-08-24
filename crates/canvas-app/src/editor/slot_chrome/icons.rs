//! Los iconos de la cabecera están todos en `crate::app_icons` (la
//! iconografía unificada de la app); este módulo solo los re-exporta con
//! la visibilidad `pub(super)` que ya usaba `header.rs`, para no tocar
//! sus imports.

pub(super) use crate::app_icons::{
    draw_delete_icon, draw_duplicate_icon, draw_lock_icon, draw_triangle_icon, IconDir,
};
