//! Vista de bienvenida: accesos rápidos, recientes y la salida de una
//! apertura fallida (reparar / cuarentena).

use std::sync::mpsc::Sender;

use eframe::egui;

use crate::loader::AppMsg;
use crate::{loader, welcome};

use super::super::{Nav, View};

/// Vista de bienvenida: accesos rápidos más la salida de una apertura
/// fallida (reparar / cuarentena) cuando `View::Welcome` trae archivo.
pub(in crate::app) fn welcome_view_ui(
    ui: &mut egui::Ui,
    view: &mut View,
    recent_files: &mut Vec<std::path::PathBuf>,
    pinned_folders: &mut Vec<std::path::PathBuf>,
    show_settings: &mut bool,
    tx: &Sender<AppMsg>,
    ctx: &egui::Context,
) -> Option<Nav> {
    let mut open_next = None;
    let (error, failed) = match view {
        View::Welcome {
            error,
            failed_path,
            failed_kind,
        } => (error.clone(), failed_path.clone().zip(*failed_kind)),
        _ => return None,
    };
    let failed_ref = failed.as_ref().map(|(path, kind)| (path.as_path(), *kind));
    match welcome::show(
        ui,
        error.as_deref(),
        failed_ref,
        recent_files,
        pinned_folders,
    ) {
        Some(welcome::WelcomeAction::NewProject) => {
            open_next = Some(Nav::NewDesign);
        }
        Some(welcome::WelcomeAction::OpenFile) => {
            loader::spawn_pick_file(tx.clone(), ctx.clone());
        }
        Some(welcome::WelcomeAction::OpenFolder) => {
            loader::spawn_pick_folder(tx.clone(), ctx.clone());
        }
        Some(welcome::WelcomeAction::OpenSettings) => {
            *show_settings = true;
        }
        Some(welcome::WelcomeAction::OpenRecent(path)) => {
            open_next = Some(Nav::Open(path));
        }
        Some(welcome::WelcomeAction::RemoveRecent(path)) => {
            recent_files.retain(|p| p != &path);
        }
        Some(welcome::WelcomeAction::PinRecent(path)) => {
            if !pinned_folders.contains(&path) {
                pinned_folders.insert(0, path);
            }
        }
        Some(welcome::WelcomeAction::UnpinRecent(path)) => {
            pinned_folders.retain(|p| p != &path);
        }
        Some(welcome::WelcomeAction::Repair(path)) => {
            loader::spawn_repair_png(path, tx.clone(), ctx.clone());
        }
        Some(welcome::WelcomeAction::Quarantine(path)) => {
            loader::spawn_gallery_op(
                loader::GalleryOp::Quarantine { path },
                false,
                tx.clone(),
                ctx.clone(),
            );
            // El archivo ya no está: el texto que lo nombraba queda obsoleto.
            if let View::Welcome {
                error,
                failed_path,
                failed_kind,
            } = view
            {
                *error = None;
                *failed_path = None;
                *failed_kind = None;
            }
        }
        None => {}
    }
    open_next
}
