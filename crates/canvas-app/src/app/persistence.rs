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

use crate::deck::{free_ram_bytes, is_critical_free_ram};
use crate::loader::{self, AppMsg};
use crate::{deck, editor, export, gallery, settings};

use super::{AppInner, SaveFlow, View, Workspace};

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
    /// índice) y guarda el documento ACTIVO, si está sucio, de inmediato.
    /// Los formatos protegidos piden Guardar como. Si la RAM libre está por debajo del
    /// umbral, NO encola nada y abre el aviso de poca memoria: un horneado
    /// masivo bajo presión es la receta del PNG blanco (ver
    /// `bake_came_out_blank`), y mejor avisar antes de lanzar N horneados.
    /// El botón «Save all anyway» del modal llama a `start_save_all_flow`
    /// directamente.
    pub(crate) fn start_save_all(&mut self, ws: &mut Workspace) {
        let View::Editor(state) = &mut ws.view else {
            return;
        };
        if should_warn_low_memory(crate::deck::free_ram_bytes()) {
            let count = save_all_doc_count(&ws.deck, state);
            if count > 0 {
                ws.save.low_memory_prompt = Some(count);
                return;
            }
        }
        start_save_all_flow(state, &mut ws.deck, &mut ws.save);
    }
}

/// Construye la cola de «Save all»: guarda el documento activo (si está
/// sucio) y encola todas las ranuras de fondo sucias (id
/// estable, no índice). Libre para poder re-dispararse desde el modal de
/// aviso de poca RAM, donde `state`/`deck`/`save` son préstamos disjuntos
/// del mismo workspace.
pub(super) fn start_save_all_flow(
    state: &mut editor::EditorState,
    deck: &mut deck::Deck,
    save: &mut SaveFlow,
) {
    if deck.unsaved_session {
        save.save_all_queue = session_dirty_ids(deck, state);
        save.save_all_attempted = false;
        return;
    }
    if state.is_dirty() {
        state.save_clicked = true;
    }
    save.save_all_queue = deck
        .slots
        .iter()
        .filter(|s| matches!(&s.content, deck::SlotContent::Ready(d) if d.history.is_dirty()))
        .map(|s| s.id)
        .collect();
    save.save_all_attempted = false;
}

/// Revalida todos los lienzos antes de ejecutar una salida diferida.
pub(in crate::app) fn saved_exit_ready(
    state: &mut editor::EditorState,
    deck: &deck::Deck,
    save: &mut SaveFlow,
) -> bool {
    if !save.save_all_queue.is_empty() {
        return false;
    }
    if state.blocks_leaving().is_some() || save_all_doc_count(deck, state) > 0 {
        // Traza del motivo: esto CANCELA la salida diferida y deja el aviso
        // visible en `save_error` (Task 4), para que Back/cierre nunca
        // termine en silencio.
        tracing::warn!(
            cola = save.save_all_queue.len(),
            bloqueo = ?state.blocks_leaving(),
            pendientes = save_all_doc_count(deck, state),
            "salida diferida rechazada: quedan cambios o ediciones pendientes"
        );
        save.close_after_save = false;
        save.after_save = None;
        save.allow_close = false;
        state.save_error = Some(
            "Changes or edits are still pending — finish editing and save again before leaving."
                .into(),
        );
        return false;
    }
    true
}

/// Cuántos documentos escribiría «Save all» (el activo, si está sucio,
/// más las ranuras de fondo sucias) — para el aviso de
/// poca RAM. Pura: se prueba con barajas construidas a mano.
fn save_all_doc_count(deck: &deck::Deck, state: &editor::EditorState) -> usize {
    if deck.unsaved_session {
        return session_dirty_ids(deck, state).len();
    }
    let active = state.is_dirty();
    let background = deck
        .slots
        .iter()
        .filter(|s| matches!(&s.content, deck::SlotContent::Ready(d) if d.history.is_dirty()))
        .count();
    background + usize::from(active)
}

/// Incluye las provisionales editadas; cada una pedirá su ruta al guardar.
fn session_dirty_ids(deck: &deck::Deck, state: &editor::EditorState) -> Vec<u64> {
    let mut ids: Vec<_> = deck
        .slots
        .iter()
        .filter_map(|slot| {
            let dirty = match &slot.content {
                deck::SlotContent::Active => state.is_dirty(),
                deck::SlotContent::Ready(doc) => doc.history.is_dirty(),
                _ => false,
            };
            dirty.then_some(slot.id)
        })
        .collect();
    let active = deck.slots.get(deck.active).map(|slot| slot.id);
    ids.sort_by_key(|id| Some(*id) != active);
    ids
}

/// ¿Conviene avisar de poca RAM antes de «Save all»? Pura para poder
/// probarla sin hardware ni variables de entorno: sin medición no se
/// avisa, y el umbral es `FREE_RAM_REDUCTION_THRESHOLD_BYTES` (el mismo
/// con el que la caché de la baraja empieza a reducir su presupuesto).
fn should_warn_low_memory(free_bytes: Option<u64>) -> bool {
    free_bytes.is_some_and(|free| free < crate::deck::FREE_RAM_REDUCTION_THRESHOLD_BYTES)
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
            // Traza: esta es la otra vía por la que aparece el banner ⚠ (el
            // PNG cambió respecto al hash del sidecar). Sin la traza, un
            // banner «sin causa» no se distingue del evento del watcher.
            if external_change {
                tracing::info!(
                    ruta = %path.display(),
                    "sidecar: el hash de la imagen no coincide; se marca cambio externo"
                );
            }
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
/// ¿Sigue la RAM en crítico DESPUÉS de liberar la memoria propia? Si la
/// medición marca crítico, primero se desecha la caché de la baraja —ranuras
/// limpias y lejanas, `evict_with_budget(0)` protege la activa, el destino de
/// un salto, los placeholders y el historial— y los scopes FX GPU que dejó
/// (`forget_scope`), y se re-mide. «La app no se bloquea a sí misma»: el
/// guardado/exportación solo aborta si SIGUE crítico tras soltar lo suyo.
/// `None` (no medible) nunca cuenta como crítico. Sin presión devuelve
/// `false` sin tocar nada.
fn critical_after_freeing_own_memory(
    deck: &mut crate::deck::Deck,
    renderer: &mut CanvasRenderer,
) -> bool {
    let critical = || is_critical_free_ram(free_ram_bytes());
    if !critical() {
        return false;
    }
    tracing::info!("RAM crítica: descartando caché propia de la baraja y scopes FX");
    // Soltar la caché ES la vía de escape cuando la presión la causa la propia app.
    let count = free_far_slot_scopes(deck, renderer);
    tracing::info!("liberados {count} scope(s); re-midiendo RAM libre");
    critical()
}

/// Expulsa las ranuras lejanas de la baraja y olvida sus scopes FX para dar
/// sitio en el atlas de vello antes de hornear (ver
/// plan-blurred-background-17.md). Presupuesto 0 = expulsar TODO lo que se
/// pueda expulsar con seguridad (la activa, `jump_to`, placeholders y ranuras
/// con historial quedan fuera, ver `Deck::evict_with_budget`).
fn free_far_slot_scopes(deck: &mut crate::deck::Deck, renderer: &mut CanvasRenderer) -> usize {
    let evicted = deck.evict_with_budget(0);
    let count = evicted.len();
    for scope in evicted {
        renderer.forget_scope(scope);
    }
    count
}

/// ¿Rechazar este destino raster ANTES de hornear? Un vídeo (o cualquier
/// extensión sin codificador) acabaría en `UnsupportedFormat` tras todo el
/// trabajo de GPU: se falla rápido con el mismo error, sin tocar el archivo
/// (el camino para un vídeo es Save As → póster PNG). Pura (testeable).
pub(super) fn raster_target_error(path: &std::path::Path) -> Option<canvas_io::IoError> {
    if canvas_io::save_format_from_path(path).is_none() {
        Some(canvas_io::IoError::UnsupportedFormat {
            path: path.to_owned(),
        })
    } else {
        None
    }
}

/// Resultado de INTENTAR lanzar un guardado. `Refused` = no se llegó a
/// escribir nada (RAM crítica, destino sin codificador, horneado rechazado);
/// el autoguardado lo usa para programar un reintento en vez de esperar a que
/// el usuario edite otra vez. `Started` = hay un worker escribiendo y el
/// resultado llegará como `AppMsg::Saved`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SaveStart {
    Started,
    Refused,
    /// Rechazo que no mejora reintentando (destino sin codificador): el
    /// autoguardado se rinde en vez de repetir el horneado cada pocos
    /// segundos. Los rechazos transitorios (RAM crítica, horneado inestable)
    /// siguen siendo `Refused`.
    RefusedPermanent,
}

pub(super) fn start_save(
    state: &mut editor::EditorState,
    sctx: &mut SaveContext,
    deck: &mut crate::deck::Deck,
    path: PathBuf,
    new_source: bool,
    jpeg_quality: u8,
) -> SaveStart {
    if state.saving {
        return SaveStart::Refused;
    }
    if let Some(error) = raster_target_error(&path) {
        state.save_error = Some(error.to_string());
        return SaveStart::RefusedPermanent;
    }
    if let Err(message) = prepare_bake(deck, sctx.renderer) {
        state.save_error = Some(message);
        return SaveStart::Refused;
    }
    let capas = state
        .doc
        .page()
        .map(|p| {
            p.layers
                .iter()
                .map(|l| l.name.clone())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    tracing::info!(ruta = %path.display(), capas = %capas, "start_save disparado");
    // Sin `forget_scope`: el bake reutiliza las texturas asentadas del
    // preview (su propio re-sync garantiza frescura); olvidar aquí re-subía
    // todo al atlas lleno y vello soltaba capas en silencio (17.png).
    let (rgba, width, height) = match bake_for_save(
        sctx.renderer,
        sctx.rs,
        sctx.scope,
        &state.doc,
        &state.images,
    ) {
        Ok(baked) => baked,
        Err(message) => {
            state.save_error = Some(message);
            return SaveStart::Refused;
        }
    };
    state.saving = true;
    state.save_error = None;
    // A04: el worker escribe una CAPTURA del documento; se sella la
    // revisión del historial para que `on_saved` solo marque como
    // guardado si no hubo ediciones durante la escritura.
    let capture = (state.history.undo_depth(), state.history.revision());
    state.saving_capture = Some(capture);
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
            ticket: loader::SaveTicket {
                target: loader::ImageInsertDest {
                    generation: deck.generation(),
                    slot_id: state.active_slot_id,
                },
                capture,
            },
        },
        sctx.tx.clone(),
        sctx.ctx.clone(),
    );
    SaveStart::Started
}

/// Rechazos ANTES de hornear que no miran el documento: RAM crítica (tras
/// soltar la propia caché, ver `critical_after_freeing_own_memory`) y sitio en
/// el atlas para el horneado. Devuelve el mensaje de usuario del rechazo. Muta
/// la baraja (expulsa ranuras lejanas y protege la activa). Compartido por el
/// guardado del lienzo activo y el de una ranura de fondo.
pub(super) fn prepare_bake(
    deck: &mut crate::deck::Deck,
    renderer: &mut CanvasRenderer,
) -> Result<(), String> {
    // RAM crítica: no se intenta el bake (podría salir incompleto o matar la
    // app). Falla rápido con el archivo intacto. Cierra también el hueco
    // temporal del aviso de «Save all»: aquí la medición es en el momento del
    // bake, no solo en el clic del menú.
    if critical_after_freeing_own_memory(deck, renderer) {
        tracing::error!("guardado abortado por RAM crítica; no se toca el archivo");
        return Err(
            "Not enough available memory to save safely — nothing was written. \
             Close other apps to free memory and try again."
                .into(),
        );
    }
    // Sitio en el atlas para el bake: expulsa ranuras lejanas (protege la activa).
    free_far_slot_scopes(deck, renderer);
    Ok(())
}

/// Hornea la página con las comprobaciones de seguridad del guardado: un
/// horneado en blanco/incompleto (17.png) o inestable se rechaza AQUÍ, así que
/// quien llama no escribe nada. No toca estado del lienzo ni disco: sirve igual
/// para el documento activo y para el de una ranura de fondo.
pub(super) fn bake_for_save(
    renderer: &mut CanvasRenderer,
    rs: &RenderState,
    scope: u64,
    doc: &Document,
    images: &canvas_render::ImageMap,
) -> Result<(Vec<u8>, u32, u32), String> {
    let scope = canvas_render::FxScope(scope);
    match renderer.bake_page_counting(&rs.device, &rs.queue, scope, doc, images, 1.0) {
        Ok((rgba, width, height, skipped)) => {
            if bake_came_out_blank_or_incomplete(doc, &rgba, skipped) {
                tracing::error!(
                    "horneado en blanco o incompleto ({} capa(s) omitida(s)); \
                     no se sobrescribe el archivo",
                    skipped
                );
                return Err(
                    "The save was refused because the image came out blank or incomplete — \
                     the file was not overwritten. Close other apps to free memory and try again."
                        .into(),
                );
            }
            if doc_needs_bake_verify(doc)
                && !rebake_matches_first(renderer, rs, scope, doc, images, 1.0, &rgba)
            {
                tracing::error!(
                    "horneado inestable (el segundo horneado difiere); \
                     no se sobrescribe el archivo"
                );
                return Err(
                    "The image rendered inconsistently — the file was not overwritten. \
                     Try saving again."
                        .into(),
                );
            }
            Ok((rgba, width, height))
        }
        Err(e) => {
            tracing::error!("horneado falló: {e}");
            Err(format!("Could not prepare the save: {e}"))
        }
    }
}

/// Guarda un diseño autónomo.
pub(super) fn start_save_design(
    state: &mut editor::EditorState,
    sctx: &mut SaveContext,
    path: PathBuf,
    new_source: bool,
) -> SaveStart {
    if state.saving {
        return SaveStart::Refused;
    }
    tracing::info!("guardando diseño en {}", path.display());
    let mut payload = state.sidecar_payload();
    let (pw, ph) = state
        .doc
        .page()
        .map(|p| (p.width, p.height))
        .unwrap_or((0.0, 0.0));
    let scale = canvas_io::preview_scale(pw, ph);
    // Igual que en `start_save`: hornear sobre las texturas asentadas.
    let scope = canvas_render::FxScope(sctx.scope);
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
    // A04: igual que `start_save` — el diseño se escribe en un worker a
    // partir de una captura; se sella la revisión para no marcar como
    // guardadas las ediciones que lleguen durante la escritura.
    state.saving_capture = Some((state.history.undo_depth(), state.history.revision()));
    *sctx.ignore_fs_events_until =
        Some(std::time::Instant::now() + std::time::Duration::from_secs(5));
    let reserve = state.canvas_name.is_some() && state.born_blank && !new_source;
    loader::spawn_save_design(
        loader::DesignSaveRequest {
            target: loader::ImageInsertDest {
                generation: sctx.scope,
                slot_id: state.active_slot_id,
            },
            capture: state.saving_capture.expect("captured above"),
            path,
            payload,
            reserve,
            new_source,
        },
        sctx.tx.clone(),
        sctx.ctx.clone(),
    );
    SaveStart::Started
}

/// Lo pedido en el diálogo de exportación ya resuelto: destino, ajustes y
/// scope de la ranura activa. Junto con el `&mut SaveContext` deja la firma
/// de `start_export` en 3 parámetros (estado, contexto, petición).
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
    sctx: &mut SaveContext,
    deck: &mut crate::deck::Deck,
    request: ExportRequest,
) {
    if state.exporting {
        return;
    }
    // RAM crítica: mismo criterio que el guardado — no se hornea ni se toca
    // el destino; falla rápido con el mensaje en el banner de `save_error`.
    // Primero se suelta la caché propia (ver `critical_after_freeing_own_memory`)
    //: solo aborta si sigue crítico tras liberar lo suyo.
    if critical_after_freeing_own_memory(deck, sctx.renderer) {
        tracing::error!("export abortado por RAM crítica; no se escribe nada");
        state.save_error = Some(
            "Not enough available memory to export safely — nothing was written. \
             Close other apps to free memory and try again."
                .into(),
        );
        return;
    }
    // Sitio en el atlas para el bake: expulsa ranuras lejanas (protege la activa).
    free_far_slot_scopes(deck, sctx.renderer);
    let ExportRequest {
        path,
        settings,
        scope,
    } = request;
    tracing::info!("exportando a {}", path.display());
    let scale = f64::from(settings.scale);
    let scope = canvas_render::FxScope(scope);
    // Igual que en `start_save`: hornear sobre las texturas asentadas.
    if settings.format.needs_bake() {
        match sctx.renderer.bake_page_counting(
            &sctx.rs.device,
            &sctx.rs.queue,
            scope,
            &state.doc,
            &state.images,
            scale,
        ) {
            Ok((rgba, width, height, skipped)) => {
                if bake_came_out_blank_or_incomplete(&state.doc, &rgba, skipped) {
                    tracing::error!(
                        "export en blanco o incompleto ({} capa(s) omitida(s)); no se escribe",
                        skipped
                    );
                    state.save_error = Some(
                        "The export was refused because it came out blank or incomplete — \
                         nothing was written. Close other apps to free memory and try again."
                            .into(),
                    );
                    return;
                }
                if doc_needs_bake_verify(&state.doc)
                    && !rebake_matches_first(
                        &mut *sctx.renderer,
                        sctx.rs,
                        scope,
                        &state.doc,
                        &state.images,
                        scale,
                        &rgba,
                    )
                {
                    tracing::error!(
                        "export inestable (el segundo horneado difiere); no se escribe"
                    );
                    state.save_error = Some(
                        "The export rendered inconsistently — nothing was written. \
                         Try exporting again."
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
                    sctx.tx.clone(),
                    sctx.ctx.clone(),
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
                sctx.renderer.sync_layer_effects(
                    &sctx.rs.device,
                    &sctx.rs.queue,
                    scope,
                    layer.id,
                    source,
                    &layer.effects,
                );
            }
        }
    }
    let blurred = sctx.renderer.blur_overrides(scope);
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
        sctx.tx.clone(),
        sctx.ctx.clone(),
    );
}

/// ¿El documento tiene al menos una capa que hornea píxeles reales
/// (imagen/video o SVG) VISIBLE? Mismo criterio que `bake_came_out_blank_or_incomplete`:
/// si NO hay ninguna, guardar el documento como raster aplanado solo escribe
/// texto/formas — y al sobrescribir un archivo de imagen en disco se descarta
/// la copia editable de la foto original que abrió el usuario. Se comprueba
/// ANTES de horneos para confirmar con el usuario (ver `discard_raster_modal_ui`).
pub(super) fn has_raster_layers(doc: &Document) -> bool {
    doc.page().is_ok_and(|page| {
        page.layers.iter().any(|layer| {
            layer.visible
                && matches!(
                    layer.content,
                    LayerContent::Image(_) | LayerContent::Video(_) | LayerContent::Svg(_)
                )
        })
    })
}

/// Lado mayor en px naturales que marca una capa como riesgo GPU: por debajo
/// ni dos capas llenan el atlas de 4096² medido en GPU real (mitad de
/// `MAX_FX_DIM`, ver plan-partial-bake-guard.md).
const GPU_VERIFY_MIN_SIDE: u32 = 1024;

/// ¿El documento necesita doble horneado de verificación? Sí cuando alguna
/// capa VISIBLE lleva efectos GPU (blur o ajustes de color) o es grande:
/// solo esas texturas puede soltar vello en silencio.
pub(super) fn doc_needs_bake_verify(doc: &Document) -> bool {
    doc.page().is_ok_and(|page| {
        page.layers.iter().any(|layer| {
            layer.visible
                && (layer.effects.blur_radius > 0.0
                    || layer.effects.has_color_adjustments()
                    || natural_long_side(&layer.content) > GPU_VERIFY_MIN_SIDE)
        })
    })
}

/// Lado mayor de las dimensiones naturales de una capa raster; 0 para
/// texto/formas/grupos (nunca van al atlas de imágenes).
fn natural_long_side(content: &LayerContent) -> u32 {
    let (w, h) = match content {
        LayerContent::Image(c) => (c.natural_width, c.natural_height),
        LayerContent::Video(c) => (c.natural_width, c.natural_height),
        LayerContent::Svg(c) => (c.natural_width, c.natural_height),
        _ => return 0,
    };
    w.max(h)
}

/// ¿Los dos horneados coinciden byte a byte? El render es determinista, así
/// que cualquier diferencia es inestabilidad real de la GPU, no redondeo.
pub(super) fn bakes_match(first: &[u8], second: &[u8]) -> bool {
    first == second
}

/// Segundo horneado de verificación con el mismo scope: `true` si coincide
/// byte a byte con el primero. Un error al re-hornear también es `false` —
/// si la GPU ya no repite el render, no hay motivo para confiar en el
/// primero. Quien llama rechaza sin escribir (ver
/// plan-partial-bake-guard.md).
fn rebake_matches_first(
    renderer: &mut CanvasRenderer,
    rs: &RenderState,
    scope: canvas_render::FxScope,
    doc: &Document,
    images: &canvas_render::ImageMap,
    scale: f64,
    first: &[u8],
) -> bool {
    match renderer.bake_page_counting(&rs.device, &rs.queue, scope, doc, images, scale) {
        Ok((second, _, _, _)) => bakes_match(first, &second),
        Err(e) => {
            tracing::error!("verificación sin segundo horneado ({e}); no se escribe");
            false
        }
    }
}

/// ¿El horneado salió UNIFORME (un solo color) pese a que el documento tiene
/// capas de imagen visibles que deberían pintar, o se OMITIÓ alguna capa de
/// imagen/SVG visible al construir la escena (`skipped > 0`: píxel ausente
/// del mapa o 0×0)? Ambos casos significan que el archivo que se escribiría
/// está corrupto en silencio (un bake fallido es un PNG blanco entero; una
/// capa omitida es contenido perdido aunque el resto pinte), así que se
/// rechaza antes de sobrescribir. Un diseño vectorial legítimamente
/// monocromo, o una foto realmente uniforme, no tienen capas pendientes ni
/// omitidas: siguen permitidos.
pub(super) fn bake_came_out_blank_or_incomplete(
    doc: &Document,
    rgba: &[u8],
    skipped: usize,
) -> bool {
    // Una capa omitida es un bake incompleto SIEMPRE, aunque el resultado no
    // sea uniforme (la única señal que distinguiría el chequeo de abajo).
    if skipped > 0 {
        return true;
    }
    let has_visible_images = doc.page().is_ok_and(|page| {
        page.layers.iter().any(|layer| {
            layer.visible
                && matches!(
                    layer.content,
                    LayerContent::Image(_) | LayerContent::Video(_) | LayerContent::Svg(_)
                )
        })
    });
    if !has_visible_images {
        return false;
    }
    let mut first: Option<[u8; 4]> = None;
    for px in rgba.as_chunks::<4>().0 {
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
