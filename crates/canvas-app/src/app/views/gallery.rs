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
        // Reintento tras dar permiso en Ajustes (macOS): mismo camino que
        // abrir la carpeta (relanza el escaneo completo); como ya estamos
        // en ella, navigation_to_folder no duplica el historial.
        Some(gallery::GalleryAction::RetryScan) => {
            let (path, navigation) = g.navigation_to_folder(g.folder.clone());
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

    // Reintento automatico del listado en montajes de nube: si el error
    // persiste, hay un ciclo disponible y ningun bucle en marcha, se lanza
    // con backoff (un solo ciclo por visita hasta nueva accion del usuario).
    if g.take_folder_auto_refresh(canvas_io::is_cloud_storage_path(&g.folder)) {
        loader::spawn_folders_auto_refresh(g.folder.clone(), tx.clone(), ctx.clone());
    }

    open_next
}
