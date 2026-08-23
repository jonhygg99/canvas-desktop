//! Bucle de mensajes: reacciona a todo lo que vuelve de los hilos de fondo
//! (`AppMsg` - cargas, guardados, exportaciones, miniaturas, sondeos de la
//! baraja, escaneos de la galeria...). Este archivo es solo el bucle y el
//! despacho: el cuerpo de cada respuesta vive en el submodulo de su dominio.

use eframe::egui;

use crate::loader;

use super::{App, Nav, View};
use loader::AppMsg;

mod document;
mod export;
mod gallery;
mod load;
mod save;
mod shell;

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
        // Aperturas diferidas para no pelear con el prestamo de self.view.
        let mut open_after: Option<Nav> = None;
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                AppMsg::FilePicked(Some(path)) | AppMsg::FolderPicked(Some(path)) => {
                    open_after = Some(Nav::Open(path));
                }
                AppMsg::FilePicked(None) | AppMsg::FolderPicked(None) => {}
                AppMsg::SaveAsPicked(path) => self.on_save_as_picked(path),
                AppMsg::Saved {
                    path,
                    result,
                    new_source,
                } => self.on_saved(path, result, new_source, ctx, &mut open_after),
                AppMsg::ExportPathPicked(path) => self.on_export_path_picked(path),
                AppMsg::Exported { path, result } => self.on_exported(path, result),
                AppMsg::ImageLoadedForLayer { path, result } => {
                    self.on_image_loaded_for_layer(path, result)
                }
                AppMsg::ImageLoadedForReplace {
                    layer,
                    label,
                    source_path,
                    result,
                } => self.on_image_loaded_for_replace(layer, label, source_path, result),
                AppMsg::GalleryScanned { folder, files } => {
                    self.on_gallery_scanned(folder, files, ctx)
                }
                AppMsg::GalleryThumb {
                    folder,
                    path,
                    result,
                } => self.on_gallery_thumb(folder, path, result, ctx),
                AppMsg::DeckProbed {
                    folder,
                    generation,
                    sizes,
                } => self.on_deck_probed(folder, generation, sizes),
                AppMsg::SlotPrepared {
                    folder,
                    generation,
                    path,
                    result,
                } => self.on_slot_prepared(folder, generation, path, result),
                AppMsg::CanvasPathReserved {
                    folder,
                    slot,
                    result,
                } => self.on_canvas_path_reserved(folder, slot, result),
                AppMsg::GalleryOpDone {
                    folder,
                    created,
                    result,
                    open,
                } => self.on_gallery_op_done(folder, created, result, open, ctx, &mut open_after),
                AppMsg::DocumentRenamed { old_path, result } => {
                    self.on_document_renamed(old_path, result)
                }
                AppMsg::DocumentDeleted { path, result } => {
                    self.on_document_deleted(path, result, &mut open_after)
                }
                AppMsg::DocumentRestored { path, result } => {
                    self.on_document_restored(path, result, ctx)
                }
                AppMsg::FocusWindow => self.on_focus_window(ctx),
                AppMsg::ShellIntegrationDone(result) => self.on_shell_integration_done(result),
                AppMsg::OpenPathExternal(path) => self.on_open_path_external(path, ctx),
                AppMsg::SourceChangedOnDisk { path } => self.on_source_changed_on_disk(path),
                AppMsg::ImageLoaded {
                    path,
                    result,
                    metadata,
                } => self.on_image_loaded(path, result, metadata, ctx),
            }
        }
        if let Some(nav) = open_after {
            self.navigate(nav, ctx);
        }
    }
}
