//! Carga y guardado de imágenes: decode con orientación EXIF, escritura
//! atómica sobre el archivo original y miniaturas de galería.

mod load;
mod save;
mod sidecar;
mod thumbs;

pub use load::{is_image_file, load_image, LoadedImage, IMAGE_EXTENSIONS};
pub use save::{save_format_from_path, save_rgba, write_atomic};
pub use sidecar::{
    delete_sidecar, read_sidecar, sidecar_path, write_sidecar, LayerPixels, RestoredDocument,
};
pub use thumbs::thumbnail;

use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum IoError {
    #[error("no se pudo abrir «{path}»: {source}")]
    Open {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("no se pudo decodificar «{path}»: {source}")]
    Decode {
        path: PathBuf,
        source: image::ImageError,
    },
    #[error("«{path}» no es un formato en el que sepamos guardar (usa png, jpg, webp, gif o bmp)")]
    UnsupportedFormat { path: PathBuf },
    #[error("no se pudo codificar «{path}»: {message}")]
    Encode { path: PathBuf, message: String },
    #[error("no se pudo guardar «{path}»: {message}. El documento sigue intacto en memoria; prueba con «Guardar como…»")]
    Write { path: PathBuf, message: String },
}
