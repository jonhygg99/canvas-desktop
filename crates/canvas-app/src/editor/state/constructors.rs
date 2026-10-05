//! Como nace un `EditorState`: desde una imagen abierta del disco, o desde un
//! lienzo en blanco (diseno o imagen).

use std::path::PathBuf;

use canvas_core::{
    CoreError, Document, History, ImageContent, LayerContent, LayerId, Selection, Transform,
};
use canvas_io::LoadedImage;
use canvas_render::{image_data_from_rgba, ImageMap};

use super::super::interaction::Gesture;
use super::super::Viewport;
use super::EditorState;
use super::LeftTab;

impl EditorState {
    /// Constructor común: los tres puntos de entrada (imagen nueva, proyecto
    /// en blanco, restaurado desde sidecar) solo difieren en el documento, sus
    /// píxeles, la selección inicial y el fondo desenfocado.
    pub(super) fn base(
        doc: Document,
        images: ImageMap,
        selection: Selection,
        background_layer: Option<LayerId>,
    ) -> Self {
        let video_playing_layer = selection.primary().filter(|id| {
            doc.layer(*id).is_ok_and(
                |l| matches!(&l.content, LayerContent::Video(v) if v.source_path.is_some()),
            )
        });
        Self {
            saved_framing: doc
                .source_path
                .as_deref()
                .and_then(|path| canvas_io::read_framing(path).ok().flatten()),
            framing: None,
            framing_requested: false,
            doc,
            history: History::default(),
            images,
            selection,
            viewport: Viewport::default(),
            aspect_lock: true,
            gesture: Gesture::None,
            context_point: None,
            panel_edit: None,
            page_edit: None,
            size_popup: None,
            replace_url_popup: None,
            background_layer,
            opacity_edit: None,
            blur_edit: None,
            color_edit: None,
            content_edit: None,
            shadow_edit: None,
            saving: false,
            saving_capture: None,
            exporting: false,
            save_error: None,
            from_gallery: None,
            return_requested: false,
            save_clicked: false,
            save_as_clicked: false,
            settings_clicked: false,
            layers_panel_toggle: false,
            active_left_tab: LeftTab::Layers,
            unsplash: crate::unsplash::Panel::default(),
            serper: crate::serper::Panel::default(),
            ytdlp: crate::ytdlp::Panel::default(),
            sidecar_enabled: true,
            is_design: false,
            source_metadata: None,
            external_change: false,
            reload_requested: false,
            pending_zoom_factor: None,
            show_grid: false,
            show_rulers: false,
            isolate: false,
            crop_mode: false,
            snap_guides: (Vec::new(), Vec::new()),
            rename_edit: None,
            file_rename_edit: None,
            file_rename_requested: None,
            delete_requested: false,
            born_blank: false,
            pending_creation: false,
            deck_nav: None,
            press_on_other_slot: false,
            active_slot_id: 0,
            global_undo: Vec::new(),
            global_redo: Vec::new(),
            pending_global_undo: None,
            pending_global_redo: None,
            pending_restore: None,
            pending_delete_from_undo: false,
            video_playing_layer,
            video_playback: None,
        }
    }

    /// Documento nuevo a partir de una imagen: página a sus dimensiones
    /// reales y la imagen como capa a tamaño completo.
    pub fn from_image(path: PathBuf, img: LoadedImage) -> Result<Self, CoreError> {
        let is_video = canvas_io::is_video_file(&path);
        let (w, h) = (f64::from(img.width), f64::from(img.height));
        let mut doc = Document::new(w, h);
        doc.source_path = Some(path.clone());
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| {
                if is_video {
                    "Video".to_owned()
                } else {
                    "Image".to_owned()
                }
            });
        let content = if is_video {
            // Intentar obtener duración vía ffprobe (mejor esfuerzo)
            let duration = canvas_io::probe_video_size(&path)
                .ok()
                .and_then(|(_, _, d)| d);
            LayerContent::Video(canvas_core::VideoContent {
                source_path: Some(path.clone()),
                natural_width: img.width,
                natural_height: img.height,
                crop: None,
                duration_secs: duration,
                poster_time: 0.0,
                trim_start: 0.0,
                trim_end: None,
            })
        } else {
            LayerContent::Image(ImageContent {
                source_path: Some(path.clone()),
                natural_width: img.width,
                natural_height: img.height,
                crop: None,
            })
        };
        let id = doc.add_layer(name, Transform::new(0.0, 0.0, w, h), content)?;
        let mut images = ImageMap::new();
        images.insert(id, image_data_from_rgba(img.rgba, img.width, img.height));
        Ok(Self::base(doc, images, Selection::single(id), None))
    }

    /// Proyecto nuevo en blanco, como diseño autónomo `.canvas`: el primer
    /// guardado no rasteriza nada, sigue siendo un `.canvas` de pleno derecho.
    pub fn new_blank(width: f64, height: f64) -> Self {
        let mut doc = Document::new(width, height);
        if let Ok(page) = doc.page_mut() {
            page.background = Some([255, 255, 255, 255]);
        }
        let mut state = Self::base(doc, ImageMap::new(), Selection::default(), None);
        state.is_design = true;
        state.born_blank = true;
        state.pending_creation = true;
        state
    }

    /// Proyecto nuevo en blanco respaldado por un raster real (PNG/JPEG/
    /// WebP): el primer guardado hornea la página y escribe el archivo más
    /// su sidecar, por el mismo camino (`start_save`) que cualquier imagen
    /// editada. `sidecar_enabled` se fuerza a `true` — sin él, ese primer
    /// guardado escribiría un raster en blanco y perdería silenciosamente
    /// las capas que el usuario acabe de dibujar, aunque el ajuste global de
    /// sidecar estuviera desactivado.
    pub fn new_blank_image(width: f64, height: f64) -> Self {
        let mut doc = Document::new(width, height);
        if let Ok(page) = doc.page_mut() {
            page.background = Some([255, 255, 255, 255]);
        }
        let mut state = Self::base(doc, ImageMap::new(), Selection::default(), None);
        state.sidecar_enabled = true;
        state.born_blank = true;
        state.pending_creation = true;
        state
    }
}
