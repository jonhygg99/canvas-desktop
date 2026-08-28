//! Respuestas de la galeria: el escaneo de una carpeta, sus miniaturas, y el
//! resultado de una operacion de archivos (crear, duplicar, pegar).

use std::path::PathBuf;

use eframe::egui;

use crate::loader::GalleryOpOutcome;
use crate::{deck, loader};

use super::super::{AppInner, Nav, View, Workspace};

impl AppInner {
    pub(super) fn on_gallery_scanned(
        &mut self,
        ws: &mut Workspace,
        folder: PathBuf,
        files: Vec<(PathBuf, Option<std::time::SystemTime>)>,
        ctx: &egui::Context,
    ) {
        let want_deck = ws.deck.folder.as_deref() == Some(folder.as_path());
        let want_gallery = matches!(&ws.view, View::Gallery(g) if g.folder == folder);
        match (want_deck, want_gallery) {
            (true, true) => {
                ws.deck.merge_scan(files.clone());
                if let View::Gallery(g) = &mut ws.view {
                    g.merge_files(files);
                }
            }
            (true, false) => ws.deck.merge_scan(files),
            (false, true) => {
                if let View::Gallery(g) = &mut ws.view {
                    g.merge_files(files);
                }
            }
            (false, false) => {}
        }
        if want_deck {
            self.spawn_deck_probe(ctx, ws);
        }
    }

    pub(super) fn on_gallery_scan_failed(
        &mut self,
        ws: &mut Workspace,
        folder: PathBuf,
        error: String,
    ) {
        if let View::Gallery(g) = &mut ws.view {
            if g.folder == folder {
                g.set_scan_error(error);
            }
        }
    }

    pub(super) fn on_gallery_thumb(
        &mut self,
        ws: &mut Workspace,
        folder: PathBuf,
        path: PathBuf,
        result: Result<canvas_io::LoadedImage, canvas_io::IoError>,
        ctx: &egui::Context,
    ) {
        let want_deck = ws.deck.folder.as_deref() == Some(folder.as_path());
        let want_gallery = matches!(&ws.view, View::Gallery(g) if g.folder == folder);
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
                        ws.deck.set_thumb(&path, Some(tex.clone()));
                    }
                    if want_gallery {
                        if let View::Gallery(g) = &mut ws.view {
                            g.set_thumb(&path, Some(tex));
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("miniatura de {} falló: {e}", path.display());
                    if want_deck {
                        ws.deck.set_thumb(&path, None);
                    }
                    if want_gallery {
                        if let View::Gallery(g) = &mut ws.view {
                            g.set_thumb(&path, None);
                        }
                    }
                }
            }
        }
    }

    pub(super) fn on_gallery_op_done(
        &mut self,
        ws: &mut Workspace,
        op: GalleryOpOutcome,
        ctx: &egui::Context,
        open_after: &mut Option<Nav>,
    ) {
        let GalleryOpOutcome {
            folder,
            created,
            result,
            open,
        } = op;
        ws.ignore_fs_events_until =
            Some(std::time::Instant::now() + std::time::Duration::from_secs(2));
        match result {
            Ok(()) if open => {
                if let Some(path) = created {
                    if let View::Gallery(g) = &mut ws.view {
                        let kind = if canvas_io::is_canvas_file(&path) {
                            crate::gallery::ItemKind::Design
                        } else {
                            crate::gallery::ItemKind::Image
                        };
                        let mut seed = deck::DeckSeed::from_gallery(g);
                        seed.push_path(path.clone(), kind);
                        ws.deck_ops.pending_deck = Some(seed);
                    }
                    *open_after = Some(Nav::Open(path));
                }
            }
            Ok(()) => {
                if matches!(&ws.view, View::Gallery(g) if g.is_affected_by(&folder)) {
                    if let View::Gallery(g) = &mut ws.view {
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
                    self.rescan_gallery(ws, ctx);
                }
                if ws.deck.folder.as_deref() == Some(folder.as_path()) {
                    loader::spawn_gallery_scan(
                        folder.clone(),
                        self.thumb_cache.clone(),
                        ws.tx.clone(),
                        ctx.clone(),
                    );
                }
            }
            Err(e) => {
                tracing::warn!("operación de galería fallida: {e}");
                if let View::Gallery(g) = &mut ws.view {
                    if g.folder == folder {
                        g.op_error = Some(e.to_string());
                    }
                }
            }
        }
    }

    /// Llegó un reintento en segundo plano del listado de subcarpetas:
    /// aplica el resultado y libera el bucle (fuese exitoso o el último).
    pub(super) fn on_folders_refreshed(
        &mut self,
        ws: &mut Workspace,
        folder: PathBuf,
        children: Vec<PathBuf>,
        error: Option<String>,
    ) {
        if let View::Gallery(g) = &mut ws.view {
            if g.folder == folder {
                g.apply_folders_refresh(children, error);
            }
        }
    }
}
