//! Respuestas de la búsqueda web (Serper): el resultado filtrado de cada
//! llamada (1 token = 1 página), cada miniatura (se sube a GPU en el hilo
//! de UI) y la imagen completa lista para insertarse como capa nueva. Los
//! thumbs son perezosos: el handler NO los pide, cada tarjeta pide el suyo
//! al pintarse.

use eframe::egui;

use super::super::{AppInner, Nav, View, Workspace};

/// Respuesta de UNA búsqueda web ya extraída del `AppMsg`: agrupa los cinco
/// campos que viajarían sueltos (convención del repo, nada de
/// `#[allow(too_many_arguments)]`).
pub(super) struct SerperSearchMsg {
    pub(super) seq: u64,
    pub(super) page: u32,
    pub(super) cache_key: String,
    pub(super) result: Result<crate::serper::SearchPage, crate::serper::SerperError>,
}

impl AppInner {
    pub(super) fn on_serper_search(
        &mut self,
        ws: &mut Workspace,
        msg: SerperSearchMsg,
        _ctx: &egui::Context,
    ) {
        let SerperSearchMsg {
            seq,
            page,
            cache_key,
            result,
        } = msg;
        let View::Editor(state) = &mut ws.view else {
            return;
        };
        let panel = &mut state.serper;
        // Respuesta caduca: entre medias se lanzó otra llamada, así que se
        // ignora (el `searching` lo gestiona la llamada nueva). El token ya
        // está gastado de todos modos: Serper cobra por llamada emitida.
        if seq != panel.search_seq {
            return;
        }
        panel.searching = false;
        panel.search_started = None;
        panel.page = page;
        match result {
            Ok(page_result) => {
                panel.apply_page(&page_result, page);
                // La caché no gasta: solo las llamadas de red suman gasto
                // real y entran en la caché de memoria.
                if !page_result.from_cache {
                    panel.credits_last = page_result.credits_charged;
                    panel.credits_session += u64::from(page_result.credits_charged);
                    self.settings.serper_credits_total += u64::from(page_result.credits_charged);
                    self.settings.save_in_background();
                    panel.cache_insert(cache_key, &page_result);
                } else {
                    panel.credits_last = 0;
                }
            }
            Err(e) => panel.error = Some(e.to_string()),
        }
    }

    pub(super) fn on_serper_thumb(
        &mut self,
        ws: &mut Workspace,
        id: String,
        result: Result<canvas_io::LoadedImage, crate::serper::SerperError>,
        ctx: &egui::Context,
    ) {
        let View::Editor(state) = &mut ws.view else {
            return;
        };
        let panel = &mut state.serper;
        let pos = panel.photos.iter().position(|p| p.photo.id == id);
        let Some(pos) = pos else {
            return;
        };
        match result {
            Ok(img) => {
                let color = egui::ColorImage::from_rgba_unmultiplied(
                    [img.width as usize, img.height as usize],
                    &img.rgba,
                );
                // El `id` es la URL: se hashea para el nombre de textura.
                let item = &mut panel.photos[pos];
                item.thumb = Some(ctx.load_texture(
                    format!("serper-thumb-{}", id_hash(&id)),
                    color,
                    egui::TextureOptions::LINEAR,
                ));
            }
            Err(crate::serper::SerperError::FilteredBanner) => {
                // Banner cazado con las dimensiones reales: la tarjeta se
                // retira en silencio y cuenta como filtrada.
                panel.photos.remove(pos);
                panel.bulk_selected.remove(&id);
                panel.filtered_post += 1;
            }
            Err(e) => {
                tracing::warn!("miniatura web {id} falló: {e}");
                panel.photos[pos].thumb_error = Some(e.to_string());
            }
        }
    }

    pub(super) fn on_serper_image_ready(
        &mut self,
        ws: &mut Workspace,
        id: String,
        label: String,
        result: Result<canvas_io::LoadedImage, crate::serper::SerperError>,
    ) {
        let View::Editor(state) = &mut ws.view else {
            return;
        };
        state.serper.inserting = None;
        match result {
            Ok(img) => {
                // Si llegó tras un ARRASTRE, cae en la posición de la
                // soltada; si no, centrada (clic).
                if let Some((drop_id, pos)) = state.serper.pending_drop.take() {
                    if drop_id == id {
                        state.add_image_layer_at(label, pos, img);
                    } else {
                        state.add_image_layer(label, None, img);
                    }
                } else {
                    state.add_image_layer(label, None, img);
                }
            }
            Err(e) => state.save_error = Some(format!("Web: {e}")),
        }
    }

    pub(super) fn on_serper_bulk_progress(
        &mut self,
        ws: &mut Workspace,
        done: usize,
        total: usize,
        _ctx: &egui::Context,
    ) {
        let View::Editor(state) = &mut ws.view else {
            return;
        };
        state.serper.bulk_progress = Some((done, total));
        state.serper.bulk_progress_at = Some(std::time::Instant::now());
    }

    pub(super) fn on_serper_bulk_done(
        &mut self,
        ws: &mut Workspace,
        folder: std::path::PathBuf,
        created: Vec<std::path::PathBuf>,
        errors: Vec<String>,
        ctx: &egui::Context,
        open_after: &mut Option<Nav>,
    ) {
        let View::Editor(state) = &mut ws.view else {
            return;
        };
        let panel = &mut state.serper;
        panel.bulk_busy = false;
        panel.bulk_progress = None;
        panel.bulk_progress_at = None;
        panel.bulk_errors = errors;
        let ok = created.len();
        // La carpeta usada se recuerda para la próxima (sigue por detrás de
        // baraja, archivo y galería en la resolución).
        self.settings.serper_last_folder = Some(folder.clone());
        self.settings.save_in_background();
        panel.bulk_done_msg = Some(if ok == 0 {
            "Bulk save finished with no new canvases.".to_owned()
        } else {
            let name = folder
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| folder.display().to_string());
            format!("Created {ok} canvases in {name}.")
        });
        // Al terminar con éxito se cierra el Select para ver los lienzos.
        if ok == 0 {
            return;
        }
        panel.bulk_open = false;
        if ws.deck.folder.as_deref() == Some(folder.as_path()) {
            // Misma baraja: salto al primer lienzo nuevo, resuelto tras el
            // reescaneo que lo incorpora (ver `on_gallery_scanned`).
            ws.deck_ops.bulk_jump = created.into_iter().next();
        } else {
            // Otra carpeta (p. ej. `New design` suelto con destino en
            // Imágenes): el reescaneo NO tocaría esta baraja, así que se
            // siembra el deck directamente con lo creado y se navega al
            // primero — mismo camino que abrir desde la galería.
            let Some(first) = created.first().cloned() else {
                return;
            };
            ws.deck_ops.pending_deck = Some(crate::deck::DeckSeed {
                folder: folder.clone(),
                sort: self.settings.gallery_sort,
                items: created
                    .into_iter()
                    .map(|path| crate::deck::SeedItem {
                        name: path
                            .file_name()
                            .map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or_default(),
                        path,
                        kind: crate::gallery::ItemKind::Image,
                        mtime: None,
                        thumb: None,
                        thumb_failed: false,
                    })
                    .collect(),
            });
            *open_after = Some(Nav::Open(first));
            return;
        }
        // Los archivos ya están en disco: la baraja los absorbe con un
        // reescaneo (igual que tras duplicar/pegar en la galería).
        ws.ignore_fs_events_until =
            Some(std::time::Instant::now() + std::time::Duration::from_secs(2));
        crate::loader::spawn_gallery_scan(
            folder,
            self.thumb_cache.clone(),
            ws.tx.clone(),
            ctx.clone(),
        );
    }
}

/// Hash estable y corto de una URL para nombrar su textura de egui.
fn id_hash(id: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    id.hash(&mut hasher);
    hasher.finish()
}
