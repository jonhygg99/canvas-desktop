//! Vista de bienvenida: accesos rapidos (nuevo diseno, abrir archivo o
//! carpeta, recientes) cuando no hay ningun proyecto abierto.

use std::path::PathBuf;
use std::sync::mpsc::Sender;

use eframe::egui;

use crate::loader::AppMsg;
use crate::{loader, welcome};

use super::super::Nav;

/// Vista de bienvenida: accesos rápidos (nuevo diseño, abrir archivo/
/// carpeta, recientes) cuando no hay ningún proyecto abierto.
pub(in crate::app) fn welcome_view_ui(
    ui: &mut egui::Ui,
    error: Option<&str>,
    recent_files: &[PathBuf],
    last_page_size: (f64, f64),
    show_settings: &mut bool,
    tx: &Sender<AppMsg>,
    ctx: &egui::Context,
) -> Option<Nav> {
    let mut open_next = None;
    match welcome::show(ui, error, recent_files, last_page_size) {
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
        None => {}
    }
    open_next
}
