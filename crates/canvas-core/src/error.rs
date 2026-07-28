use crate::layer::LayerId;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CoreError {
    #[error("la capa {0:?} no existe en el documento")]
    LayerNotFound(LayerId),
    #[error("el documento no tiene ninguna página")]
    NoPages,
    #[error("la capa {0:?} no es un grupo")]
    NotAGroup(LayerId),
    #[error("mover {child:?} dentro de {parent:?} crearía un ciclo")]
    CycleWouldForm { child: LayerId, parent: LayerId },
}
