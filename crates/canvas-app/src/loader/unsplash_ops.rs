//! Búsqueda en Unsplash en hilos aparte: la llamada a la API, la descarga
//! de miniaturas y la de la imagen completa. Nada de red ni de decodificado
//! toca la UI; los resultados viajan por el canal del workspace (`AppMsg`),
//! igual que el resto del loader.

use std::sync::mpsc::Sender;

use eframe::egui;

use super::AppMsg;

/// Lanza la búsqueda de `query` (página `page`, 1-based). La respuesta trae
/// las fotos SIN miniaturas: el handler las pide una a una con
/// `spawn_unsplash_thumb`.
pub fn spawn_unsplash_search(query: String, page: u32, tx: Sender<AppMsg>, ctx: egui::Context) {
    std::thread::spawn(move || {
        let result = crate::unsplash::search(&query, page);
        let _ = tx.send(AppMsg::UnsplashSearch { query, page, result });
        ctx.request_repaint();
    });
}

/// Descarga y decodifica la miniatura de un resultado para mostrarla en la
/// rejilla del panel.
pub fn spawn_unsplash_thumb(id: String, url: String, tx: Sender<AppMsg>, ctx: egui::Context) {
    std::thread::spawn(move || {
        let result =
            crate::unsplash::download(&url).and_then(|bytes| crate::unsplash::decode(&bytes));
        let _ = tx.send(AppMsg::UnsplashThumb { id, result });
        ctx.request_repaint();
    });
}

/// Descarga y decodifica la imagen completa de una foto para insertarla como
/// capa nueva del documento abierto.
pub fn spawn_unsplash_image(
    id: String,
    label: String,
    url: String,
    tx: Sender<AppMsg>,
    ctx: egui::Context,
) {
    std::thread::spawn(move || {
        let result =
            crate::unsplash::download(&url).and_then(|bytes| crate::unsplash::decode(&bytes));
        let _ = tx.send(AppMsg::UnsplashImageReady { id, label, result });
        ctx.request_repaint();
    });
}
