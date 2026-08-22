//! Bucle de mensajes: reacciona a todo lo que vuelve de los hilos de fondo
//! (`AppMsg` — cargas, guardados, exportaciones, miniaturas, sondeos de la
//! baraja, escaneos de la galería…) y relanza el escaneo de la galería
//! activa cuando algo la invalida.

use std::path::PathBuf;

use eframe::egui;

use crate::{deck, editor, loader};

use super::{App, Nav, View};
use loader::AppMsg;

use super::persistence::build_slot_doc;

impl App {
    /// Relanza el escaneo de la carpeta actualmente abierta en la galería
    /// (tras crear/duplicar/pegar un archivo). `GalleryState::merge_files`
    /// conserva las miniaturas ya cargadas, así que esto es casi gratis.
    pub(super) fn rescan_gallery(&mut self, ctx: &egui::Context) {
        if let View::Gallery(g) = &self.view {
            loader::spawn_gallery_scan(
                g.folder.clone(),
                self.thumb_cache.clone(),
                self.tx.clone(),
                ctx.clone(),
            );
        }
    }

    pub(super) fn handle_messages(&mut self, ctx: &egui::Context) {
        // Aperturas diferidas para no pelear con el préstamo de self.view.
        let mut open_after: Option<Nav> = None;
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                AppMsg::FilePicked(Some(path)) | AppMsg::FolderPicked(Some(path)) => {
                    self.request_nav(Nav::Open(path), ctx);
                }
                AppMsg::FilePicked(None) | AppMsg::FolderPicked(None) => {}
                AppMsg::SaveAsPicked(path) => {
                    self.pending_save_as = path;
                }
                AppMsg::Saved {
                    path,
                    result,
                    new_source,
                } => {
                    if let View::Editor(state) = &mut self.view {
                        state.saving = false;
                        match result {
                            Ok(()) => {
                                tracing::info!("guardado OK: {}", path.display());
                                state.history.mark_saved();
                                // A partir de este guardado ya hay píxeles
                                // del usuario en disco: el próximo `Ctrl+S`
                                // vuelve a pedir confirmación si sobrescribe.
                                state.born_blank = false;
                                // Los eventos de disco inminentes son de este
                                // guardado: ventana de gracia y watcher nuevo
                                // (la sustitución atómica puede invalidarlo).
                                self.ignore_fs_events_until = Some(
                                    std::time::Instant::now() + std::time::Duration::from_secs(2),
                                );
                                self.watcher = None;
                                // Refresca la miniatura de la tira (y de la
                                // galería, si está abierta ahí) con el
                                // contenido recién guardado — sin esto, un
                                // diseño añadido o editado en esta misma
                                // sesión se queda con su miniatura en blanco
                                // hasta volver a abrir la carpeta, porque
                                // nada más dispara un rescan.
                                if let Some(folder) = path.parent() {
                                    loader::spawn_single_thumb(
                                        folder.to_path_buf(),
                                        path.clone(),
                                        self.thumb_cache.clone(),
                                        self.tx.clone(),
                                        ctx.clone(),
                                    );
                                }
                                if new_source {
                                    state.doc.source_path = Some(path);
                                }
                                // «Save all»: si lo que se acaba de guardar
                                // era el frente de la cola, avanza. Se
                                // comprueba por id de ranura, no por ruta:
                                // más robusto ante un renombrado en vuelo.
                                if self.save_all_queue.first().is_some_and(|&id| {
                                    self.deck.slots.get(self.deck.active).map(|s| s.id) == Some(id)
                                }) {
                                    self.save_all_queue.remove(0);
                                    self.save_all_attempted = false;
                                }
                                if self.close_after_save {
                                    self.allow_close = true;
                                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                                } else if let Some(nav) = self.after_save.take() {
                                    open_after = Some(nav);
                                }
                            }
                            Err(e) => {
                                self.close_after_save = false;
                                self.after_save = None;
                                // No hace falta esperar al frame siguiente
                                // para que el chequeo de la cola detecte el
                                // fallo: se aborta el lote aquí mismo si era
                                // su frente el que acaba de fallar.
                                if self.save_all_queue.first().is_some_and(|&id| {
                                    self.deck.slots.get(self.deck.active).map(|s| s.id) == Some(id)
                                }) {
                                    self.save_all_queue.clear();
                                    self.save_all_attempted = false;
                                }
                                state.save_error = Some(e);
                            }
                        }
                    }
                }
                AppMsg::ExportPathPicked(path) => {
                    if let (Some(path), Some(settings)) =
                        (path, self.pending_export_settings.take())
                    {
                        self.pending_export = Some((path, settings));
                    } else {
                        self.pending_export_settings = None;
                    }
                }
                AppMsg::Exported { path, result } => {
                    if let View::Editor(state) = &mut self.view {
                        state.exporting = false;
                        match result {
                            Ok(()) => tracing::info!("exportado OK: {}", path.display()),
                            Err(e) => {
                                state.save_error = Some(format!("Could not export: {e}"));
                            }
                        }
                    }
                }
                AppMsg::ImageLoadedForLayer { path, result } => {
                    if let View::Editor(state) = &mut self.view {
                        match result {
                            Ok(img) => {
                                let name = path
                                    .file_stem()
                                    .map(|s| s.to_string_lossy().into_owned())
                                    .unwrap_or_else(|| "Image".to_owned());
                                state.add_image_layer(name, Some(path), img);
                            }
                            Err(e) => {
                                state.save_error =
                                    Some(format!("Could not add \"{}\": {e}", path.display()));
                            }
                        }
                    }
                }
                AppMsg::ImageLoadedForReplace {
                    layer,
                    label,
                    source_path,
                    result,
                } => {
                    if let View::Editor(state) = &mut self.view {
                        match result {
                            Ok(img) => {
                                if let Err(e) = state.replace_image_layer(layer, source_path, img) {
                                    state.save_error =
                                        Some(format!("Could not replace image: {e}"));
                                }
                            }
                            Err(e) => {
                                state.save_error =
                                    Some(format!("Could not replace with {label}: {e}"));
                            }
                        }
                    }
                }
                AppMsg::GalleryScanned { folder, files } => {
                    // La baraja del editor (si es la misma carpeta) y la
                    // rejilla (si está abierta ahí) pueden querer el mismo
                    // reescaneo a la vez — típicamente al volver de un
                    // editor recién abierto desde esa galería.
                    let want_deck = self.deck.folder.as_deref() == Some(folder.as_path());
                    let want_gallery = matches!(&self.view, View::Gallery(g) if g.folder == folder);
                    match (want_deck, want_gallery) {
                        (true, true) => {
                            self.deck.merge_scan(files.clone());
                            if let View::Gallery(g) = &mut self.view {
                                g.merge_files(files);
                            }
                        }
                        (true, false) => self.deck.merge_scan(files),
                        (false, true) => {
                            if let View::Gallery(g) = &mut self.view {
                                g.merge_files(files);
                            }
                        }
                        (false, false) => {}
                    }
                    // Archivos nuevos en `merge_scan` nacen con `page: None`
                    // (`idle_slot`): sondearlos cubre el caso de añadir
                    // archivos a la carpeta mientras el editor ya está
                    // abierto en ella, no solo la apertura inicial.
                    if want_deck {
                        self.spawn_deck_probe(ctx);
                    }
                }
                AppMsg::GalleryThumb {
                    folder,
                    path,
                    result,
                } => {
                    // Igual que arriba: se sube la textura UNA vez y se
                    // reparte el handle (barato de clonar) a quien la quiera,
                    // para no duplicar la subida a GPU cuando ambas coinciden.
                    let want_deck = self.deck.folder.as_deref() == Some(folder.as_path());
                    let want_gallery = matches!(&self.view, View::Gallery(g) if g.folder == folder);
                    if want_deck || want_gallery {
                        match result {
                            Ok(img) => {
                                let color = egui::ColorImage::from_rgba_unmultiplied(
                                    [img.width as usize, img.height as usize],
                                    &img.rgba,
                                );
                                let tex = ctx.load_texture(
                                    path.to_string_lossy().into_owned(),
                                    color,
                                    egui::TextureOptions::LINEAR,
                                );
                                if want_deck {
                                    self.deck.set_thumb(&path, Some(tex.clone()));
                                }
                                if want_gallery {
                                    if let View::Gallery(g) = &mut self.view {
                                        g.set_thumb(&path, Some(tex));
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::warn!("miniatura de {} falló: {e}", path.display());
                                if want_deck {
                                    self.deck.set_thumb(&path, None);
                                }
                                if want_gallery {
                                    if let View::Gallery(g) = &mut self.view {
                                        g.set_thumb(&path, None);
                                    }
                                }
                            }
                        }
                    }
                }
                AppMsg::DeckProbed { folder, sizes } => {
                    if self.deck.folder.as_deref() == Some(folder.as_path()) {
                        self.deck.set_probes(sizes);
                    }
                }
                AppMsg::SlotLoaded {
                    folder,
                    path,
                    result,
                    metadata,
                } => {
                    // Guarda de obsolescencia: si la baraja ya no es esta
                    // carpeta (el usuario abrió otra cosa mientras cargaba),
                    // el mensaje se descarta entero — el `inflight` de la
                    // baraja NUEVA no tiene nada que ver con esta carga.
                    if self.deck.folder.as_deref() == Some(folder.as_path()) {
                        self.deck.loading_finished();
                        if let Some(idx) = self.deck.find_by_path(&path) {
                            // Si mientras tanto la ranura dejó de estar
                            // `Loading` (se activó por otra vía, o ya se
                            // descartó), no se pisa: esta carga ya no pinta
                            // nada.
                            let still_loading =
                                self.deck.slots.get(idx).is_some_and(|s| {
                                    matches!(s.content, deck::SlotContent::Loading)
                                });
                            if still_loading {
                                let metadata = (!metadata.is_empty()).then_some(metadata);
                                let new_content = match result {
                                    Ok(outcome) => build_slot_doc(
                                        path.clone(),
                                        outcome,
                                        metadata,
                                        self.settings.sidecar_default,
                                    )
                                    .map_or_else(
                                        || {
                                            deck::SlotContent::Failed(
                                                "could not build the document".to_owned(),
                                            )
                                        },
                                        |doc| deck::SlotContent::Ready(Box::new(doc)),
                                    ),
                                    Err(e) => {
                                        tracing::warn!(
                                            "carga de fondo de {} falló: {e}",
                                            path.display()
                                        );
                                        deck::SlotContent::Failed(e)
                                    }
                                };
                                if let Some(slot) = self.deck.slots.get_mut(idx) {
                                    slot.content = new_content;
                                }
                            }
                        }
                    }
                }
                AppMsg::CanvasPathReserved {
                    folder,
                    slot,
                    result,
                } => {
                    // Libera el cerrojo PRIMERO y siempre, para que un
                    // guardián de carpeta obsoleta (más abajo) no lo deje
                    // atascado.
                    if self.materializing == Some(slot) {
                        self.materializing = None;
                    }
                    // Guarda de obsolescencia, igual que `SlotLoaded`: si la
                    // baraja ya no es esta carpeta, el archivo reservado (0
                    // bytes) queda huérfano — se registra, no se limpia
                    // (borrarlo de fondo podría chocar con un usuario que
                    // reabrió justo esa carpeta).
                    if self.deck.folder.as_deref() != Some(folder.as_path()) {
                        tracing::warn!(
                            "baraja: reserva de nombre para «{}» llegó tras cambiar de carpeta; \
                             el archivo reservado queda huérfano",
                            folder.display()
                        );
                        continue;
                    }
                    match result {
                        Ok(path) => {
                            let Some(idx) = self.deck.find_by_id(slot) else {
                                tracing::warn!(
                                    "baraja: la ranura provisional ya no existe al reservar su nombre"
                                );
                                continue;
                            };
                            // Mismo patrón que `DocumentRenamed`: la tira lee
                            // ruta y nombre de la RANURA, no del documento.
                            if let Some(s) = self.deck.slots.get_mut(idx) {
                                s.name = path
                                    .file_name()
                                    .map(|n| n.to_string_lossy().into_owned())
                                    .unwrap_or_default();
                                s.path = path.clone();
                                s.is_placeholder = false;
                            }
                            if idx == self.deck.active {
                                if let View::Editor(state) = &mut self.view {
                                    // `state.is_design` refleja la extensión
                                    // REAL reservada (`settings.new_canvas_format`
                                    // en el momento de crear la ranura), no un
                                    // `true` fijo: la mayoría de lienzos nuevos
                                    // hoy son un raster, no un diseño autónomo.
                                    state.is_design = canvas_io::is_canvas_file(&path);
                                    state.doc.source_path = Some(path);
                                    // El bloque de guardado normal, más abajo
                                    // en este mismo frame, toma la rama de
                                    // diseño y llama a `start_save_design`
                                    // con horneado de miniatura, ventana de
                                    // gracia y `mark_saved()` de siempre —
                                    // gratis, sin duplicar nada de eso aquí.
                                    state.save_clicked = true;
                                }
                            } else if let deck::SlotContent::Ready(d) =
                                &mut self.deck.slots[idx].content
                            {
                                // El usuario saltó a otro lienzo mientras la
                                // reserva estaba en vuelo: se deja lista para
                                // guardarse la próxima vez (Ctrl+S al volver
                                // a ella, o Save All), sin forzarlo ahora.
                                d.doc.source_path = Some(path);
                            }
                            // Relleno automático: siempre queda una
                            // provisional lista al final, con o sin éxito
                            // arriba.
                            self.deck.push_placeholder(
                                self.settings.last_page_size,
                                self.settings.new_canvas_format.extension(),
                            );
                        }
                        Err(e) => {
                            self.materialize_blocked = Some(slot);
                            tracing::warn!("no se pudo crear el archivo del nuevo lienzo: {e}");
                            if let View::Editor(state) = &mut self.view {
                                state.save_error = Some(format!(
                                    "Could not create a file for the new canvas: {e}"
                                ));
                            }
                        }
                    }
                }
                AppMsg::GalleryOpDone {
                    folder,
                    created,
                    result,
                    open,
                } => {
                    // Lo que vamos a abrir lo acabamos de escribir nosotros:
                    // ventana de gracia para que el watcher no cante «cambió
                    // en disco» si el usuario ya estaba en el editor.
                    self.ignore_fs_events_until =
                        Some(std::time::Instant::now() + std::time::Duration::from_secs(2));
                    match result {
                        Ok(()) if open => {
                            if let Some(path) = created {
                                open_after = Some(Nav::Open(path));
                            }
                        }
                        Ok(()) => {
                            // Solo rescanea si el usuario sigue en esa galería:
                            // pudo haber navegado mientras corría la copia.
                            if matches!(&self.view, View::Gallery(g) if g.folder == folder
                                    || g.folder.parent() == Some(folder.as_path())) {
                                // El resultado de la operación queda
                                // seleccionado (borde azul): la copia recién
                                // duplicada/pegada, el archivo recién
                                // renombrado, o nada tras un borrado
                                // (`created` es `None`, limpia la marca).
                                if let View::Gallery(g) = &mut self.view {
                                    g.selected = created.clone();
                                    g.refresh_folder_lists();
                                }
                                self.rescan_gallery(ctx);
                            }
                            // Igual, pero para la baraja del editor (p.ej. el
                            // botón «⧉» de la cabecera de un lienzo, que
                            // dispara esta misma operación aunque la vista
                            // actual sea el editor, no la galería) — la
                            // reconciliación (`merge_scan`, incluido
                            // `order_hint`) llega sola al recibir
                            // `GalleryScanned`, aquí solo hace falta pedirla.
                            if self.deck.folder.as_deref() == Some(folder.as_path()) {
                                loader::spawn_gallery_scan(
                                    folder.clone(),
                                    self.thumb_cache.clone(),
                                    self.tx.clone(),
                                    ctx.clone(),
                                );
                            }
                        }
                        Err(e) => {
                            // No hay nada destructivo que deshacer (la copia
                            // fallida ya se revirtió en el hilo de trabajo).
                            // Se registra y, si el usuario sigue en esa
                            // galería, también se le muestra: antes solo
                            // quedaba en el log, invisible en la UI.
                            tracing::warn!("operación de galería fallida: {e}");
                            if let View::Gallery(g) = &mut self.view {
                                if g.folder == folder {
                                    g.op_error = Some(e);
                                }
                            }
                        }
                    }
                }
                AppMsg::DocumentRenamed { old_path, result } => {
                    let is_active = matches!(&self.view, View::Editor(state)
                        if state.doc.source_path.as_deref() == Some(old_path.as_path()));
                    if is_active {
                        if let View::Editor(state) = &mut self.view {
                            match result {
                                Ok(new_path) => {
                                    // La ranura activa de la baraja lleva su
                                    // propia copia de la ruta/nombre (la
                                    // tira los lee de ahí, no del documento):
                                    // sin esto, renombrar dejaría la tira con
                                    // el nombre viejo hasta el próximo
                                    // reescaneo.
                                    if let Some(slot) = self.deck.slots.get_mut(self.deck.active) {
                                        slot.path = new_path.clone();
                                        slot.name = new_path
                                            .file_name()
                                            .map(|n| n.to_string_lossy().into_owned())
                                            .unwrap_or_default();
                                    }
                                    state.doc.source_path = Some(new_path);
                                }
                                // Reutiliza el banner de error que ya existe
                                // en el panel: no hace falta un campo nuevo.
                                Err(e) => state.save_error = Some(e),
                            }
                        }
                    } else {
                        // Ranura de FONDO (cabecera del lienzo en el área
                        // central, no la activa): sin `state.doc` que
                        // actualizar, solo la propia ranura de la baraja —
                        // mismo campo que arriba, generalizado por ruta en
                        // vez de "la activa". Sin banner de error propio
                        // para una ranura que no se está mirando: se
                        // registra y ya.
                        match result {
                            Ok(new_path) => {
                                if let Some(slot) =
                                    self.deck.slots.iter_mut().find(|s| s.path == old_path)
                                {
                                    slot.path = new_path.clone();
                                    slot.name = new_path
                                        .file_name()
                                        .map(|n| n.to_string_lossy().into_owned())
                                        .unwrap_or_default();
                                }
                            }
                            Err(e) => tracing::warn!(
                                "no se pudo renombrar {} en segundo plano: {e}",
                                old_path.display()
                            ),
                        }
                    }
                }
                AppMsg::DocumentDeleted { path, result } => {
                    // `state` toma prestado `self.view`; no se puede
                    // reasignar `self.view` mientras siga vivo, así que la
                    // decisión se guarda en una variable local y se aplica
                    // después de que el préstamo termine.
                    let mut go_to_welcome = false;
                    // `remove` (no `get`): de un solo uso — `Some(sidecar)`
                    // si este borrado lo pidió el usuario directamente (no
                    // como consecuencia de deshacer un `Create`), con el
                    // sidecar que tenía (si tenía uno) anotado ANTES de
                    // borrar. Se apila como `GlobalStep::Delete` más abajo,
                    // solo si el borrado tuvo éxito.
                    let undoable_delete = self.undoable_deletes.remove(&path);
                    if result.is_ok() {
                        if let (Some(sidecar), View::Editor(state)) =
                            (undoable_delete, &mut self.view)
                        {
                            state.record_delete(editor::DeleteRecord {
                                path: path.clone(),
                                sidecar,
                            });
                        }
                    }
                    let is_active = matches!(&self.view, View::Editor(state)
                        if state.doc.source_path.as_deref() == Some(path.as_path()));
                    if is_active {
                        if let View::Editor(state) = &mut self.view {
                            match result {
                                Ok(()) => {
                                    // El archivo ya no existe: no tiene
                                    // sentido preguntar por cambios sin
                                    // guardar (no hay dónde guardarlos). Si
                                    // la baraja tiene más ranuras y la
                                    // vecina ya está cargada, se salta a
                                    // ella en vez de salir del editor entero
                                    // — el archivo desapareció, pero el
                                    // resto de la carpeta sigue teniendo
                                    // sentido en pantalla.
                                    let mut jumped = false;
                                    if self.deck.slots.len() > 1 {
                                        let removed = self.deck.active;
                                        self.deck.slots.remove(removed);
                                        // Sin esto los supervivientes se
                                        // quedan con el `rect` viejo
                                        // (calculado con la borrada
                                        // todavía en la pila) hasta el
                                        // próximo cambio que sí encienda
                                        // el flag — se ve como un hueco
                                        // vacío que nadie ocupa.
                                        self.deck.layout_dirty = true;
                                        let neighbor =
                                            removed.min(self.deck.slots.len().saturating_sub(1));
                                        if let Some(slot) = self.deck.slots.get_mut(neighbor) {
                                            if matches!(slot.content, deck::SlotContent::Ready(_)) {
                                                let deck::SlotContent::Ready(incoming) =
                                                    std::mem::replace(
                                                        &mut slot.content,
                                                        deck::SlotContent::Active,
                                                    )
                                                else {
                                                    unreachable!("comprobado justo arriba");
                                                };
                                                state.put_slot(*incoming);
                                                self.deck.active = neighbor;
                                                jumped = true;
                                            }
                                        }
                                    }
                                    if !jumped {
                                        match state.from_gallery.clone() {
                                            Some(folder) => open_after = Some(Nav::Open(folder)),
                                            None => go_to_welcome = true,
                                        }
                                    }
                                }
                                Err(e) => state.save_error = Some(e),
                            }
                        }
                    } else {
                        // Ranura de FONDO (cabecera del lienzo en el área
                        // central, no la activa): borrar generaliza el mismo
                        // bloque de arriba que YA quita la ranura activa de
                        // `self.deck.slots` — sin salto ni pantalla de
                        // bienvenida, porque el usuario no estaba mirando
                        // este lienzo.
                        match result {
                            Ok(()) => {
                                if let Some(idx) =
                                    self.deck.slots.iter().position(|s| s.path == path)
                                {
                                    self.deck.slots.remove(idx);
                                    // Si la borrada estaba ANTES de la
                                    // activa en el `Vec`, todo lo posterior
                                    // se desplaza un puesto — sin este
                                    // ajuste `deck.active` (un índice, no un
                                    // id) pasaría a apuntar a la ranura
                                    // equivocada, y la que de verdad sigue
                                    // activa dejaría de encajar en ninguna
                                    // rama del render (ni "es la activa" ni
                                    // "tiene contenido `Ready`", porque su
                                    // contenido es el marcador `Active`) —
                                    // su cuerpo desaparecía aunque la
                                    // cabecera se siguiera pintando.
                                    if idx < self.deck.active {
                                        self.deck.active -= 1;
                                    }
                                    self.deck.layout_dirty = true;
                                }
                            }
                            Err(e) => tracing::warn!(
                                "no se pudo borrar {} en segundo plano: {e}",
                                path.display()
                            ),
                        }
                    }
                    if go_to_welcome {
                        self.view = View::Welcome { error: None };
                    }
                }
                AppMsg::DocumentRestored { path, result } => match result {
                    Ok(()) => {
                        // Igual que tras una `GalleryOp` (`GalleryOpDone`,
                        // arriba): si la carpeta activa (baraja o galería)
                        // es la del archivo restaurado, se rescanea para que
                        // reaparezca como ranura/miniatura — no hace falta
                        // reconstruir un `Slot` a mano.
                        self.ignore_fs_events_until =
                            Some(std::time::Instant::now() + std::time::Duration::from_secs(2));
                        if let Some(folder) = path.parent().map(PathBuf::from) {
                            if self.deck.folder.as_deref() == Some(folder.as_path()) {
                                loader::spawn_gallery_scan(
                                    folder.clone(),
                                    self.thumb_cache.clone(),
                                    self.tx.clone(),
                                    ctx.clone(),
                                );
                            }
                            if matches!(&self.view, View::Gallery(g) if g.folder == folder
                                    || g.folder.parent() == Some(folder.as_path())) {
                                self.rescan_gallery(ctx);
                                if let View::Gallery(g) = &mut self.view {
                                    g.refresh_folder_lists();
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("no se pudo restaurar «{}»: {e}", path.display());
                        if let View::Editor(state) = &mut self.view {
                            state.save_error =
                                Some(format!("Could not restore \"{}\": {e}", path.display()));
                        }
                    }
                },
                AppMsg::FocusWindow => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                AppMsg::ShellIntegrationDone(result) => {
                    self.shell_status = match result {
                        Ok(msg) => msg,
                        Err(e) => format!("Failed: {e}"),
                    };
                }
                AppMsg::OpenPathExternal(path) => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    // Pregunta si hay un editor con cambios sin guardar.
                    self.request_nav(Nav::Open(path), ctx);
                }
                AppMsg::SourceChangedOnDisk { path } => {
                    let own_save = self
                        .ignore_fs_events_until
                        .is_some_and(|t| std::time::Instant::now() < t);
                    if !own_save {
                        if let View::Editor(state) = &mut self.view {
                            if state.doc.source_path.as_deref() == Some(path.as_path()) {
                                state.external_change = true;
                            }
                        }
                    }
                }
                AppMsg::ImageLoaded {
                    path,
                    result,
                    metadata,
                } => {
                    // Ignora cargas que ya no corresponden a la vista actual.
                    let expected = matches!(&self.view, View::Loading { path: p } if *p == path);
                    if !expected {
                        continue;
                    }
                    let metadata = (!metadata.is_empty()).then_some(metadata);
                    match result {
                        Ok(loader::LoadOutcome::Restored(restored)) => {
                            // Si la imagen cambió por fuera desde el último
                            // guardado con capas, avisa y deja elegir.
                            let use_layers = restored.hash_matches
                                || {
                                    let choice = rfd::MessageDialog::new()
                                    .set_level(rfd::MessageLevel::Warning)
                                    .set_title("Image changed outside Canvas Desktop")
                                    .set_description(format!(
                                        "\"{}\" was modified by another program after the last save with layers.\nRestore the editable layers anyway? (\"No\" opens the image as it is now.)",
                                        path.file_name().map(|s| s.to_string_lossy()).unwrap_or_default()
                                    ))
                                    .set_buttons(rfd::MessageButtons::YesNo)
                                    .show();
                                    matches!(choice, rfd::MessageDialogResult::Yes)
                                };
                            if use_layers {
                                self.resolve_deck(&path, ctx);
                                let mut state =
                                    editor::EditorState::from_restored(path.clone(), restored);
                                state.from_gallery = self.deck.folder.clone();
                                state.sidecar_enabled = self.settings.sidecar_default;
                                state.source_metadata = metadata;
                                self.remember_page_size(&state.doc);
                                self.view = View::Editor(Box::new(state));
                            } else {
                                // Recarga plana, ignorando el sidecar.
                                loader::spawn_load_image(
                                    path.clone(),
                                    false,
                                    self.tx.clone(),
                                    ctx.clone(),
                                );
                                self.view = View::Loading { path: path.clone() };
                            }
                        }
                        Ok(loader::LoadOutcome::Design(restored)) => {
                            // Diseño autónomo: `hash_matches` siempre es
                            // `true` (no hay nada que contrastar), así que no
                            // hace falta el diálogo de «cambió por fuera».
                            self.resolve_deck(&path, ctx);
                            let mut state =
                                editor::EditorState::from_design(path.clone(), restored);
                            state.from_gallery = self.deck.folder.clone();
                            self.remember_page_size(&state.doc);
                            self.view = View::Editor(Box::new(state));
                        }
                        Ok(loader::LoadOutcome::Flat(img)) => {
                            match editor::EditorState::from_image(path.clone(), img) {
                                Ok(mut state) => {
                                    self.resolve_deck(&path, ctx);
                                    state.from_gallery = self.deck.folder.clone();
                                    state.sidecar_enabled = self.settings.sidecar_default;
                                    state.source_metadata = metadata;
                                    self.remember_page_size(&state.doc);
                                    self.view = View::Editor(Box::new(state));
                                }
                                Err(e) => {
                                    self.view = View::Welcome {
                                        error: Some(format!(
                                            "Could not open \"{}\": {e}",
                                            path.display()
                                        )),
                                    };
                                }
                            }
                        }
                        Err(e) => {
                            self.view = View::Welcome {
                                error: Some(format!("Could not open \"{}\": {e}", path.display())),
                            };
                        }
                    }
                    self.sync_title(ctx);
                }
            }
        }
        if let Some(nav) = open_after {
            self.navigate(nav, ctx);
        }
    }
}
