//! Bucle de mensajes: reacciona a todo lo que vuelve de los hilos de fondo
//! (`AppMsg` - cargas, guardados, exportaciones, miniaturas, sondeos de la
//! baraja, escaneos de la galeria...). Este archivo es solo el bucle y el
//! despacho: el cuerpo de cada respuesta vive en el submodulo de su dominio.
//!
//! Cada workspace tiene su propio canal (`Workspace::tx`/`rx`): los hilos a
//! los que pide trabajo responden por ESE canal, así que el mensaje llega ya
//! direccionado y no hace falta un campo `workspace_id` en cada variante ni
//! enrutar por contenido. Un mensaje cuyo workspace ya se cerró se descarta
//! con un log (el guardado/operación ya escribió en disco; solo se pierde la
//! notificación de la UI).

use eframe::egui;

use crate::loader;

use super::{AppInner, Nav, View, Workspace};
use loader::AppMsg;

mod document;
mod export;
mod gallery;
mod load;
mod save;
mod shell;

impl AppInner {
    /// Relanza el escaneo de la carpeta actualmente abierta en la galería de
    /// `ws` (tras crear/duplicar/pegar un archivo). `GalleryState::merge_files`
    /// conserva las miniaturas ya cargadas, así que esto es casi gratis.
    pub(super) fn rescan_gallery(&mut self, ws: &mut Workspace, ctx: &egui::Context) {
        if let View::Gallery(g) = &ws.view {
            loader::spawn_gallery_scan(
                g.folder.clone(),
                self.thumb_cache.clone(),
                ws.tx.clone(),
                ctx.clone(),
            );
        }
    }

    /// Drena el canal GLOBAL (eventos de shell) y el canal de cada
    /// workspace. Se llama al principio del frame raíz; los mensajes
    /// dirigidos al workspace enfocado se aplican además dentro del frame de
    /// esa ventana (las hijas repintan solas y no esperan al raíz).
    pub(crate) fn drain_all(&mut self, ctx: &egui::Context) {
        let mut open_after: Option<Nav> = None;
        self.drain_shell(ctx, &mut open_after);
        // Mensajes de shell que producen navegación van al workspace enfocado.
        if let Some(nav) = open_after {
            let idx = self.focused.min(self.workspaces.len().saturating_sub(1));
            if let Some(ws_arc) = self.workspaces.get(idx).cloned() {
                let mut ws = ws_arc.lock().unwrap();
                self.navigate(&mut ws, nav, ctx);
            }
        }

        // Drena los canales de TODAS las ventanas (CLONADOS primero: no se
        // puede prestar `self.workspaces` y llamar `self.navigate` a la vez).
        let all = self.workspaces.clone();
        for ws_arc in all {
            let mut ws = ws_arc.lock().unwrap();
            let mut open_after = None;
            self.drain_ws(&mut ws, ctx, &mut open_after);
            if let Some(nav) = open_after {
                self.navigate(&mut ws, nav, ctx);
            }
        }
    }

    /// Drena UN canal de workspace (usado también por las ventanas hijas en
    /// su propio frame).
    pub(crate) fn drain_ws(
        &mut self,
        ws: &mut Workspace,
        ctx: &egui::Context,
        open_after: &mut Option<Nav>,
    ) {
        while let Ok(msg) = ws.rx.try_recv() {
            self.dispatch(ws, ctx, open_after, msg);
        }
    }

    fn drain_shell(&mut self, ctx: &egui::Context, _open_after: &mut Option<Nav>) {
        while let Ok(msg) = self.shell_rx.try_recv() {
            self.dispatch_shell(ctx, msg);
        }
    }

    /// Despacha un mensaje dirigido a un workspace concreto.
    fn dispatch(
        &mut self,
        ws: &mut Workspace,
        ctx: &egui::Context,
        open_after: &mut Option<Nav>,
        msg: AppMsg,
    ) {
        match msg {
            AppMsg::FilePicked(Some(path)) | AppMsg::FolderPicked(Some(path)) => {
                *open_after = Some(Nav::Open(path));
            }
            AppMsg::FilePicked(None) | AppMsg::FolderPicked(None) => {}
            AppMsg::SaveAsPicked(path) => ws.save.pending_save_as = path,
            AppMsg::Saved {
                path,
                result,
                new_source,
            } => self.on_saved(ws, path, result, new_source, ctx, open_after),
            AppMsg::UnsavedDialogAnswer(decision) => {
                self.on_unsaved_dialog_answer(ws, decision, open_after, ctx)
            }
            AppMsg::ExportPathPicked(path) => self.on_export_path_picked(ws, path),
            AppMsg::Exported { path, result } => self.on_exported(ws, path, result),
            AppMsg::ImageLoadedForLayer { path, result } => {
                self.on_image_loaded_for_layer(ws, path, result)
            }
            AppMsg::ImageLoadedForReplace {
                layer,
                label,
                source_path,
                result,
            } => self.on_image_loaded_for_replace(ws, layer, label, source_path, result),
            AppMsg::GalleryScanned { folder, files } => {
                self.on_gallery_scanned(ws, folder, files, ctx)
            }
            AppMsg::GalleryScanFailed { folder, error } => {
                self.on_gallery_scan_failed(ws, folder, error)
            }
            AppMsg::FoldersRefreshed {
                folder,
                children,
                error,
            } => self.on_folders_refreshed(ws, folder, children, error),
            AppMsg::GalleryThumb {
                folder,
                path,
                result,
            } => self.on_gallery_thumb(ws, folder, path, result, ctx),
            AppMsg::DeckProbed {
                folder,
                generation,
                sizes,
            } => self.on_deck_probed(ws, folder, generation, sizes),
            AppMsg::SlotPrepared {
                folder,
                generation,
                path,
                result,
            } => self.on_slot_prepared(ws, folder, generation, path, result),
            AppMsg::CanvasPathReserved {
                folder,
                slot,
                result,
            } => self.on_canvas_path_reserved(ws, folder, slot, result),
            AppMsg::GalleryOpDone {
                folder,
                created,
                result,
                open,
            } => self.on_gallery_op_done(ws, folder, created, result, open, ctx, open_after),
            AppMsg::DocumentRenamed { old_path, result } => {
                self.on_document_renamed(ws, old_path, result)
            }
            AppMsg::DocumentDeleted { path, result } => {
                self.on_document_deleted(ws, path, result, open_after)
            }
            AppMsg::DocumentRestored { path, result } => {
                self.on_document_restored(ws, path, result, ctx)
            }
            AppMsg::FocusWindow => self.on_focus_window(ctx),
            AppMsg::ShellIntegrationDone(result) => self.on_shell_integration_done(result),
            AppMsg::OpenPathExternal(path) => self.on_open_path_external(ctx, path),
            AppMsg::SourceChangedOnDisk { path } => self.on_source_changed_on_disk(ws, path),
            AppMsg::ImageLoaded {
                path,
                result,
                metadata,
            } => self.on_image_loaded(ws, path, result, metadata, ctx),
        }
    }

    /// Despacha un mensaje del canal global (shell).
    fn dispatch_shell(&mut self, ctx: &egui::Context, msg: AppMsg) {
        match msg {
            AppMsg::FocusWindow => self.on_focus_window(ctx),
            AppMsg::ShellIntegrationDone(result) => self.on_shell_integration_done(result),
            AppMsg::OpenPathExternal(path) => self.on_open_path_external(ctx, path),
            _ => {}
        }
    }
}
