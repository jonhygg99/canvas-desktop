//! Respuestas de la galeria: el escaneo de una carpeta, sus miniaturas, y el
//! resultado de una operacion de archivos (crear, duplicar, pegar).

use std::path::PathBuf;

use eframe::egui;

use crate::{deck, loader};

use super::super::{App, Nav, View};

impl App {
    pub(super) fn on_gallery_scanned(
        &mut self,
        folder: PathBuf,
        files: Vec<(PathBuf, Option<std::time::SystemTime>)>,
        ctx: &egui::Context,
    ) {
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

    pub(super) fn on_gallery_thumb(
        &mut self,
        folder: PathBuf,
        path: PathBuf,
        result: Result<canvas_io::LoadedImage, String>,
        ctx: &egui::Context,
    ) {
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

    pub(super) fn on_gallery_op_done(
        &mut self,
        folder: PathBuf,
        created: Option<PathBuf>,
        result: Result<(), String>,
        open: bool,
        ctx: &egui::Context,
        open_after: &mut Option<Nav>,
    ) {
        // Lo que vamos a abrir lo acabamos de escribir nosotros:
        // ventana de gracia para que el watcher no cante «cambió
        // en disco» si el usuario ya estaba en el editor.
        self.ignore_fs_events_until =
            Some(std::time::Instant::now() + std::time::Duration::from_secs(2));
        match result {
            Ok(()) if open => {
                if let Some(path) = created {
                    // New design comes from the gallery. Carry
                    // every visible file into the editor deck,
                    // then activate the page just created.
                    if let View::Gallery(g) = &mut self.view {
                        let kind = if canvas_io::is_canvas_file(&path) {
                            crate::gallery::ItemKind::Design
                        } else {
                            crate::gallery::ItemKind::Image
                        };
                        let mut seed = deck::DeckSeed::from_gallery(g);
                        seed.push_path(path.clone(), kind);
                        self.deck_ops.pending_deck = Some(seed);
                    }
                    *open_after = Some(Nav::Open(path));
                }
            }
            Ok(()) => {
                // Solo rescanea si el usuario sigue en esa galería:
                // pudo haber navegado mientras corría la copia.
                if matches!(&self.view, View::Gallery(g) if g.folder == folder
                    || g.folder.parent() == Some(folder.as_path()))
                {
                    // El resultado de la operación queda
                    // seleccionado (borde azul): la copia recién
                    // duplicada/pegada, el archivo recién
                    // renombrado, o nada tras un borrado
                    // (`created` es `None`, limpia la marca).
                    if let View::Gallery(g) = &mut self.view {
                        // If we renamed the current folder itself,
                        // update g.folder to the new path.
                        if let Some(ref new_path) = created {
                            if new_path.is_dir()
                                && new_path != &g.folder
                                && new_path.parent() == g.folder.parent()
                            {
                                g.folder = new_path.clone();
                            }
                        }
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
}
