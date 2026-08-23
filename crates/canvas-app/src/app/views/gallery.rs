//! Vista de galeria: la cuadricula de miniaturas de una carpeta.

use std::sync::mpsc::Sender;

use eframe::egui;

use crate::loader::AppMsg;
use crate::{deck, gallery, loader, settings};

use super::super::Nav;

/// Vista de galería: cuadrícula de una carpeta, con sus operaciones de
/// archivo (crear/duplicar/pegar/renombrar/borrar) siempre en un hilo aparte.
pub(in crate::app) fn gallery_view_ui(
    g: &mut gallery::GalleryState,
    ui: &mut egui::Ui,
    settings: &mut settings::AppSettings,
    pending_deck: &mut Option<deck::DeckSeed>,
    tx: &Sender<AppMsg>,
    ctx: &egui::Context,
) -> Option<Nav> {
    let mut open_next = None;
    match gallery::show(g, ui) {
        Some(gallery::GalleryAction::CycleFolderPanelSide) => {
            g.folder_panel_side = gallery::next_folder_panel_side(g.folder_panel_side);
            settings.gallery_folder_panel_side = g.folder_panel_side;
            settings.save_in_background();
        }
        Some(gallery::GalleryAction::Open(path)) => {
            // Se lleva las miniaturas ya cargadas al editor: si el archivo
            // resulta tener hermanos, la tira arranca sin parpadeo de ⏳
            // (`resolve_deck` la consume al terminar de cargar).
            *pending_deck = Some(deck::DeckSeed::from_gallery(g));
            open_next = Some(Nav::Open(path));
        }
        Some(gallery::GalleryAction::OpenFolder(folder)) => {
            let (path, navigation) = g.navigation_to_folder(folder);
            open_next = Some(Nav::OpenGallery { path, navigation });
        }
        Some(gallery::GalleryAction::Back) => {
            if let Some((path, navigation)) = g.navigation_back() {
                open_next = Some(Nav::OpenGallery { path, navigation });
            }
        }
        Some(gallery::GalleryAction::Forward) => {
            if let Some((path, navigation)) = g.navigation_forward() {
                open_next = Some(Nav::OpenGallery { path, navigation });
            }
        }
        Some(gallery::GalleryAction::SortChanged(sort)) => {
            settings.gallery_sort = sort;
            settings.save_in_background();
        }
        Some(gallery::GalleryAction::NewDesign) => {
            let seed = deck::DeckSeed::from_gallery(g);
            open_next = Some(Nav::NewDesignInFolder { seed });
        }
        Some(gallery::GalleryAction::Duplicate(path)) => {
            loader::spawn_gallery_op(
                loader::GalleryOp::Duplicate { path },
                false,
                tx.clone(),
                ctx.clone(),
            );
        }
        Some(gallery::GalleryAction::PasteHere(src)) => {
            loader::spawn_gallery_op(
                loader::GalleryOp::CopyInto {
                    src,
                    folder: g.folder.clone(),
                },
                false,
                tx.clone(),
                ctx.clone(),
            );
        }
        Some(gallery::GalleryAction::Rename(path, new_stem)) => {
            loader::spawn_gallery_op(
                loader::GalleryOp::Rename { path, new_stem },
                false,
                tx.clone(),
                ctx.clone(),
            );
        }
        Some(gallery::GalleryAction::RenameFolder(path, new_name)) => {
            loader::spawn_gallery_op(
                loader::GalleryOp::RenameFolder { path, new_name },
                false,
                tx.clone(),
                ctx.clone(),
            );
        }
        Some(gallery::GalleryAction::DeleteFolder(path)) => {
            loader::spawn_gallery_op(
                loader::GalleryOp::DeleteFolder { path },
                false,
                tx.clone(),
                ctx.clone(),
            );
        }
        Some(gallery::GalleryAction::CreateFolder(parent, name)) => {
            loader::spawn_gallery_op(
                loader::GalleryOp::CreateFolder { parent, name },
                false,
                tx.clone(),
                ctx.clone(),
            );
        }
        Some(gallery::GalleryAction::Delete(path)) => {
            loader::spawn_gallery_op(
                loader::GalleryOp::Delete { path },
                false,
                tx.clone(),
                ctx.clone(),
            );
        }
        None => {}
    }
    open_next
}
