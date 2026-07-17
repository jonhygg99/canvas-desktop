use crate::layer::LayerId;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CoreError {
    #[error("la capa {0:?} no existe en el documento")]
    LayerNotFound(LayerId),
    #[error("el documento no tiene ninguna página")]
    NoPages,
}
