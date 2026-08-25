//! Guardado y exportación: arranque de los hilos de guardado (imagen,
//! diseño autónomo, export), "Save all" en cola, y los ayudantes que
//! preparan lo que esos hilos necesitan (slots de la baraja, la rejilla de
//! la galería al volver del editor, resolución de sidecars).

use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::time::Instant;

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
    sctx.renderer
        .forget_scope(canvas_render::FxScope::default());
    match sctx.renderer.bake_page(
        &sctx.rs.device,
        &sctx.rs.queue,
        canvas_render::FxScope::default(),
        &state.doc,
        &state.images,
        1.0,
    ) {
        Ok((rgba, width, height)) => {
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
    sctx.renderer
        .forget_scope(canvas_render::FxScope::default());
    match sctx.renderer.bake_page(
        &sctx.rs.device,
        &sctx.rs.queue,
        canvas_render::FxScope::default(),
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

/// PNG/JPEG hornean en la GPU igual que al guardar; SVG/PDF se generan a
/// mano a partir del documento.
pub(super) fn start_export(
    state: &mut editor::EditorState,
    renderer: &mut CanvasRenderer,
    rs: &RenderState,
    tx: &Sender<AppMsg>,
    ctx: &egui::Context,
    path: PathBuf,
    settings: export::ExportSettings,
) {
    if state.exporting {
        return;
    }
    tracing::info!("exportando a {}", path.display());
    let scale = f64::from(settings.scale);
    renderer.forget_scope(canvas_render::FxScope::default());

    if settings.format.needs_bake() {
        match renderer.bake_page(
            &rs.device,
            &rs.queue,
            canvas_render::FxScope::default(),
            &state.doc,
            &state.images,
            scale,
        ) {
            Ok((rgba, width, height)) => {
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
                    canvas_render::FxScope::default(),
                    layer.id,
                    source,
                    &layer.effects,
                );
            }
        }
    }
    let blurred = renderer.blur_overrides(canvas_render::FxScope::default());
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
mod resolve_canvas_sidecar_tests {
    use super::resolve_canvas_sidecar;

    #[test]
    fn resolves_a_sidecar_inside_the_dot_canvas_folder_to_its_image() {
        let dir = tempfile::tempdir().expect("tempdir");
        let image = dir.path().join("foto.png");
        std::fs::write(&image, b"x").unwrap();
        let sidecar_dir = dir.path().join(canvas_io::SIDECAR_DIR);
        std::fs::create_dir_all(&sidecar_dir).unwrap();
        let sidecar = sidecar_dir.join("foto.png.canvas");
        std::fs::write(&sidecar, b"{}").unwrap();

        assert_eq!(resolve_canvas_sidecar(sidecar), image);
    }

    #[test]
    fn resolves_a_legacy_sibling_sidecar_to_its_image() {
        let dir = tempfile::tempdir().expect("tempdir");
        let image = dir.path().join("foto.png");
        std::fs::write(&image, b"x").unwrap();
        let sidecar = dir.path().join("foto.png.canvas");
        std::fs::write(&sidecar, b"{}").unwrap();

        assert_eq!(resolve_canvas_sidecar(sidecar), image);
    }

    #[test]
    fn a_standalone_design_is_returned_as_is() {
        let dir = tempfile::tempdir().expect("tempdir");
        let design = dir.path().join("Untitled.canvas");
        std::fs::write(&design, b"{}").unwrap();

        assert_eq!(resolve_canvas_sidecar(design.clone()), design);
    }
}
