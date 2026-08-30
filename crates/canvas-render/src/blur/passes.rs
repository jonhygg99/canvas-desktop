//! Fachada de compatibilidad para la cadena de pasadas GPU por capa.
//!
//! La implementación vive en `sync.rs`; este módulo conserva el nombre
//! público histórico de `passes::SyncLayerRequest`.

pub use super::sync::SyncLayerRequest;
