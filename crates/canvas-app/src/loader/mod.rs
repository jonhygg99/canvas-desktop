//! Trabajo en hilos aparte: carga de imágenes y diálogos nativos. La UI nunca
//! bloquea en disco; los resultados llegan por canal.
//!
//! Dividido en submódulos por dominio: `load_ops` (abrir imágenes/diseños/
//! ranuras de la baraja, y los diálogos de abrir archivo/carpeta),
//! `save_ops` (guardar), `export_ops` (exportar), `gallery_ops`
//! (operaciones de archivos de la galería), `image_import` (añadir/
//! reemplazar una capa de imagen, incluida la descarga por URL) y
//! `unsplash_ops` (búsqueda de imágenes de Unsplash).

mod export_ops;
mod file_ops;
mod gallery_ops;
mod image_import;
mod load_ops;
mod save_ops;
mod unsplash_ops;

use std::path::PathBuf;

use canvas_core::LayerId;
use canvas_io::{ImageMetadata, IoError, LoadedImage, RestoredDocument};

pub use export_ops::{spawn_export_raster, spawn_export_vector, spawn_pick_export_path};
pub use gallery_ops::{
    spawn_document_delete, spawn_document_rename, spawn_folders_auto_refresh, spawn_gallery_op,
    spawn_gallery_scan, spawn_restore_from_trash, spawn_single_thumb,
};
pub use image_import::{
    spawn_load_image_as_layer, spawn_load_replacement_image_from_url, spawn_pick_replacement_image,
};
pub use load_ops::{
    spawn_deck_probe, spawn_load_design, spawn_load_image, spawn_load_slot, spawn_pick_file,
    spawn_pick_folder,
};
pub use save_ops::{
    spawn_pick_design_path, spawn_pick_save_path, spawn_reserve_canvas_path, spawn_save,
    spawn_save_design, SaveInput,
};
pub use unsplash_ops::{spawn_unsplash_image, spawn_unsplash_search, spawn_unsplash_thumb};

/// Resultado de abrir una imagen: mapa de bits plano, o documento con capas
/// restaurado desde su sidecar `.canvas`. `Design` es un `.canvas` autónomo:
/// el archivo abierto ES el documento, no la imagen que lo acompaña.
pub enum LoadOutcome {
    Flat(LoadedImage),
    Restored(RestoredDocument),
    Design(RestoredDocument),
}

/// El resultado de `canvas_io::open_document` (la política única de
/// apertura, en canvas-io) viaja como `LoadOutcome` por `AppMsg`; no hay
/// lógica de conversión: es la misma información con otro nombre.
impl From<canvas_io::OpenOutcome> for LoadOutcome {
    fn from(outcome: canvas_io::OpenOutcome) -> Self {
        match outcome {
            canvas_io::OpenOutcome::Flat(loaded) => LoadOutcome::Flat(loaded),
            canvas_io::OpenOutcome::Restored(restored) => LoadOutcome::Restored(restored),
            canvas_io::OpenOutcome::Design(restored) => LoadOutcome::Design(restored),
        }
    }
}

/// Resultado de una operación de archivos de la galería (crear, duplicar,
/// pegar): lo que transporta `AppMsg::GalleryOpDone`. Agrupa en un valor con
/// nombre los cuatro campos que antes viajaban como parámetros sueltos de
/// `on_gallery_op_done`.
#[derive(Debug)]
pub struct GalleryOpOutcome {
    /// Carpeta sobre la que se hizo la operación (para el rescan).
    pub folder: PathBuf,
    /// Ruta resultante, para abrirla o para que la galería la resalte al
    /// rescanear; ausente si la operación falló.
    pub created: Option<PathBuf>,
    pub result: Result<(), IoError>,
    /// Si venía de «✚ New design», abre el archivo recién creado.
    pub open: bool,
}

pub enum AppMsg {
    FilePicked(Option<PathBuf>),
    FolderPicked(Option<PathBuf>),
    ImageLoaded {
        path: PathBuf,
        result: Result<LoadOutcome, IoError>,
        /// ICC/EXIF del archivo original, para preservarlos al guardar.
        metadata: ImageMetadata,
    },
    /// Imagen cargada para AÑADIRSE como capa al documento abierto.
    ImageLoadedForLayer {
        path: PathBuf,
        result: Result<LoadedImage, IoError>,
    },
    /// Imagen cargada para REEMPLAZAR una capa de imagen concreta.
    ImageLoadedForReplace {
        layer: LayerId,
        label: String,
        source_path: Option<PathBuf>,
        result: Result<LoadedImage, IoError>,
    },
    /// Resultado de una búsqueda en Unsplash (fotos, sin miniaturas aún,
    /// y si era la última página). `seq` descarta respuestas caducas cuando
    /// se relanza con otros filtros.
    UnsplashSearch {
        query: String,
        seq: u64,
        page: u32,
        result: Result<crate::unsplash::SearchPage, crate::unsplash::UnsplashError>,
    },
    /// Miniatura de un resultado de Unsplash ya descargada y decodificada.
    UnsplashThumb {
        id: String,
        result: Result<LoadedImage, crate::unsplash::UnsplashError>,
    },
    /// Imagen completa de Unsplash descargada y decodificada, lista para
    /// insertarse como capa nueva del documento abierto.
    UnsplashImageReady {
        id: String,
        label: String,
        result: Result<LoadedImage, crate::unsplash::UnsplashError>,
    },
    SaveAsPicked(Option<PathBuf>),
    Saved {
        path: PathBuf,
        result: Result<(), IoError>,
        /// true si venía de «Guardar como…» y el documento debe apuntar aquí.
        new_source: bool,
    },
    /// Ruta elegida para exportar (o `None` si se canceló el diálogo).
    ExportPathPicked(Option<PathBuf>),
    Exported {
        path: PathBuf,
        result: Result<(), IoError>,
    },
    GalleryScanned {
        folder: PathBuf,
        /// (ruta, fecha de modificación si se pudo leer)
        files: Vec<(PathBuf, Option<std::time::SystemTime>)>,
    },
    GalleryScanFailed {
        folder: PathBuf,
        error: String,
    },
    /// Resultado de un reintento en segundo plano del listado de
    /// subcarpetas (montajes de nube que fallan de forma transitoria).
    FoldersRefreshed {
        folder: PathBuf,
        children: Vec<PathBuf>,
        error: Option<String>,
    },
    GalleryThumb {
        folder: PathBuf,
        path: PathBuf,
        result: Result<LoadedImage, IoError>,
    },
    /// Tamaños de página sondeados de toda una carpeta, en UN solo mensaje
    /// (la sonda es de cabecera: la carpeta entera son decenas de ms) para
    /// que la baraja del editor haga un único `relayout`, no uno por
    /// archivo. `None` por archivo si `probe_page_size` falló para ese uno.
    DeckProbed {
        folder: PathBuf,
        generation: u64,
        sizes: Vec<(PathBuf, Option<(f64, f64)>)>,
    },
    /// Un lienzo de la baraja terminó de cargar en segundo plano (scroll,
    /// tira, `PageUp`/`PageDown`). Deliberadamente NO es `ImageLoaded`: esa
    /// rama puede abrir un `rfd::MessageDialog` modal ("Image changed
    /// outside Canvas Desktop"), inaceptable para una carga disparada solo
    /// por hacer scroll — aquí un hash que no coincide se guarda como
    /// `external_change` en silencio y se muestra como el banner normal en
    /// cuanto la ranura se activa.
    SlotPrepared {
        folder: PathBuf,
        generation: u64,
        path: PathBuf,
        result: Result<crate::deck::SlotDoc, IoError>,
    },
    /// Nombre reservado en disco para una ranura PROVISIONAL de la baraja
    /// que el usuario acaba de empezar a editar. Solo reserva: el archivo se
    /// escribe después, por el camino de guardado de siempre
    /// (`start_save_design`), que es el único que tiene la GPU para hornear
    /// la miniatura embebida. `slot` es el id ESTABLE, no el índice — entre
    /// la petición y la respuesta la baraja puede haberse reordenado (mismo
    /// criterio que `save_all_queue`).
    CanvasPathReserved {
        folder: PathBuf,
        slot: u64,
        result: Result<PathBuf, IoError>,
    },
    /// Ruta llegada desde una segunda instancia (por el socket local).
    /// NOTA: `GalleryScanFailed`/`FoldersRefreshed`/`ShellIntegrationDone`
    /// siguen llevando `String` a propósito — su error ya nace redactado
    /// para la UI (`describe_read_dir_error`, la API de `canvas-shell`).
    OpenPathExternal(PathBuf),
    /// Una segunda instancia sin rutas pide traer la ventana al frente.
    FocusWindow,
    /// El archivo abierto cambió en disco (watcher `notify`).
    SourceChangedOnDisk {
        path: PathBuf,
    },
    /// Resultado del registro/desregistro de la integración con el shell.
    ShellIntegrationDone(Result<String, String>),
    /// Una operación de archivos de la galería (crear, duplicar, pegar)
    /// terminó. Ver `GalleryOpOutcome`.
    GalleryOpDone(GalleryOpOutcome),
    /// El archivo abierto en el editor se renombró (botón «✏» junto al
    /// nombre en el panel). No reutiliza `Saved`: renombrar no debe marcar
    /// el documento como recién guardado.
    DocumentRenamed {
        old_path: PathBuf,
        result: Result<PathBuf, IoError>,
    },
    /// El archivo abierto en el editor se envió a la Papelera (botón
    /// «Delete» del panel).
    DocumentDeleted {
        path: PathBuf,
        result: Result<(), IoError>,
    },
    /// Se restauró `path` desde la Papelera de reciclaje al deshacer un
    /// `GlobalStep::Delete` (`editor::EditorState::pending_restore`).
    DocumentRestored {
        path: PathBuf,
        result: Result<(), IoError>,
    },
    /// Respuesta del diálogo «¿guardar los cambios?» de una ventana o de
    /// una navegación. El modal corre en un hilo aparte a propósito:
    /// bloquear el pase de un viewport diferido con `rfd::…::show()`
    /// congela todo el event loop multi-ventana.
    UnsavedDialogAnswer(DialogDecision),
}

/// Qué decidió el usuario en un diálogo «¿guardar los cambios?».
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogDecision {
    /// Guardar y continuar (cerrar la ventana / abrir lo pedido).
    Save,
    /// Descartar los cambios y continuar.
    Discard,
    /// No hacer nada.
    Cancel,
}

/// Operación de archivos pedida desde la galería. Siempre en un hilo aparte:
/// copiar un PNG grande no puede bloquear la UI.
pub enum GalleryOp {
    /// Duplica `path` (y su sidecar, si es una imagen que tiene uno) dentro
    /// de la misma carpeta, con sufijo « copy».
    Duplicate {
        path: PathBuf,
    },
    /// Copia `src` (y su sidecar, si lo tiene) dentro de `folder`. Mismo
    /// nombre si no colisiona; si `src` ya está en `folder`, se comporta
    /// como `Duplicate` (sufijo « copy»).
    CopyInto {
        src: PathBuf,
        folder: PathBuf,
    },
    /// Cambia solo el nombre base (stem); la extensión no se toca —
    /// cambiarla rompería `is_image_file`/`is_canvas_file` y la detección
    /// de sidecar.
    Rename {
        path: PathBuf,
        new_stem: String,
    },
    /// A la Papelera de reciclaje (crate `trash`), no borrado permanente.
    Delete {
        path: PathBuf,
    },
    CreateFolder {
        parent: PathBuf,
        name: String,
    },
    RenameFolder {
        path: PathBuf,
        new_name: String,
    },
    DeleteFolder {
        path: PathBuf,
    },
}
