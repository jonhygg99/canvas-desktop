//! Guardado y exportación: arranque de los hilos de guardado (imagen,
//! diseño autónomo, export), "Save all" en cola, y los ayudantes que
//! preparan lo que esos hilos necesitan (slots de la baraja, la rejilla de
//! la galería al volver del editor, resolución de sidecars).

use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::time::Instant;

use canvas_core::{Document, LayerContent};
use canvas_render::CanvasRenderer;
use eframe::egui;
use eframe::egui_wgpu::RenderState;

use crate::loader::{self, AppMsg};
use crate::{deck, editor, export, gallery, settings};

use super::{AppInner, View, Workspace};

/// Préstamos que las funciones de guardado necesitan del `AppInner` y del
/// workspace durante un frame.
pub(in crate::app) struct SaveContext<'a> {
    pub renderer: &'a mut CanvasRenderer,
    pub rs: &'a RenderState,
    pub tx: &'a Sender<AppMsg>,
    pub ctx: &'a egui::Context,
    pub ignore_fs_events_until: &'a mut Option<Instant>,
    /// `FxScope` de la ranura activa de esta ventana (único por ranura a
    /// nivel de proceso). El guardado lo usa en vez del scope por defecto:
    /// antes, TODOS los guardados de TODAS las ventanas compartían el
    /// scope 0 y dos guardados a la vez se pisaban la caché de efectos.
    /// Además, al coincidir con el scope con el que la ranura se renderiza,
    /// el horneado reutiliza las texturas de efectos ya calculadas.
    pub scope: u64,
}

impl AppInner {
    /// «Save all»: encola las ranuras de fondo sucias (id estable, no
    /// índice). El documento ACTIVO, si está sucio, se guarda aparte y de
    /// inmediato; la cola solo lleva lo demás. Deja fuera SVG/GIF.
    pub(crate) fn start_save_all(&mut self, ws: &mut Workspace) {
        let View::Editor(state) = &mut ws.view else {
            return;
        };
        if state.is_dirty()
            && state
                .doc
                .source_path
                .as_deref()
                .is_some_and(canvas_io::can_overwrite)
        {
            state.save_clicked = true;
        }
        ws.save.save_all_queue = ws
            .deck
            .slots
            .iter()
            .filter(|s| {
                !s.is_placeholder
                    && matches!(&s.content, deck::SlotContent::Ready(d) if d.history.is_dirty())
                    && canvas_io::can_overwrite(&s.path)
            })
            .map(|s| s.id)
            .collect();
        ws.save.save_all_attempted = false;
    }
}

/// Al volver a la galería desde el editor, siembra la rejilla con lo que ya
/// tenía la baraja (miniaturas ya subidas a GPU).
pub(super) fn seed_gallery_from_deck(
    deck: &deck::Deck,
    folder: PathBuf,
    sort: settings::GallerySort,
    folder_panel_side: deck::StripSide,
) -> gallery::GalleryState {
    let mut g = gallery::GalleryState::new(folder.clone(), sort, folder_panel_side);
    if deck.folder.as_deref() == Some(folder.as_path()) {
        g.items = deck
            .slots
            .iter()
            .filter(|s| !s.is_placeholder)
            .map(|s| gallery::GalleryItem {
                path: s.path.clone(),
                name: s.name.clone(),
                mtime: s.mtime,
                kind: s.kind,
                tex: s.thumb.clone(),
                failed: s.thumb_failed,
            })
            .collect();
        g.scanned = !g.items.is_empty();
        g.apply_sort();
    }
    g
}

/// Construye el `SlotDoc` de una carga de fondo de la baraja, reutilizando
/// los constructores de `EditorState`.
pub(crate) fn build_slot_doc(
    path: PathBuf,
    outcome: loader::LoadOutcome,
    metadata: Option<canvas_io::ImageMetadata>,
    sidecar_default: bool,
) -> Option<deck::SlotDoc> {
    match outcome {
        loader::LoadOutcome::Restored(restored) => {
            let external_change = !restored.hash_matches;
            let mut state = editor::EditorState::from_restored(path, restored);
            state.sidecar_enabled = sidecar_default;
            state.source_metadata = metadata;
            state.external_change = external_change;
            Some(state.take_slot())
        }
        loader::LoadOutcome::Design(restored) => {
            let mut state = editor::EditorState::from_design(path, restored);
            Some(state.take_slot())
        }
        loader::LoadOutcome::Flat(img) => match editor::EditorState::from_image(path, img) {
            Ok(mut state) => {
                state.sidecar_enabled = sidecar_default;
                state.source_metadata = metadata;
                Some(state.take_slot())
            }
            Err(e) => {
                tracing::warn!("carga de fondo: no se pudo construir el documento: {e}");
                None
            }
        },
    }
}

/// Hornea la página en la GPU (hilo de UI) y delega codificar+escribir a un
/// hilo de trabajo.
pub(super) fn start_save(
    state: &mut editor::EditorState,
    sctx: &mut SaveContext,
    path: PathBuf,
    new_source: bool,
    jpeg_quality: u8,
) {
    if state.saving {
        return;
    }
    tracing::info!("guardando en {}", path.display());
    let scope = canvas_render::FxScope(sctx.scope);
    sctx.renderer.forget_scope(scope);
    match sctx.renderer.bake_page(
        &sctx.rs.device,
        &sctx.rs.queue,
        scope,
        &state.doc,
        &state.images,
        1.0,
    ) {
        Ok((rgba, width, height)) => {
            if bake_came_out_blank(&state.doc, &rgba) {
                tracing::error!(
                    "horneado en blanco con capas de imagen visibles; \
                     no se sobrescribe el archivo"
                );
                state.save_error = Some(
                    "The image came out blank — the file was not overwritten. \
                     Close other apps to free memory and try again."
                        .into(),
                );
                return;
            }
            state.saving = true;
            state.save_error = None;
            *sctx.ignore_fs_events_until =
                Some(std::time::Instant::now() + std::time::Duration::from_secs(5));
            let sidecar = state.sidecar_enabled.then(|| state.sidecar_payload());
            loader::spawn_save(
                loader::SaveInput {
                    path,
                    rgba,
                    width,
                    height,
                    jpeg_quality,
                    metadata: state.source_metadata.clone(),
                    new_source,
                    sidecar,
                },
                sctx.tx.clone(),
                sctx.ctx.clone(),
            );
        }
        Err(e) => {
            tracing::error!("horneado falló: {e}");
            state.save_error = Some(format!("Could not prepare the save: {e}"));
        }
    }
}

/// Guarda un diseño autónomo.
pub(super) fn start_save_design(
    state: &mut editor::EditorState,
    sctx: &mut SaveContext,
    path: PathBuf,
    new_source: bool,
) {
    if state.saving {
        return;
    }
    tracing::info!("guardando diseño en {}", path.display());
    state.is_design = true;
    let mut payload = state.sidecar_payload();
    let (pw, ph) = state
        .doc
        .page()
        .map(|p| (p.width, p.height))
        .unwrap_or((0.0, 0.0));
    let scale = canvas_io::preview_scale(pw, ph);
    let scope = canvas_render::FxScope(sctx.scope);
    sctx.renderer.forget_scope(scope);
    match sctx.renderer.bake_page(
        &sctx.rs.device,
        &sctx.rs.queue,
        scope,
        &state.doc,
        &state.images,
        scale,
    ) {
        Ok((rgba, w, h)) => payload.preview = canvas_io::make_preview(&rgba, w, h),
        Err(e) => tracing::warn!("miniatura del diseño no horneada: {e}"),
    }
    state.saving = true;
    state.save_error = None;
    *sctx.ignore_fs_events_until =
        Some(std::time::Instant::now() + std::time::Duration::from_secs(5));
    loader::spawn_save_design(path, payload, new_source, sctx.tx.clone(), sctx.ctx.clone());
}

/// Lo pedido en el diálogo de exportación ya resuelto: destino, ajustes y
/// scope de la ranura activa. Agrupado para reducir la firma de
/// `start_export` de 8 a 6 parámetros.
pub(super) struct ExportRequest {
    pub(super) path: PathBuf,
    pub(super) settings: export::ExportSettings,
    /// `FxScope` de la ranura activa (único por ranura a nivel de proceso):
    /// el export no debe compartir el scope 0 con otras ventanas.
    pub(super) scope: u64,
}

/// PNG/JPEG hornean en la GPU igual que al guardar; SVG/PDF se generan a
/// mano a partir del documento.
pub(super) fn start_export(
    state: &mut editor::EditorState,
    renderer: &mut CanvasRenderer,
    rs: &RenderState,
    tx: &Sender<AppMsg>,
    ctx: &egui::Context,
    request: ExportRequest,
) {
    if state.exporting {
        return;
    }
    let ExportRequest {
        path,
        settings,
        scope,
    } = request;
    tracing::info!("exportando a {}", path.display());
    let scale = f64::from(settings.scale);
    let scope = canvas_render::FxScope(scope);
    renderer.forget_scope(scope);

    if settings.format.needs_bake() {
        match renderer.bake_page(
            &rs.device,
            &rs.queue,
            scope,
            &state.doc,
            &state.images,
            scale,
        ) {
            Ok((rgba, width, height)) => {
                if bake_came_out_blank(&state.doc, &rgba) {
                    tracing::error!(
                        "export horneado en blanco con capas de imagen visibles; no se escribe"
                    );
                    state.save_error = Some(
                        "The export came out blank — nothing was written. \
                         Close other apps to free memory and try again."
                            .into(),
                    );
                    return;
                }
                state.exporting = true;
                state.save_error = None;
                loader::spawn_export_raster(
                    path,
                    rgba,
                    width,
                    height,
                    settings.jpeg_quality,
                    tx.clone(),
                    ctx.clone(),
                );
            }
            Err(e) => {
                tracing::error!("horneado falló: {e}");
                state.save_error = Some(format!("Could not prepare the export: {e}"));
            }
        }
        return;
    }

    if let Ok(page) = state.doc.page() {
        for layer in &page.layers {
            if let Some(source) = state.images.get(&layer.id) {
                renderer.sync_layer_effects(
                    &rs.device,
                    &rs.queue,
                    scope,
                    layer.id,
                    source,
                    &layer.effects,
                );
            }
        }
    }
    let blurred = renderer.blur_overrides(scope);
    let mut images: Vec<canvas_io::LayerPixels> = Vec::new();
    if let Ok(page) = state.doc.page() {
        for layer in &page.layers {
            // Las capas SIN efectos deben embeber sus píxeles ORIGINALES:
            // `blurred` también contiene la copia reducida de pantalla de las
            // imágenes que superan el tope del atlas (ver `MAX_FX_DIM`), y
            // aplanarla en el SVG degradaría la resolución del vector.
            let has_effects =
                layer.effects.blur_radius > 0.0 || layer.effects.has_color_adjustments();
            let data = if has_effects {
                blurred
                    .get(&layer.id)
                    .or_else(|| state.images.get(&layer.id))
            } else {
                state.images.get(&layer.id)
            };
            let Some(data) = data else {
                continue;
            };
            images.push((
                layer.id.raw(),
                data.data.data().to_vec(),
                data.width,
                data.height,
            ));
        }
    }
    state.exporting = true;
    state.save_error = None;
    loader::spawn_export_vector(
        path,
        state.doc.clone(),
        images,
        settings.format,
        scale,
        tx.clone(),
        ctx.clone(),
    );
}

/// ¿El horneado salió UNIFORME (un solo color) pese a que el documento tiene
/// capas de imagen visibles que deberían pintar? Un resultado así casi
/// siempre significa que la GPU falló al dibujar las capas (presión de
/// memoria, atlas de vello sin espacio para las texturas): escribir ese
/// horneado sobre el archivo del usuario lo destruiría en silencio (el
/// fondo de página es blanco, así que un bake fallido es un PNG blanco
/// entero). La protección es deliberadamente conservadora: solo dispara con
/// un bake de UN solo color y capas de imagen visibles; un diseño
/// vectorial legítimamente monocromo, o una foto realmente uniforme, son
/// casos límite aceptables — mejor un error claro que un archivo destruido.
fn bake_came_out_blank(doc: &Document, rgba: &[u8]) -> bool {
    let has_visible_images = doc.page().is_ok_and(|page| {
        page.layers.iter().any(|layer| {
            layer.visible && matches!(layer.content, LayerContent::Image(_) | LayerContent::Svg(_))
        })
    });
    if !has_visible_images {
        return false;
    }
    let mut first: Option<[u8; 4]> = None;
    for px in rgba.chunks_exact(4) {
        let current = [px[0], px[1], px[2], px[3]];
        match first {
            None => first = Some(current),
            Some(prev) if prev != current => return false,
            _ => {}
        }
    }
    true
}

/// ¿La extensión de `path` es JPEG? (para el aviso de calidad de recompresión)
pub(super) fn is_jpeg_path(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "jpg" | "jpeg"))
}

/// `foto.png.canvas` (hermano legacy) o `.canvas/foto.png.canvas` (ubicación
/// actual) → `foto.png` si esa imagen existe; cualquier otra ruta se
/// devuelve tal cual.
pub(super) fn resolve_canvas_sidecar(path: PathBuf) -> PathBuf {
    if !canvas_io::is_canvas_file(&path) {
        return path;
    }
    let in_sidecar_dir = path
        .parent()
        .and_then(|p| p.file_name())
        .is_some_and(|n| n == canvas_io::SIDECAR_DIR);
    if in_sidecar_dir {
        if let Some(grandparent) = path.parent().and_then(|p| p.parent()) {
            if let Some(stem) = path.file_stem() {
                let inner = grandparent.join(stem);
                if canvas_io::is_image_file(&inner) && inner.is_file() {
                    return inner;
                }
            }
        }
        return path;
    }
    let inner = path.with_extension("");
    if canvas_io::is_image_file(&inner) && inner.is_file() {
        return inner;
    }
    path
}

#[cfg(test)]
#[path = "persistence_tests.rs"]
mod resolve_canvas_sidecar_tests;
