//! Carga y guardado de imágenes: decode con orientación EXIF, escritura
//! atómica sobre el archivo original y miniaturas de galería.

mod load;
mod metadata;
mod save;
mod sidecar;
mod svg;
mod thumbs;

pub use load::{can_overwrite, is_image_file, load_image, LoadedImage, IMAGE_EXTENSIONS};
pub use metadata::{
    extract_metadata, extract_metadata_from_file, patch_orientation_to_1, reinject_metadata,
    ImageMetadata,
};
pub use save::{save_format_from_path, save_rgba, write_atomic};
pub use sidecar::{
    delete_sidecar, read_sidecar, sidecar_path, write_sidecar, LayerPixels, RestoredDocument,
};
pub use svg::load_svg;
pub use thumbs::thumbnail;

use std::path::PathBuf;

use thiserror::Error;

// Los mensajes van en inglés porque acaban en la UI (la spec fija la UI en inglés).
#[derive(Debug, Error)]
pub enum IoError {
    #[error("Could not open \"{path}\": {source}")]
    Open {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("Could not decode \"{path}\": {source}")]
    Decode {
        path: PathBuf,
        source: image::ImageError,
    },
    #[error(
        "\"{path}\" is not a format Canvas Desktop can save to (use png, jpg, webp, gif or bmp)"
    )]
    UnsupportedFormat { path: PathBuf },
    #[error("Could not encode \"{path}\": {message}")]
    Encode { path: PathBuf, message: String },
    #[error("Could not save \"{path}\": {message}. The document is still intact in memory; try \"Save as…\"")]
    Write { path: PathBuf, message: String },
}
