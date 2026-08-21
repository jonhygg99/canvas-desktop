//! Carga y guardado de imágenes: decode con orientación EXIF, escritura
//! atómica sobre el archivo original y miniaturas de galería.

mod clipboard;
mod export;
mod load;
mod metadata;
mod png_codec;
mod probe;
mod save;
mod sidecar;
mod svg;
mod thumbs;

pub use clipboard::{
    decode_layer_png, encode_layer_png, read_clipboard, write_clipboard, ClipboardDoc,
};
pub use export::{document_to_svg, svg_to_pdf, ExportFormat, ExportImages, TextLineBreaker};
pub use load::{
    can_overwrite, is_canvas_file, is_image_file, is_standalone_design, load_image,
    peek_numbered_path, peek_unique_path, reserve_numbered_path, reserve_unique_path, LoadedImage,
    CANVAS_EXTENSION, IMAGE_EXTENSIONS,
};
pub use metadata::{
    extract_metadata, extract_metadata_from_file, patch_orientation_to_1, reinject_metadata,
    ImageMetadata,
};
pub use probe::{probe_page_size, probe_page_size_with};
pub use save::{save_format_from_path, save_rgba, write_atomic};
pub use sidecar::{
    blank_design, delete_sidecar, ensure_sidecar_dir, find_sidecar, make_preview, preview_scale,
    read_design, read_preview, read_sidecar, sidecar_dir, sidecar_path, write_blank_canvas,
    write_design, write_sidecar, CanvasPayload, LayerPixels, RestoredDocument, PREVIEW_MAX_DIM,
    SIDECAR_DIR,
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
