//! Cuerpo de la pestaña «Images»: el selector de fuente arriba y, debajo, el
//! buscador elegido —Serper para Google e Instagram/Facebook, Unsplash para
//! su propia API—. Los dos buscadores conservan su estado en `EditorState`
//! (`serper` y `unsplash`), así que alternar no pierde resultados: el único
//! cambio que reinicia algo es el salto entre los DOS corpus de Serper, que
//! es lo que hacía `switch_mode` cuando eran pestañas distintas.
//!
//! El destino del bulk de Serper se resuelve aquí (baraja → archivo abierto →
//! galería → última carpeta usada) porque necesita la carpeta de la baraja y
//! el archivo abierto, que la pestaña solo conoce en este punto.

use std::sync::mpsc::Sender;

use eframe::egui;

use crate::editor::state::ImageSource;
use crate::editor::EditorState;
use crate::loader::{AppMsg, ImageInsertDest};
use crate::settings::AppSettings;

use super::source_selector::{apply_source, source_selector_ui};

/// Contenido de la pestaña «Images». `deck_folder` alimenta el destino del
/// bulk junto al archivo abierto; `insert_dest` es la ranura que recibirá un
/// clic de inserción (A07).
pub(super) fn images_tab_ui(
    state: &mut EditorState,
    settings: &mut AppSettings,
    deck_folder: Option<std::path::PathBuf>,
    insert_dest: ImageInsertDest,
    ui: &mut egui::Ui,
    tx: &Sender<AppMsg>,
) {
    ui.add_space(6.0);
    source_selector_ui(&mut state.image_source, ui);
    apply_source(&mut state.serper, state.image_source);
    ui.add_space(4.0);
    match state.image_source {
        ImageSource::Unsplash => crate::unsplash::panel_ui(state, insert_dest, ui, tx),
        ImageSource::Web | ImageSource::Social => {
            let dest = crate::serper::bulk::resolve_bulk_folder(
                deck_folder,
                state.doc.source_path.clone(),
                state.from_gallery.clone(),
                settings.serper_last_folder.clone(),
            );
            crate::serper::panel_ui(&mut state.serper, settings, dest, insert_dest, ui, tx);
        }
    }
}
