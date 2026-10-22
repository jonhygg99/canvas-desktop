//! Guardado de un lienzo de clip: qué composición se renderiza y dónde se
//! escribe.
//!
//! Un lienzo de clip es un documento cuyo `source_path` es un vídeo: la
//! página (fondo desenfocado + clip recortado) se renderiza a un mp4 con
//! ffmpeg. Esta política vive aquí y la comparten el `Ctrl+S`
//! (`views/editor/save_flow.rs`) y el autoguardado/la cola de alta
//! (`app/autosave.rs`, `app/write_queue.rs`): «¿dónde se guarda un clip?»
//! tiene una sola respuesta.
//!
//! Reglas de destino (`clip_save_target`, pura):
//!
//! - Solo un clip RENDERIZABLE: primera capa de vídeo visible con origen en
//!   disco, sin rotación/recorte/opacidad y sin contenido visible extra que
//!   ffmpeg no reproduciría (texto, formas, fotos, grupo aparte).
//! - Solo un destino que la app pueda escribir SIN preguntar: el nombre
//!   reservado de una ranura provisional dentro de la carpeta del proyecto
//!   (primer guardado) o el archivo del propio lienzo ya dentro del proyecto.
//!   Un mp4 suelto sin proyecto sigue pidiendo «Save as…»: es el único
//!   original y no se destruye sin avisar.
//! - La fuente temporal tiene que existir. Si se borró desde Download no hay
//!   composición que renderizar; el póster sigue en memoria y se guarda por
//!   «Save as…».
//!
//! El render corre en un hilo (`loader::spawn_video_save`); aquí solo se
//! decide, se sella la captura del historial y se lanza. QUIÉN lo lanza y
//! cuándo es asunto de la cola única de escritura (`app/write_queue.rs`), que
//! también escribe los lienzos de fondo sucios: sea activo o de fondo, un
//! clip se renderiza con la misma política de destino de aquí.

use std::path::{Path, PathBuf};

use canvas_core::{Document, LayerId};
use eframe::egui;

use crate::editor;
use crate::loader;

use super::frame::EditorFrame;
use super::persistence::SaveStart;

/// Composición a renderizar del documento: página + rectángulo de la capa de
/// vídeo + blur del fondo + trim. Pura (testeable).
pub(super) struct VideoCompositionRequest {
    pub(super) src: PathBuf,
    pub(super) start_s: f64,
    pub(super) end_s: Option<f64>,
    pub(super) page_w: u32,
    pub(super) page_h: u32,
    pub(super) fg_x: i32,
    pub(super) fg_y: i32,
    pub(super) fg_w: u32,
    pub(super) fg_h: u32,
    pub(super) bg_blur: f32,
}

/// ¿El documento ES un clip (su origen es un vídeo)? Solo entonces se
/// renderiza: un lienzo mixto (foto + capa de vídeo insertada) sigue el camino
/// raster de siempre aunque tenga capas de vídeo.
pub(super) fn is_clip_document(doc: &Document) -> bool {
    doc.source_path
        .as_deref()
        .is_some_and(canvas_io::is_video_file)
}

/// ¿El clip de origen ya no está en disco (se borró el temporal desde
/// Download)? Entonces no hay composición que renderizar, pero el póster
/// sigue en memoria y «Save as» lo salva. Pura (testeable).
pub(super) fn source_missing(doc: &Document) -> bool {
    let Ok(page) = doc.page() else {
        return false;
    };
    page.layers
        .iter()
        .find_map(|layer| match &layer.content {
            canvas_core::LayerContent::Video(link) => Some(link),
            _ => None,
        })
        .and_then(|link| link.source_path.clone())
        .is_some_and(|src| !src.is_file())
}

/// Composición del lienzo ACTIVO.
pub(super) fn video_composition_request(
    state: &editor::EditorState,
) -> Option<VideoCompositionRequest> {
    composition_of(&state.doc, state.background_layer)
}

/// Resuelve la composición a renderizar: la primera capa de vídeo visible con
/// origen en disco + su In/Out no destructivo, el tamaño de página y el blur
/// del fondo. `None` cuando el documento NO es renderizable a vídeo — sin capa
/// de vídeo con origen, o con contenido visible extra que ffmpeg no
/// reproduciría (texto, formas, fotos, un vídeo rotado/recortado/con
/// transparencia): entonces el guardado sigue su camino raster de siempre en
/// vez de inventar un mp4 a medias. Pura (testeable). Funciona también sobre el
/// documento de una ranura de fondo (la cola de clips la necesita).
pub(super) fn composition_of(
    doc: &Document,
    background_layer: Option<LayerId>,
) -> Option<VideoCompositionRequest> {
    let page = doc.page().ok()?;
    let video = page.layers.iter().find(|layer| {
        layer.visible && matches!(layer.content, canvas_core::LayerContent::Video(_))
    })?;
    let canvas_core::LayerContent::Video(link) = &video.content else {
        unreachable!("filtrado arriba");
    };
    let src = link.source_path.clone()?;
    // Solo la composición estándar (vídeo colocado + fondo con blur).
    // Pura a propósito (sin `is_file`): que el temporal se borrara se detecta
    // en el flujo de guardado (`source_missing`), no aquí — los tests
    // construyen composiciones sobre rutas ficticias.
    if video.transform.rotation != 0.0 || video.transform.flip_h || video.transform.flip_v {
        return None;
    }
    if link.crop.is_some() || video.opacity < 0.999 {
        return None;
    }
    let extra = page.layers.iter().any(|layer| {
        layer.visible
            && layer.id != video.id
            && Some(layer.id) != background_layer
            && !matches!(layer.content, canvas_core::LayerContent::Group(_))
    });
    if extra {
        return None;
    }
    let bg_blur = background_layer
        .and_then(|id| page.layer(id))
        .filter(|bg| bg.visible)
        .map(|bg| bg.effects.blur_radius)
        .unwrap_or(0.0);
    // x264/yuv420p exigen dimensiones pares: el transform se redondea a px y
    // se baja 1 si sale impar (1 px frente al editor, documentado). La
    // posición va con signo: el `overlay` recorta igual que el editor.
    let even = |v: f64| (v.round() as u32).max(2) & !1;
    Some(VideoCompositionRequest {
        src,
        start_s: link.trim_start,
        end_s: link.trim_end,
        page_w: even(page.width),
        page_h: even(page.height),
        fg_x: video.transform.x.round() as i32,
        fg_y: video.transform.y.round() as i32,
        fg_w: even(video.transform.width),
        fg_h: even(video.transform.height),
        bg_blur,
    })
}

impl VideoCompositionRequest {
    /// Empaqueta la composición con su destino para el worker de render.
    pub(super) fn into_composition(self, dst: PathBuf) -> canvas_io::VideoComposition {
        canvas_io::VideoComposition {
            src: self.src,
            start_s: self.start_s,
            end_s: self.end_s,
            page_w: self.page_w,
            page_h: self.page_h,
            fg_x: self.fg_x,
            fg_y: self.fg_y,
            fg_w: self.fg_w,
            fg_h: self.fg_h,
            bg_blur: self.bg_blur,
            dst,
        }
    }
}

/// Destino del guardado EN EL SITIO de un lienzo de clip: el primer guardado de
/// una ranura provisional (el nombre reservado, con el temporal como origen) o
/// el archivo del propio lienzo dentro del proyecto. `None` si el clip no es
/// renderizable, si su fuente desapareció o si no hay dónde escribir sin
/// preguntar (mp4 suelto, baraja sin carpeta). Pura (tabla de tests).
pub(super) fn clip_save_target(
    doc: &Document,
    background: Option<LayerId>,
    slot_path: &Path,
    is_placeholder: bool,
    folder: Option<&Path>,
) -> Option<PathBuf> {
    if !is_clip_document(doc) {
        return None;
    }
    let request = composition_of(doc, background)?;
    if is_placeholder {
        // Primer guardado: el nombre reservado es el destino y el temporal de
        // Download es el origen. Sin origen en disco no hay nada que renderizar.
        let _folder = folder?;
        if !request.src.is_file() || !canvas_io::is_video_file(slot_path) {
            return None;
        }
        return Some(slot_path.to_path_buf());
    }
    let src = doc.source_path.clone()?;
    // Un mp4 suelto sin sidecar sigue siendo el único original: diálogo.
    if folder.is_none() && request.src == src {
        return None;
    }
    // Se comprueba la ENTRADA (lo que ffmpeg lee), no la salida. Si se borró
    // el mp4 del lienzo no hay motivo para negarse: `replace_file` cae a
    // `rename` cuando el destino no existe y lo vuelve a crear. Si lo que
    // falta es la fuente del clip, no hay composición que renderizar y
    // reintentarlo solo repite el fallo.
    if slot_path != src || !request.src.is_file() {
        return None;
    }
    Some(src)
}

/// Lanza el renderizado en el sitio del lienzo ACTIVO: sella la captura del
/// historial (A04) y delega en el worker. Con un render en curso no se lanza
/// otro — dos workers escribirían el mismo temporal y el mp4 saldría corrupto.
pub(super) fn start_clip_save(
    state: &mut editor::EditorState,
    f: &mut EditorFrame<'_>,
    ctx: &egui::Context,
    dst: PathBuf,
) -> SaveStart {
    if state.saving {
        return SaveStart::Refused;
    }
    let Some(request) = video_composition_request(state) else {
        return SaveStart::RefusedPermanent;
    };
    // Suelta los handles propios ANTES de tocar disco, pero SOLO si se
    // reescribe el archivo que el preview está leyendo: el preview (ffmpeg
    // hijo leyendo el origen) y el audio (rodio con el `File` abierto)
    // bloquean `ReplaceFileW` con 0x80070020. Cuando el destino es OTRO
    // archivo (el mp4 del proyecto desde el temporal, o una ranura de fondo)
    // no hay nada que soltar y parar la reproducción del usuario sería
    // gratuito. El reintento de `replace_file_with_tmp` cubre lo transitorio
    // (Drive, cierre en curso).
    if dst == request.src {
        state.pause_video();
        crate::audio::stop();
    }
    tracing::info!(
        "render vídeo {}x{} desde {} (trim {:?}-{:?}) a {}",
        request.page_w,
        request.page_h,
        request.src.display(),
        request.start_s,
        request.end_s,
        dst.display()
    );
    state.saving = true;
    state.saving_capture = Some((state.history.undo_depth(), state.history.revision()));
    state.save_error = None;
    let capture = state.saving_capture.expect("capturado justo arriba");
    let target = loader::ImageInsertDest {
        generation: f.deck.generation(),
        slot_id: f.deck.slots.get(f.deck.active).map_or(0, |s| s.id),
    };
    let reserve = f
        .deck
        .slots
        .get(f.deck.active)
        .is_some_and(|s| s.is_placeholder && s.path == dst)
        && f.deck.folder.is_some();
    loader::spawn_video_save(
        loader::VideoSaveRequest {
            target,
            capture,
            payload: state.sidecar_payload(),
            reserve,
            composition: request.into_composition(dst),
        },
        f.tx.clone(),
        ctx.clone(),
    );
    SaveStart::Started
}

#[cfg(test)]
#[path = "clip_save_tests.rs"]
mod tests;
