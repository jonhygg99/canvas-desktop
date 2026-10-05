//! Trabajo en hilos aparte: carga de imágenes y diálogos nativos. La UI nunca
//! bloquea en disco; los resultados llegan por canal.
//!
//! Dividido en submódulos por dominio: `load_ops` (abrir imágenes/diseños/
//! ranuras de la baraja, y los diálogos de abrir archivo/carpeta),
//! `save_ops` (guardar), `export_ops` (exportar), `gallery_ops`
//! (operaciones de archivos de la galería), `image_import` (añadir/
//! reemplazar una capa de imagen, incluida la descarga por URL),
//! `unsplash_ops` (búsqueda de imágenes de Unsplash) y `serper_ops`
//! (búsqueda de imágenes web vía Serper).

mod export_ops;
mod file_ops;
mod gallery_ops;
mod image_import;
mod load_ops;
mod repair_ops;
mod save_ops;
mod serper_ops;
mod unsplash_ops;
mod ytdlp_ops;

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
pub use repair_ops::{auto_action_for, spawn_repair_png, AutoRepairAction};
pub use save_ops::{
    spawn_pick_design_path, spawn_pick_save_path, spawn_reserve_canvas_path, spawn_save,
    spawn_save_design, SaveInput,
};
pub use serper_ops::{
    begin_bulk_files, default_bulk_dir, resolve_bulk_page, spawn_serper_image, spawn_serper_search,
    spawn_serper_thumb, BulkItem, SerperImageRequest,
};
pub use unsplash_ops::{spawn_unsplash_image, spawn_unsplash_search, spawn_unsplash_thumb};
pub use ytdlp_ops::{
    spawn_ytdlp_delete, spawn_ytdlp_download, spawn_ytdlp_frames, YtdlpDownloadRequest,
    YtdlpFramesOutcome,
};

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
// pegar): lo que transporta `AppMsg::GalleryOpDone`. Agrupa en un valor con
// nombre los cuatro campos que antes viajaban como parámetros sueltos de
// `on_gallery_op_done`.
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

/// Destino de una inserción de imagen pedida desde un panel (clic o
/// arrastre): generación de la baraja + id estable de la ranura activa en el
/// momento del clic. Se captura donde hay baraja a mano (el lienzo o la tira
/// de paneles) y viaja con la petición; el handler solo aplica la imagen si
/// el destino sigue vigente (A07).
#[derive(Debug, Clone, Copy)]
pub struct ImageInsertDest {
    pub generation: u64,
    pub slot_id: u64,
}

/// Identidad completa de una inserción asíncrona de imagen (Unsplash/Serper):
/// destino + nº de petición del panel + foto pedida. No se usa la URL como
/// identidad: dos peticiones seguidas con la misma URL son indistinguibles
/// por URL pero tienen distinto `seq` (A07).
#[derive(Debug, Clone)]
pub struct ImageInsertTarget {
    pub dest: ImageInsertDest,
    pub seq: u64,
    pub photo_id: String,
}

/// ¿Sigue vigente el destino de una respuesta de inserción? Pura y
/// testeable: la petición pendiente del panel debe ser esta misma (`seq` +
/// foto) Y la baraja debe seguir en la misma generación con la misma ranura
/// activa. Si el usuario saltó de lienzo, cambió de proyecto o cerró entre
/// medias, la respuesta se descarta en vez de insertarse en otro documento.
pub fn insert_target_current(
    pending: Option<&ImageInsertTarget>,
    msg: &ImageInsertTarget,
    generation: u64,
    slot_id: Option<u64>,
) -> bool {
    pending.is_some_and(|t| t.seq == msg.seq && t.photo_id == msg.photo_id)
        && msg.dest.generation == generation
        && Some(msg.dest.slot_id) == slot_id
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
        /// Destino que la pidió (A07): solo se inserta si sigue vigente.
        target: ImageInsertTarget,
    },
    /// Resultado de UNA llamada web/Serper (1 token = 1 página de hasta
    /// 100 fotos ya filtradas). `seq` descarta respuestas caducas y
    /// `cache_key` alimenta la caché de memoria en el handler.
    SerperSearch {
        seq: u64,
        page: u32,
        cache_key: String,
        result: Result<crate::serper::SearchPage, crate::serper::SerperError>,
    },
    /// Miniatura de un resultado web ya descargada y decodificada.
    /// `FilteredBanner` significa que el thumb era un banner (se retira la
    /// tarjeta en silencio, no es un error visible).
    SerperThumb {
        id: String,
        result: Result<LoadedImage, crate::serper::SerperError>,
    },
    /// Imagen web completa descargada y decodificada, lista para insertarse
    /// como capa nueva del documento abierto.
    SerperImageReady {
        id: String,
        label: String,
        result: Result<crate::serper::FetchedImage, crate::serper::SerperError>,
        /// Destino que la pidió (A07): solo se inserta si sigue vigente.
        target: ImageInsertTarget,
    },
    /// Progreso de la creación masiva web (hechas, total).
    SerperBulkProgress {
        done: usize,
        total: usize,
    },
    /// La creación masiva terminó: lienzos creados y errores por imagen.
    SerperBulkDone {
        folder: PathBuf,
        created: Vec<PathBuf>,
        errors: Vec<String>,
        /// Cuántos lienzos usan píxeles de rescate en vez de la URL directa
        /// (A08): se avisa en el mensaje final.
        substituted: usize,
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
        // En caja a propósito: `SlotDoc` (documento + píxeles + historial)
        // es la variante grande de `AppMsg` y viaja por valor por el canal —
        // sin el `Box`, clippy `large_enum_variant` falla con `-D warnings`.
        result: Result<Box<crate::deck::SlotDoc>, IoError>,
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
    /// Progreso de una descarga yt-dlp (índice, total, texto).
    YtdlpDownloadProgress {
        index: usize,
        total: usize,
        text: String,
    },
    /// Tanda de descargas yt-dlp terminada.
    YtdlpDownloadDone {
        folder: PathBuf,
        paths: Vec<PathBuf>,
        errors: Vec<String>,
    },
    /// Rutas mandadas a la papelera desde el Download (borrado unitario o
    /// Clear): las que salieron y los fallos redactados.
    YtdlpFilesDeleted {
        removed: Vec<PathBuf>,
        errors: Vec<String>,
    },
    /// Fotogramas de vista previa listos para el editor.
    YtdlpFramesReady(YtdlpFramesOutcome),
    /// La vista previa falló (guía visible en la ventana).
    YtdlpFramesFailed {
        clip_id: String,
        error: String,
    },
    /// Aceptar de la ventana Editar: crear el lienzo nuevo con estos params.
    YtdlpEditAccepted(crate::ytdlp::VideoAccept),
    /// Un PNG con checksum roto se reparó en su mismo nombre: la corrupta
    /// está en la papelera del sistema y `repaired` (== `original`) ya
    /// trae los píxeles sanos.
    PngRepaired {
        original: PathBuf,
        repaired: PathBuf,
    },
    /// La reparación no fue posible: `kind` dice por qué (vacío, truncado,
    /// daño mayor) y `message` lleva el texto para la UI/log.
    RepairFailed {
        original: PathBuf,
        kind: canvas_io::CorruptionKind,
        message: String,
    },
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
    /// A la cuarentena del proyecto (`.canvas/quarantine/`): aparta un
    /// archivo ilegible sin borrarlo. Sin deshacer integrado (a diferencia
    /// del borrado del editor): restaurar es moverlo de vuelta a mano.
    Quarantine {
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
