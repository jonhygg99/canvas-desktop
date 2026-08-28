//! Respuestas de la búsqueda de Unsplash: los resultados de la API, cada
//! miniatura (se sube a GPU en el hilo de UI, como las de la galería) y la
//! imagen completa lista para insertarse como capa nueva.

use std::collections::HashSet;

use eframe::egui;

use crate::loader;
use crate::unsplash::PhotoItem;

use super::super::{AppInner, View, Workspace};

impl AppInner {
    pub(super) fn on_unsplash_search(
        &mut self,
        ws: &mut Workspace,
        _query: String,
        seq: u64,
        page: u32,
        result: Result<crate::unsplash::SearchPage, crate::unsplash::UnsplashError>,
        ctx: &egui::Context,
    ) {
        let View::Editor(state) = &mut ws.view else {
            return;
        };
        let panel = &mut state.unsplash;
        // Respuesta caduca: entre medias se lanzó otra búsqueda (nuevo
        // filtro/consulta), así que se ignora; el `searching` lo gestiona la
        // búsqueda nueva.
        if seq != panel.search_seq {
            return;
        }
        panel.searching = false;
        panel.page = page;
        match result {
            Ok(page_result) => {
                // Solo la primera página reinicia; «Load more» añade.
                if page == 1 {
                    panel.photos.clear();
                }
                let existing: HashSet<String> =
                    panel.photos.iter().map(|p| p.photo.id.clone()).collect();
                for photo in page_result.photos {
                    if existing.contains(&photo.id) {
                        continue;
                    }
                    let id = photo.id.clone();
                    let thumb_url = photo.urls.small.clone();
                    loader::spawn_unsplash_thumb(id, thumb_url, ws.tx.clone(), ctx.clone());
                    panel.photos.push(PhotoItem {
                        photo,
                        thumb: None,
                        thumb_failed: false,
                    });
                }
                panel.error = None;
                panel.reached_end = page_result.reached_end;
                if panel.photos.is_empty() {
                    panel.error = Some("No results for that query".to_owned());
                }
            }
            Err(e) => panel.error = Some(e.to_string()),
        }
    }

    pub(super) fn on_unsplash_thumb(
        &mut self,
        ws: &mut Workspace,
        id: String,
        result: Result<canvas_io::LoadedImage, crate::unsplash::UnsplashError>,
        ctx: &egui::Context,
    ) {
        let View::Editor(state) = &mut ws.view else {
            return;
        };
        let Some(item) = state.unsplash.photos.iter_mut().find(|p| p.photo.id == id) else {
            return;
        };
        match result {
            Ok(img) => {
                let color = egui::ColorImage::from_rgba_unmultiplied(
                    [img.width as usize, img.height as usize],
                    &img.rgba,
                );
                item.thumb = Some(ctx.load_texture(
                    format!("unsplash-thumb-{id}"),
                    color,
                    egui::TextureOptions::LINEAR,
                ));
            }
            Err(e) => {
                tracing::warn!("miniatura de Unsplash {id} falló: {e}");
                item.thumb_failed = true;
            }
        }
    }

    pub(super) fn on_unsplash_image_ready(
        &mut self,
        ws: &mut Workspace,
        id: String,
        label: String,
        result: Result<canvas_io::LoadedImage, crate::unsplash::UnsplashError>,
    ) {
        let View::Editor(state) = &mut ws.view else {
            return;
        };
        state.unsplash.inserting = None;
        match result {
            Ok(img) => {
                // Si la foto llegó tras un ARRASTRE soltado sobre el lienzo,
                // cae en la posición de la soltada; si no, centrada (clic).
                if let Some((drop_id, pos)) = state.unsplash.pending_drop.take() {
                    if drop_id == id {
                        state.add_image_layer_at(label, pos, img);
                    } else {
                        state.add_image_layer(label, None, img);
                    }
                } else {
                    state.add_image_layer(label, None, img);
                }
            }
            Err(e) => state.save_error = Some(format!("Unsplash: {e}")),
        }
    }
}
