//! Estado del panel «Images» que vive en `EditorState`: consulta, filtros,
//! resultados y el arrastre en curso hacia el lienzo.

use std::sync::mpsc::Sender;

use eframe::egui;

use crate::loader;

use super::types::{Photo, SearchFilters};

/// Estado del panel «Images» del sidebar del editor: consulta, filtros,
/// resultados (con sus miniaturas ya subidas a GPU) y errores. Vive en
/// `EditorState`.
#[derive(Default)]
pub struct Panel {
    pub query: String,
    /// Filtros activos de la búsqueda (orientación, color, orden).
    pub filters: SearchFilters,
    /// Última página cargada (1-based).
    pub page: u32,
    /// Hay una búsqueda o descarga de lote en vuelo (desactiva la UI).
    pub searching: bool,
    pub photos: Vec<PhotoItem>,
    pub error: Option<String>,
    /// Se llegó a la última página: «Load more» desaparece y se muestra un
    /// aviso de fin de resultados.
    pub reached_end: bool,
    /// Id de la foto cuya imagen completa se está descargando para insertar.
    pub inserting: Option<String>,
    /// Foto de Unsplash arrastrada y soltada sobre el lienzo: su id y la
    /// posición de página donde debe caer. Se consume en
    /// `on_unsplash_image_ready`; si es `None`, el clic inserta centrada.
    pub pending_drop: Option<(String, (f64, f64))>,
    /// Contador de búsquedas lanzadas: descarta respuestas caducas (llegarían
    /// con los filtros/consulta anteriores).
    pub search_seq: u64,
}

/// Un resultado con su miniatura (si ya llegó del worker).
pub struct PhotoItem {
    pub photo: Photo,
    pub thumb: Option<egui::TextureHandle>,
    pub thumb_failed: bool,
}

/// Payload del arrastre de una foto de Unsplash hacia el lienzo: lo que el
/// canvas necesita para lanzar la descarga si la sueltan sobre él. Viaja por
/// el drag & drop de egui (`dnd_drag_source` → `dnd_release_payload`).
#[derive(Clone)]
pub struct DragUnsplash {
    pub id: String,
    pub label: String,
    pub url: String,
}

impl Panel {
    /// Una foto de Unsplash se ha soltado sobre el lienzo en `page_pos`:
    /// recuerda el destino (para que `on_unsplash_image_ready` la coloque
    /// ahí en vez de centrada) y lanza la descarga, igual que el clic.
    /// No hace nada si otra descarga ya está en vuelo.
    pub fn drop_on_canvas(
        &mut self,
        payload: DragUnsplash,
        page_pos: (f64, f64),
        tx: &Sender<loader::AppMsg>,
        ctx: &egui::Context,
    ) {
        if self.inserting.is_some() {
            return;
        }
        self.inserting = Some(payload.id.clone());
        self.pending_drop = Some((payload.id.clone(), page_pos));
        loader::spawn_unsplash_image(
            payload.id,
            payload.label,
            payload.url,
            tx.clone(),
            ctx.clone(),
        );
    }
}
